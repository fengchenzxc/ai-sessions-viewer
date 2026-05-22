// Claude Code 会话源：~/.claude/projects/<dir>/<sessionId>.jsonl
//
// 每行是 `{ "type": "user" | "assistant" | "custom-title" | ..., ... }`，
// user/assistant 的 `message.content` 数组里夹着 text / thinking / tool_use /
// tool_result / image 等块。

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::SessionSource;
use crate::types::{Block, DiffHunk, DiffLine, Msg, ProjectInfo, SessionMeta, SessionPage};
use crate::util::{
    append_jsonl_line, clean_title, home, is_jsonl, mtime_millis, text_block, validate_rename_name,
};

pub struct ClaudeSource;

fn projects_dir() -> PathBuf {
    home().join(".claude").join("projects")
}

impl SessionSource for ClaudeSource {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn list_projects(&self) -> Result<Vec<ProjectInfo>, String> {
        let dir = projects_dir();
        let mut out = Vec::new();
        let entries = fs::read_dir(&dir).map_err(|e| format!("读取项目目录失败: {e}"))?;
        for e in entries.flatten() {
            let path = e.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = e.file_name().to_string_lossy().to_string();
            let mut count = 0usize;
            let mut last = 0u64;
            let mut cwd: Option<String> = None;
            if let Ok(files) = fs::read_dir(&path) {
                for f in files.flatten() {
                    let fp = f.path();
                    if is_jsonl(&fp) {
                        count += 1;
                        let m = mtime_millis(&fp);
                        if m > last {
                            last = m;
                        }
                        if cwd.is_none() {
                            cwd = first_cwd(&fp);
                        }
                    }
                }
            }
            if count == 0 {
                continue;
            }
            let display_path = cwd.unwrap_or_else(|| dir_name.replace('-', "/"));
            let exists = Path::new(&display_path).is_dir();
            out.push(ProjectInfo {
                dir_name,
                display_path,
                session_count: count,
                last_modified: last,
                exists,
            });
        }
        out.sort_by_key(|p| std::cmp::Reverse(p.last_modified));
        Ok(out)
    }

    fn list_sessions(
        &self,
        project_key: &str,
        offset: usize,
        limit: usize,
    ) -> Result<SessionPage, String> {
        let pdir = projects_dir().join(project_key);
        let mut files: Vec<(PathBuf, u64)> = Vec::new();
        let entries = fs::read_dir(&pdir).map_err(|e| format!("读取会话目录失败: {e}"))?;
        for f in entries.flatten() {
            let fp = f.path();
            if is_jsonl(&fp) {
                let mt = mtime_millis(&fp);
                files.push((fp, mt));
            }
        }
        files.sort_by_key(|f| std::cmp::Reverse(f.1));
        let total = files.len();
        let sessions = files
            .iter()
            .skip(offset)
            .take(limit)
            .map(|(p, _)| scan(p))
            .collect();
        Ok(SessionPage { total, sessions })
    }

    fn read_session(&self, path: &str) -> Result<Vec<Msg>, String> {
        read(path)
    }

    fn rename_session(&self, path: &Path, name: &str) -> Result<(), String> {
        let trimmed = validate_rename_name(name)?;
        let id = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.trim_end_matches(".jsonl").to_string())
            .unwrap_or_default();
        // Claude Code `/rename` 会成对追加 custom-title + agent-name 两条记录
        // （同值）。这里照搬，保证 claude CLI 与本应用互认。
        let title_line = serde_json::json!({
            "type": "custom-title",
            "customTitle": trimmed,
            "sessionId": id,
        })
        .to_string();
        let agent_line = serde_json::json!({
            "type": "agent-name",
            "agentName": trimmed,
            "sessionId": id,
        })
        .to_string();
        append_jsonl_line(path, &title_line)?;
        append_jsonl_line(path, &agent_line)?;
        // 运行时镜像：若该会话当前有运行中的 claude 进程，更新对应 PID.json
        // 的 name。是 best-effort，找不到 / 失败都不影响持久标题。
        mirror_runtime_name(&id, trimmed);
        Ok(())
    }

    fn trash_title(&self, path: &Path) -> String {
        scan(path).title
    }

    fn resume_cli(&self, session_id: &str) -> String {
        format!("claude --resume {session_id}")
    }

    fn new_session_cli(&self) -> String {
        "claude".to_string()
    }

    fn image_src(&self, block: &Value) -> Option<String> {
        image_src(block)
    }
}

// ----- 内部解析 --------------------------------------------------------------

fn first_cwd(fp: &Path) -> Option<String> {
    let file = fs::File::open(fp).ok()?;
    for line in BufReader::new(file).lines().map_while(Result::ok).take(12) {
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if let Some(c) = v.get("cwd").and_then(|x| x.as_str()) {
                return Some(c.to_string());
            }
        }
    }
    None
}

/// 用户在 Claude 处理过程中排队输入的消息会被记成
/// `{"type":"attachment","attachment":{"type":"queued_command","prompt":...}}`，
/// 而非常规的 `type:"user"` 记录。把其中的 `prompt` 解析成消息块：纯文本排队
/// 消息的 `prompt` 是字符串，带贴图的则是 text / image 块数组。非排队命令的
/// attachment（hook_success / task_reminder / diagnostics 等）返回 None。
fn queued_command_blocks(v: &Value) -> Option<Vec<Block>> {
    let att = v.get("attachment")?;
    if att.get("type").and_then(|x| x.as_str()) != Some("queued_command") {
        return None;
    }
    let mut blocks = Vec::new();
    match att.get("prompt")? {
        Value::String(s) if !s.trim().is_empty() => {
            blocks.push(text_block("text", s));
        }
        Value::Array(arr) => {
            for el in arr {
                match el.get("type").and_then(|x| x.as_str()) {
                    Some("text") => {
                        if let Some(s) = el.get("text").and_then(|x| x.as_str()) {
                            if !s.trim().is_empty() {
                                blocks.push(text_block("text", s));
                            }
                        }
                    }
                    Some("image") => {
                        if let Some(src) = image_src(el) {
                            blocks.push(Block {
                                kind: "image".to_string(),
                                image_src: Some(src),
                                ..Default::default()
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    if blocks.is_empty() {
        None
    } else {
        Some(blocks)
    }
}

fn user_text(v: &Value) -> Option<String> {
    let content = v.get("message")?.get("content")?;
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(arr) => {
            for el in arr {
                if el.get("type").and_then(|x| x.as_str()) == Some("text") {
                    if let Some(s) = el.get("text").and_then(|x| x.as_str()) {
                        return Some(s.to_string());
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Claude: `{"type":"image","source":{"type":"base64"|"url", ...}}`
fn image_src(el: &Value) -> Option<String> {
    if el.get("type").and_then(|x| x.as_str()) != Some("image") {
        return None;
    }
    let source = el.get("source")?;
    let src_type = source.get("type").and_then(|x| x.as_str()).unwrap_or("");
    if src_type == "base64" {
        let media = source
            .get("media_type")
            .and_then(|x| x.as_str())
            .unwrap_or("image/png");
        let data = source.get("data").and_then(|x| x.as_str())?;
        return Some(format!("data:{media};base64,{data}"));
    }
    if src_type == "url" {
        return source
            .get("url")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
    }
    None
}

/// 判断这条 user 消息是不是 Claude Code 紧跟在真实贴图之后写下的图片元引用，
/// 形如 `[Image: source: <local-path>]` 或 `[Image: original WxH, displayed at ...]`。
/// 真正的贴图已经在上一条 user 记录里以 base64 渲染过了，这种纯元数据直接丢弃。
/// 一条 user 记录可能携带多张图（content 数组里多个 text block），只要全是这类
/// 元引用就整体跳过。
fn is_image_source_meta(v: &Value, blocks: &[Block]) -> bool {
    let is_meta = v
        .get("isMeta")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if !is_meta {
        return false;
    }
    if blocks.is_empty() {
        return false;
    }
    blocks.iter().all(|b| {
        if b.kind != "text" {
            return false;
        }
        let txt = b.text.as_deref().unwrap_or("").trim();
        if !txt.starts_with("[Image:") || !txt.ends_with(']') {
            return false;
        }
        let inner = txt.trim_start_matches("[Image:").trim_start();
        inner.starts_with("source:") || inner.starts_with("original")
    })
}

fn stringify_tool_result(c: Option<&Value>) -> String {
    match c {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => {
            let mut parts = Vec::new();
            for el in arr {
                match el.get("type").and_then(|x| x.as_str()) {
                    Some("text") => {
                        if let Some(s) = el.get("text").and_then(|x| x.as_str()) {
                            parts.push(s.to_string());
                        }
                    }
                    Some("image") => parts.push("[图片]".to_string()),
                    _ => {}
                }
            }
            parts.join("\n")
        }
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// 把 Claude 的 structuredPatch 解析成带行号的 diff。
fn parse_structured_patch(v: &Value) -> Option<Vec<DiffHunk>> {
    let arr = v.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let mut hunks = Vec::new();
    for h in arr {
        let old_start = h.get("oldStart").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        let new_start = h.get("newStart").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        let mut old_no = old_start;
        let mut new_no = new_start;
        let mut lines = Vec::new();
        if let Some(raw) = h.get("lines").and_then(|x| x.as_array()) {
            for l in raw {
                let s = l.as_str().unwrap_or("");
                let (kind, text): (&str, &str) = match s.chars().next() {
                    Some('+') => ("add", &s[1..]),
                    Some('-') => ("del", &s[1..]),
                    _ => ("ctx", s.strip_prefix(' ').unwrap_or(s)),
                };
                let (o, n) = match kind {
                    "add" => {
                        let n = new_no;
                        new_no += 1;
                        (None, Some(n))
                    }
                    "del" => {
                        let o = old_no;
                        old_no += 1;
                        (Some(o), None)
                    }
                    _ => {
                        let (o, n) = (old_no, new_no);
                        old_no += 1;
                        new_no += 1;
                        (Some(o), Some(n))
                    }
                };
                lines.push(DiffLine {
                    kind: kind.to_string(),
                    old_no: o,
                    new_no: n,
                    text: text.to_string(),
                });
            }
        }
        hunks.push(DiffHunk {
            old_start,
            new_start,
            lines,
        });
    }
    Some(hunks)
}

/// 把新标题镜像到 ~/.claude/sessions/<PID>.json 的 name 字段。
/// 这是 Claude Code 运行时维护的会话态文件，按 sessionId 找到匹配项，
/// 只改 name、保留其余字段。是 best-effort：找不到 / 解析失败 / 写失败都静默跳过，
/// 不影响 jsonl 里的持久标题。
fn mirror_runtime_name(session_id: &str, name: &str) {
    let dir = home().join(".claude").join("sessions");
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let content = match fs::read_to_string(&p) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut v: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("sessionId").and_then(|x| x.as_str()) != Some(session_id) {
            continue;
        }
        if let Some(obj) = v.as_object_mut() {
            obj.insert("name".to_string(), Value::String(name.to_string()));
            if let Ok(serialized) = serde_json::to_string(&v) {
                let _ = fs::write(&p, serialized);
            }
        }
    }
}

/// 单遍扫描一个 jsonl，提取标题 / 时间 / 消息数等元信息。
fn scan(fp: &Path) -> SessionMeta {
    let file_name = fp
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let id = file_name.trim_end_matches(".jsonl").to_string();
    let size = fs::metadata(fp).map(|m| m.len()).unwrap_or(0);
    let modified = mtime_millis(fp);

    // Claude Code `/rename <name>` 会成对追加 `custom-title` + `agent-name`
    // 两条记录（同值）。两者都识别，最后一条生效；否则回落到首条 user message。
    let mut first_user_title = String::new();
    let mut custom_title: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut created: Option<String> = None;
    let mut message_count = 0usize;

    if let Ok(file) = fs::File::open(fp) {
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
            if cwd.is_none() {
                if let Some(c) = v.get("cwd").and_then(|x| x.as_str()) {
                    cwd = Some(c.to_string());
                }
            }
            if t == "custom-title" || t == "agent-name" {
                let field = if t == "custom-title" {
                    "customTitle"
                } else {
                    "agentName"
                };
                if let Some(ct) = v.get(field).and_then(|x| x.as_str()) {
                    let trimmed = ct.trim();
                    if !trimmed.is_empty() {
                        custom_title = Some(trimmed.to_string());
                    }
                }
                continue;
            }
            if t == "user" || t == "assistant" {
                if created.is_none() {
                    created = v
                        .get("timestamp")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string());
                }
                message_count += 1;
            }
            // 排队输入的消息（attachment/queued_command）也算一条用户消息。
            if t == "attachment" && queued_command_blocks(&v).is_some() {
                message_count += 1;
            }
            if first_user_title.is_empty() && t == "user" {
                if let Some(txt) = user_text(&v) {
                    let clean = clean_title(&txt);
                    if !clean.is_empty() {
                        first_user_title = clean;
                    }
                }
            }
        }
    }
    let title = custom_title.unwrap_or_else(|| {
        if first_user_title.is_empty() {
            "(无标题会话)".to_string()
        } else {
            first_user_title
        }
    });
    SessionMeta {
        id,
        file_name,
        path: fp.to_string_lossy().to_string(),
        title,
        cwd,
        created,
        modified,
        size,
        message_count,
    }
}

fn read(path: &str) -> Result<Vec<Msg>, String> {
    let file = fs::File::open(path).map_err(|e| format!("打开会话失败: {e}"))?;
    let mut msgs = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        // 用户在 Claude 处理中排队输入的消息不是常规 user 记录，而是
        // `attachment`（attachment.type == "queued_command"）。常规解析只认
        // user/assistant，会整条丢掉它 —— 这里单独补成一条 user 气泡。
        if t == "attachment" {
            if let Some(blocks) = queued_command_blocks(&v) {
                msgs.push(Msg {
                    uuid: v.get("uuid").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    role: "user".to_string(),
                    timestamp: v
                        .get("timestamp")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                    model: None,
                    sidechain: v
                        .get("isSidechain")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false),
                    blocks,
                });
            }
            continue;
        }
        if t != "user" && t != "assistant" {
            continue;
        }
        let sidechain = v
            .get("isSidechain")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let uuid = v
            .get("uuid")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let timestamp = v
            .get("timestamp")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let message = v.get("message");
        let model = message
            .and_then(|m| m.get("model"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());

        let mut blocks = Vec::new();
        if let Some(content) = message.and_then(|m| m.get("content")) {
            match content {
                Value::String(s) if !s.trim().is_empty() => {
                    blocks.push(text_block("text", s));
                }
                Value::Array(arr) => {
                    for el in arr {
                        let et = el.get("type").and_then(|x| x.as_str()).unwrap_or("");
                        match et {
                            "text" => {
                                if let Some(s) = el.get("text").and_then(|x| x.as_str()) {
                                    if !s.trim().is_empty() {
                                        blocks.push(text_block("text", s));
                                    }
                                }
                            }
                            "thinking" => {
                                if let Some(s) = el.get("thinking").and_then(|x| x.as_str()) {
                                    if !s.trim().is_empty() {
                                        blocks.push(text_block("thinking", s));
                                    }
                                }
                            }
                            "tool_use" => {
                                let name = el
                                    .get("name")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("tool")
                                    .to_string();
                                let input = el
                                    .get("input")
                                    .map(|i| serde_json::to_string_pretty(i).unwrap_or_default());
                                let id = el
                                    .get("id")
                                    .and_then(|x| x.as_str())
                                    .map(|s| s.to_string());
                                blocks.push(Block {
                                    kind: "tool_use".to_string(),
                                    tool_name: Some(name),
                                    tool_input: input,
                                    tool_id: id,
                                    ..Default::default()
                                });
                            }
                            "tool_result" => {
                                let id = el
                                    .get("tool_use_id")
                                    .and_then(|x| x.as_str())
                                    .map(|s| s.to_string());
                                let is_error = el
                                    .get("is_error")
                                    .and_then(|x| x.as_bool())
                                    .unwrap_or(false);
                                let txt = stringify_tool_result(el.get("content"));
                                // 同一条记录顶层的 toolUseResult 携带文件改动的结构化 diff。
                                let tur = v.get("toolUseResult");
                                let file_path = tur
                                    .and_then(|t| t.get("filePath"))
                                    .and_then(|x| x.as_str())
                                    .map(|s| s.to_string());
                                let diff = tur
                                    .and_then(|t| t.get("structuredPatch"))
                                    .and_then(parse_structured_patch);
                                blocks.push(Block {
                                    kind: "tool_result".to_string(),
                                    text: Some(txt),
                                    tool_id: id,
                                    is_error,
                                    file_path,
                                    diff,
                                    ..Default::default()
                                });
                            }
                            "image" => {
                                if let Some(src) = image_src(el) {
                                    blocks.push(Block {
                                        kind: "image".to_string(),
                                        image_src: Some(src),
                                        ..Default::default()
                                    });
                                } else {
                                    blocks.push(text_block("text", "[图片]"));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        if blocks.is_empty() {
            continue;
        }
        // Claude 把用户贴图拆成两条 user 记录：一条是带 base64 的真实消息，
        // 紧跟一条 `isMeta:true` 的 `[Image: source: <local-path>]` 引用。
        // 已经在上一条里渲染过真实图，跳过 meta 那条避免出现重复气泡。
        if t == "user" && is_image_source_meta(&v, &blocks) {
            continue;
        }
        msgs.push(Msg {
            uuid,
            role: t.to_string(),
            timestamp,
            model,
            sidechain,
            blocks,
        });
    }
    Ok(msgs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_text_queued_command() {
        let v = json!({
            "type": "attachment",
            "attachment": { "type": "queued_command", "prompt": "改完看 readme" },
        });
        let blocks = queued_command_blocks(&v).expect("text prompt");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, "text");
        assert_eq!(blocks[0].text.as_deref(), Some("改完看 readme"));
    }

    #[test]
    fn extracts_queued_command_with_image() {
        // 带贴图的排队消息：prompt 是 text + image 数组，图片不能丢。
        let v = json!({
            "type": "attachment",
            "attachment": { "type": "queued_command", "prompt": [
                { "type": "text", "text": "[Image #10]" },
                { "type": "image", "source": {
                    "type": "base64", "media_type": "image/png", "data": "AAAA" } },
            ] },
        });
        let blocks = queued_command_blocks(&v).expect("text + image prompt");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, "text");
        assert_eq!(blocks[1].kind, "image");
        assert_eq!(
            blocks[1].image_src.as_deref(),
            Some("data:image/png;base64,AAAA"),
        );
    }

    #[test]
    fn ignores_non_queued_attachments() {
        // hook_success / task_reminder / diagnostics 等 attachment 不是用户消息
        let v = json!({
            "type": "attachment",
            "attachment": { "type": "hook_success", "content": "OK" },
        });
        assert!(queued_command_blocks(&v).is_none());
    }

    #[test]
    fn ignores_blank_queued_prompt() {
        let v = json!({
            "type": "attachment",
            "attachment": { "type": "queued_command", "prompt": "   " },
        });
        assert!(queued_command_blocks(&v).is_none());
    }
}
