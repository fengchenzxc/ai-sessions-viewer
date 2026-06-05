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
use crate::stats::{
    pricing, shell as shell_util,
    types::{CallRecord, Turn},
};
use crate::types::{
    Block, DiffHunk, DiffLine, Msg, ProjectInfo, SessionMeta, SessionPage, UsageSummary,
};
use crate::util::{
    append_jsonl_line, clean_title, home, is_jsonl, mtime_millis, parse_iso8601_ms, text_block,
    validate_rename_name,
};

pub struct ClaudeSource;

fn projects_dir() -> PathBuf {
    home().join(".claude").join("projects")
}

impl SessionSource for ClaudeSource {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn list_projects(
        &self,
        _include_codex_internal: bool,
        _include_codex_archived: bool,
    ) -> Result<Vec<ProjectInfo>, String> {
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
        _include_codex_internal: bool,
        _include_codex_archived: bool,
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

    fn discover_stats_sessions(&self, project_key: &str) -> Result<Vec<SessionMeta>, String> {
        let pdir = projects_dir().join(project_key);
        let mut out: Vec<SessionMeta> = Vec::new();
        let entries = fs::read_dir(&pdir).map_err(|e| format!("读取会话目录失败: {e}"))?;
        for f in entries.flatten() {
            let path = f.path();
            if is_jsonl(&path) {
                out.push(scan(&path));
                continue;
            }
            // <sessionId>/subagents/*.jsonl —— 子代理产生的独立 JSONL，
            // 是真实的 API 调用且独立计费。codeburn 用同名 collectJsonlFiles 逻辑。
            // 不进 list_sessions（避免污染聊天列表），只进统计扫描。
            if path.is_dir() {
                let sub = path.join("subagents");
                if let Ok(sub_entries) = fs::read_dir(&sub) {
                    for sf in sub_entries.flatten() {
                        let sp = sf.path();
                        if is_jsonl(&sp) {
                            out.push(scan(&sp));
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// 单会话同伴文件：`<projects>/<projectKey>/<sessionId>.jsonl` 的旁边可能
    /// 有 `<projects>/<projectKey>/<sessionId>/subagents/*.jsonl`。把它们也算入
    /// 单会话统计，跟全局 by-session 的口径一致（codeburn 同样做法）。
    fn discover_session_companions(&self, path: &str) -> Vec<SessionMeta> {
        let parent_path = Path::new(path);
        // parent.with_extension("") -> "<projects>/<projectKey>/<sessionId>"
        let sub_dir = parent_path.with_extension("").join("subagents");
        let Ok(entries) = fs::read_dir(&sub_dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for sf in entries.flatten() {
            let sp = sf.path();
            if is_jsonl(&sp) {
                out.push(scan(&sp));
            }
        }
        out
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

    fn resume_cli(&self, session_id: &str, _path: &str) -> String {
        format!("claude --resume {session_id}")
    }

    fn new_session_cli(&self) -> String {
        "claude".to_string()
    }

    fn image_src(&self, block: &Value) -> Option<String> {
        image_src(block)
    }

    fn usage_summary(&self, path: &str) -> Result<UsageSummary, String> {
        usage_summary(Path::new(path))
    }

    fn read_turns(&self, path: &str) -> Result<Vec<Turn>, String> {
        Ok(read_turns(Path::new(path)))
    }
}

// ----- 内部解析 --------------------------------------------------------------

/// 一次性把整份 JSONL 走一遍，累加每条 assistant 消息里的 `message.usage` 字段。
/// Claude 的形状：
///   {"type":"assistant","message":{"usage":{"input_tokens":N, "output_tokens":N,
///       "cache_creation_input_tokens":N, "cache_read_input_tokens":N, ...}}}
/// user 消息没有 usage；不存在的字段当 0 处理。文件不可读 → 返回 default 而非
/// 错误，避免会话列表里因为一个坏文件整个挂掉 —— 用户看到「0 tokens」也比看到
/// 全列表挂掉好。
fn usage_summary(fp: &Path) -> Result<UsageSummary, String> {
    let file = match fs::File::open(fp) {
        Ok(f) => f,
        Err(_) => return Ok(UsageSummary::default()),
    };
    let mut acc = UsageSummary::default();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let usage = v
            .get("message")
            .and_then(|m| m.get("usage"))
            .or_else(|| v.get("usage"));
        let Some(u) = usage else { continue };
        acc.input_tokens += u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
        acc.output_tokens += u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
        acc.cache_creation_input_tokens += u
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        acc.cache_read_input_tokens += u
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
    }
    Ok(acc.finalize())
}

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
    let is_meta = v.get("isMeta").and_then(|x| x.as_bool()).unwrap_or(false);
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
/// Subagent JSONL 的路径形态：`.../<project_dir>/<parent_uuid>/subagents/agent-*.jsonl`。
/// 父目录名是 `subagents` 即认定它是子代理产物。
fn is_subagent_path(fp: &Path) -> bool {
    fp.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        == Some("subagents")
}

fn scan(fp: &Path) -> SessionMeta {
    let file_name = fp
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    // Subagent 文件的 session id 用父 session 的 UUID，让聚合器自然把它们的
    // cost / calls / tokens 合到父 session 下 —— 数据 0 丢失，session 计数不再被
    // inflated（典型场景：sidebar 显示 198 个 session，统计页之前算 298 个，差额
    // ~100 全是 subagent 文件被当成独立 session；现在两处一致）。
    let id = if is_subagent_path(fp) {
        fp.parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| file_name.trim_end_matches(".jsonl").to_string())
    } else {
        file_name.trim_end_matches(".jsonl").to_string()
    };
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
        codex_app_list_rank: None,
        codex_app_list_scanned: 0,
        codex_app_first_page_size: 50,
        codex_app_first_page_position: 0,
        codex_internal: false,
        codex_archived: false,
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
                    uuid: v
                        .get("uuid")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
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
                                let id =
                                    el.get("id").and_then(|x| x.as_str()).map(|s| s.to_string());
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

// ---- read_turns（统计聚合用）---------------------------------------------
//
// 单遍走 JSONL 把每条消息抽成结构化的 Turn / CallRecord。和 `read()` 的区别：
//   - 不返回 UI 用的 Block 结构（thinking / text / image / tool_result 全跳）
//   - 在每个 assistant message 上把 usage / model 顺便挖出来
//   - tool_use 块只关心 name 和 input —— name 直接进 tools，Bash 的 input 抽
//     第一个命令词进 bash_commands；mcp__server__tool 前缀抽 server 进 mcp_servers
//
// 一条 user 消息开启一个 Turn；之后的 assistant 消息持续 push 进该 Turn 的 calls。
// 没有 user 消息打头的孤儿 assistant（很少见但合法）合并到上一个 Turn 末尾。
fn read_turns(fp: &Path) -> Vec<Turn> {
    let session_id = fp
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.trim_end_matches(".jsonl").to_string())
        .unwrap_or_default();
    let file = match fs::File::open(fp) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let mut turns: Vec<Turn> = Vec::new();
    let mut cur: Option<Turn> = None;
    let mut project_path: String = String::new();

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if project_path.is_empty() {
            if let Some(c) = v.get("cwd").and_then(|x| x.as_str()) {
                project_path = c.to_string();
            }
        }
        let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        if t != "user" && t != "assistant" {
            continue;
        }
        let ts_ms = v
            .get("timestamp")
            .and_then(|x| x.as_str())
            .and_then(parse_iso8601_ms)
            .unwrap_or(0);

        if t == "user" {
            // 把上一轮（含 calls 的）写出
            if let Some(prev) = cur.take() {
                turns.push(prev);
            }
            let user_text = user_text(&v).unwrap_or_default();
            cur = Some(Turn {
                user_message: user_text,
                project_path: project_path.clone(),
                session_id: session_id.clone(),
                calls: Vec::new(),
                timestamp_ms: ts_ms,
            });
            continue;
        }

        // assistant
        let message = match v.get("message") {
            Some(m) => m,
            None => continue,
        };
        let model = message
            .get("model")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        // Claude `message.id`（"msg_xxx"）—— 用于跨文件去重。fork / continue / sub-agent
        // JSONL 之间常见同一条 assistant 消息被多个文件抄录，按这个 id 跳过避免翻倍。
        let message_id = message
            .get("id")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        // usage：claude 是 message.usage.{input_tokens, output_tokens, cache_*}
        let mut usage = UsageSummary::default();
        if let Some(u) = message.get("usage") {
            usage.input_tokens = u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
            usage.output_tokens = u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
            // cache_creation 有两种形状：
            //   legacy: cache_creation_input_tokens = 整数（不分 tier）
            //   split:  cache_creation = { ephemeral_5m_input_tokens: N, ephemeral_1h_input_tokens: M }
            // 两者通常同时出现，legacy 字段 = 5m + 1h。我们这里把 total 收齐到
            // `cache_creation_input_tokens`，再把 1h 子集单独记到 `_1h_` 字段供 cost 算 2× 计费。
            let legacy = u
                .get("cache_creation_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let cc = u.get("cache_creation");
            let fivem = cc
                .and_then(|x| x.get("ephemeral_5m_input_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let one_h = cc
                .and_then(|x| x.get("ephemeral_1h_input_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            // 缺哪个用哪个：拼一份 5m + 1h；如果 split 是 0 / 缺失，退回 legacy。
            let split_total = fivem.saturating_add(one_h);
            usage.cache_creation_input_tokens = legacy.max(split_total);
            // 1h 子集要 ≤ total，钳一下防御性。Anthropic 偶尔分裂上报、legacy 缺一拍。
            usage.cache_creation_1h_input_tokens = one_h.min(usage.cache_creation_input_tokens);
            usage.cache_read_input_tokens = u
                .get("cache_read_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            usage = usage.finalize();
        }
        // 工具集合
        let mut tools: Vec<String> = Vec::new();
        let mut bash_commands: Vec<String> = Vec::new();
        let mut mcp_servers: Vec<String> = Vec::new();
        let mut has_agent_spawn = false;
        if let Some(content) = message.get("content").and_then(|x| x.as_array()) {
            for el in content {
                if el.get("type").and_then(|x| x.as_str()) != Some("tool_use") {
                    continue;
                }
                let name = el
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                if matches!(name.as_str(), "Task" | "Agent" | "task_spawn") {
                    has_agent_spawn = true;
                }
                if name == "Bash" || name == "BashTool" {
                    if let Some(input) = el.get("input") {
                        // input 可能是 object 或 string；shell_util 接受字符串
                        let raw = match input {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        if let Some(cmd) = shell_util::extract_first_command(&raw) {
                            bash_commands.push(cmd);
                        }
                    }
                }
                if let Some(server) = shell_util::extract_mcp_server(&name) {
                    mcp_servers.push(server);
                }
                tools.push(name);
            }
        }
        let cost = if usage.total == 0 {
            0.0
        } else {
            pricing::cost_usd(&model, &usage)
        };
        let call = CallRecord {
            model,
            message_id,
            usage,
            cost_usd: cost,
            tools,
            bash_commands,
            mcp_servers,
            has_plan_mode: false, // Claude 不显式记 plan mode；用 ExitPlanMode 工具名兜底判断
            has_agent_spawn,
        };
        if let Some(turn) = cur.as_mut() {
            // 把 ExitPlanMode 工具识别为 plan-mode 标记
            if call.tools.iter().any(|t| t == "ExitPlanMode") {
                turn.calls.push(CallRecord {
                    has_plan_mode: true,
                    ..call
                });
            } else {
                turn.calls.push(call);
            }
        } else {
            // 孤儿 assistant（合法但少见）：起一个空 user_message 的占位 turn
            cur = Some(Turn {
                user_message: String::new(),
                project_path: project_path.clone(),
                session_id: session_id.clone(),
                calls: vec![call],
                timestamp_ms: ts_ms,
            });
        }
    }
    if let Some(t) = cur {
        turns.push(t);
    }
    turns
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

    // ---- usage_summary --------------------------------------------------

    use std::io::Write;

    fn write_temp(name: &str, lines: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("csv-claude-usage-tests");
        let _ = fs::create_dir_all(&dir);
        let p = dir.join(name);
        let mut f = fs::File::create(&p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        p
    }

    #[test]
    fn usage_sums_input_output_cache_across_assistant_messages() {
        let p = write_temp(
            "sum.jsonl",
            &[
                r#"{"type":"user","message":{"content":"hi"}}"#,
                r#"{"type":"assistant","message":{"usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":100,"cache_read_input_tokens":0}}}"#,
                r#"{"type":"assistant","message":{"usage":{"input_tokens":3,"output_tokens":7,"cache_creation_input_tokens":0,"cache_read_input_tokens":100}}}"#,
            ],
        );
        let u = usage_summary(&p).unwrap();
        assert_eq!(u.input_tokens, 13);
        assert_eq!(u.output_tokens, 12);
        assert_eq!(u.cache_creation_input_tokens, 100);
        assert_eq!(u.cache_read_input_tokens, 100);
        assert_eq!(u.reasoning_output_tokens, 0);
        assert_eq!(u.total, 225);
    }

    #[test]
    fn usage_ignores_lines_without_usage() {
        let p = write_temp(
            "no-usage.jsonl",
            &[
                r#"{"type":"user","message":{"content":"hi"}}"#,
                r#"{"type":"system","content":"x"}"#,
            ],
        );
        assert_eq!(usage_summary(&p).unwrap(), UsageSummary::default());
    }

    #[test]
    fn usage_handles_missing_subfields_as_zero() {
        let p = write_temp(
            "partial.jsonl",
            &[
                // 只有 output_tokens，其他字段缺失 —— 不应该挂
                r#"{"type":"assistant","message":{"usage":{"output_tokens":42}}}"#,
            ],
        );
        let u = usage_summary(&p).unwrap();
        assert_eq!(u.output_tokens, 42);
        assert_eq!(u.total, 42);
    }

    #[test]
    fn usage_returns_default_when_file_missing() {
        let p = std::path::PathBuf::from("/tmp/csv-claude-usage-tests/nonexistent.jsonl");
        assert_eq!(usage_summary(&p).unwrap(), UsageSummary::default());
    }

    #[test]
    #[ignore = "manual full-scan; reads every Claude JSONL on disk"]
    fn dedup_full_claude_scan() {
        let src = ClaudeSource;
        let projects = src.list_projects(false, false).unwrap();
        let mut agg = crate::stats::aggregate::Aggregator::new();
        for p in &projects {
            let sessions = src.discover_stats_sessions(&p.dir_name).unwrap_or_default();
            for s in sessions {
                let turns = read_turns(std::path::Path::new(&s.path));
                agg.feed_session(&crate::stats::aggregate::SessionFeed {
                    agent: "claude",
                    project_dir_name: &p.dir_name,
                    project_display: &p.display_path,
                    session_id: &s.id,
                    path: &s.path,
                    title: &s.title,
                    last_modified: s.modified,
                    message_count: s.message_count,
                    turns: &turns,
                });
            }
        }
        let snap = agg.snapshot("claude");
        eprintln!("\n=== FULL CLAUDE SCAN (with dedup + subagents) ===");
        eprintln!("sessions: {}", snap.session_count);
        eprintln!("calls: {}", snap.call_count);
        eprintln!("cost: ${:.2}", snap.cost_usd);
        eprintln!(
            "input: {} ({:.1}M)",
            snap.usage.input_tokens,
            snap.usage.input_tokens as f64 / 1e6
        );
        eprintln!(
            "output: {} ({:.1}M)",
            snap.usage.output_tokens,
            snap.usage.output_tokens as f64 / 1e6
        );
        eprintln!(
            "cache_read: {} ({:.1}M)",
            snap.usage.cache_read_input_tokens,
            snap.usage.cache_read_input_tokens as f64 / 1e6
        );
        eprintln!(
            "cache_write: {} ({:.1}M)",
            snap.usage.cache_creation_input_tokens,
            snap.usage.cache_creation_input_tokens as f64 / 1e6
        );
        eprintln!("\ndaily activity (top 15 by cost):");
        let mut daily = snap.daily_activity.clone();
        daily.sort_by(|a, b| {
            b.cost_usd
                .partial_cmp(&a.cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for d in daily.iter().take(15) {
            eprintln!("  {}  ${:>7.2}  calls={}", d.date, d.cost_usd, d.call_count);
        }
    }

    #[test]
    #[ignore = "manual; set CLAUDE_DEDUP_FIXTURE=<path>.jsonl to run"]
    fn dedup_verify_real_file() {
        let Ok(path) = std::env::var("CLAUDE_DEDUP_FIXTURE") else {
            return;
        };
        let turns = read_turns(std::path::Path::new(&path));
        let total: usize = turns.iter().map(|t| t.calls.len()).sum();
        let uniq: std::collections::HashSet<&String> = turns
            .iter()
            .flat_map(|t| &t.calls)
            .filter_map(|c| c.message_id.as_ref())
            .collect();
        eprintln!("\nfile: {path}");
        eprintln!(
            "  turns: {} calls(pre-dedup): {} unique msg-ids: {}",
            turns.len(),
            total,
            uniq.len()
        );
        let mut agg = crate::stats::aggregate::Aggregator::new();
        agg.feed_session(&crate::stats::aggregate::SessionFeed {
            agent: "claude",
            project_dir_name: "p",
            project_display: "/p",
            session_id: "s",
            path: &path,
            title: "t",
            last_modified: 1,
            message_count: 0,
            turns: &turns,
        });
        let s = agg.snapshot("test");
        eprintln!(
            "aggregator: call_count={} cost=${:.2} input={} output={} cache_read={}",
            s.call_count,
            s.cost_usd,
            s.usage.input_tokens,
            s.usage.output_tokens,
            s.usage.cache_read_input_tokens
        );
    }

    // ---- subagent fold --------------------------------------------------

    #[test]
    fn scan_folds_subagent_into_parent_session_id() {
        // sidebar 已经把 subagent 排除在 session 列表外（list_sessions 只读
        // <project>/*.jsonl），但 stats 走的是 scan() —— 这里要保证 subagent 用
        // 父 UUID 作为 session_id，让聚合器把它们合到父 session 下，避免一个
        // 概念两个数（sidebar 198 / stats 298）。
        let p = std::path::PathBuf::from(
            "/x/.claude/projects/-Users-x-app/abc123-uuid/subagents/agent-foo.jsonl",
        );
        assert!(is_subagent_path(&p));
        let meta = scan(&p);
        assert_eq!(
            meta.id, "abc123-uuid",
            "subagent session id should be parent uuid"
        );
    }

    #[test]
    fn scan_keeps_top_level_session_id_unchanged() {
        let p = std::path::PathBuf::from("/x/.claude/projects/-Users-x-app/abc123-uuid.jsonl");
        assert!(!is_subagent_path(&p));
        let meta = scan(&p);
        assert_eq!(meta.id, "abc123-uuid");
    }
}
