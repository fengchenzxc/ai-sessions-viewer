// Gemini CLI 会话源：~/.gemini/tmp/<slug>/chats/session-<startTime>-<id8>.jsonl
//
// 项目组织：
//   ~/.gemini/projects.json             —— { projects: { "/abs/cwd": "slug" } }
//   ~/.gemini/tmp/<slug>/.project_root  —— 该 slug 对应的 cwd 绝对路径
//   ~/.gemini/tmp/<slug>/chats/         —— 该项目下所有会话 JSONL
//
// project_key（dir_name）= slug；display_path = cwd —— 与 Claude 同形。
//
// JSONL 行类型：
//   首行: {sessionId, projectHash, startTime, lastUpdated, kind:"main"}
//   $set: {"$set":{...}}                                          —— 元数据补丁，跳过
//   user: {id, timestamp, type:"user", content:[{text}] | string}
//   gemini: {id, timestamp, type:"gemini", content,
//           thoughts?:[{subject,description}], toolCalls?:[...], model?, tokens?}
//   info/warning/error: {id, timestamp, type, content}            —— 状态噪音，跳过
//
// 关键坑：Gemini 会**重复写入同一条 gemini 记录** —— 首次只携带 toolCalls，后续追加
// thoughts。同 id 多次出现时取**最后**一份（最完整）。`read` 按出现顺序累积，
// 用 (Vec, HashMap<id, idx>) 实现「保留首次位置 + 替换为最新内容」。
//
// 限制：
//   - Gemini CLI 的 `--resume` 只接受 "latest" 或索引号，**不支持 UUID**，所以
//     `resume_cli` 一律返回 `gemini --resume latest`：点击非最新会话也会回到最新。
//   - rename 只能写入我们自己的 `$rename` marker；Gemini 自带的 `--list-sessions`
//     不读这个字段，重命名只在本 viewer 内可见。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::SessionSource;
use crate::stats::types::Turn;
use crate::types::{Block, Msg, ProjectInfo, SessionMeta, SessionPage, UsageSummary};
use crate::util::{
    append_jsonl_line, clean_title, format_iso8601_utc, home, is_jsonl, mtime_millis, now_millis,
    text_block, validate_rename_name,
};

pub struct GeminiSource;

fn tmp_dir() -> PathBuf {
    home().join(".gemini").join("tmp")
}

fn chats_dir(slug: &str) -> PathBuf {
    tmp_dir().join(slug).join("chats")
}

/// 读取 ~/.gemini/tmp/<slug>/.project_root，拿到该 slug 对应的 cwd。
fn cwd_for_slug(slug: &str) -> Option<String> {
    let p = tmp_dir().join(slug).join(".project_root");
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// 列出 chats 目录里所有 session-*.jsonl 文件。
fn chat_files(slug: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(chats_dir(slug)) {
        for e in rd.flatten() {
            let p = e.path();
            if is_jsonl(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// 把 user content 折叠成纯文本（用于扫标题）。content 可能是字符串，也可能是
/// `[{text},{text}]` 数组。
fn user_text_from_content(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let mut out = String::new();
            for el in arr {
                if let Some(s) = el.get("text").and_then(|x| x.as_str()) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(s);
                }
            }
            out
        }
        _ => String::new(),
    }
}

/// Gemini image: `inlineData {mimeType, data}` → data URL；外链 `imageUrl` 直接透传。
fn image_src(el: &Value) -> Option<String> {
    if let Some(obj) = el.get("inlineData").and_then(|x| x.as_object()) {
        let mime = obj
            .get("mimeType")
            .and_then(|x| x.as_str())
            .unwrap_or("image/png");
        let data = obj.get("data").and_then(|x| x.as_str())?;
        return Some(format!("data:{mime};base64,{data}"));
    }
    if let Some(s) = el.get("imageUrl").and_then(|x| x.as_str()) {
        if !s.trim().is_empty() {
            return Some(s.to_string());
        }
    }
    None
}

/// 取首条用户输入作为回收站标题。
fn first_user_text(path: &Path) -> String {
    if let Ok(file) = fs::File::open(path) {
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if v.get("type").and_then(|x| x.as_str()) == Some("user") {
                    if let Some(c) = v.get("content") {
                        let cleaned = clean_title(&user_text_from_content(c));
                        if !cleaned.is_empty() {
                            return cleaned;
                        }
                    }
                }
            }
        }
    }
    "(无标题会话)".to_string()
}

/// 扫一个会话文件，构造 SessionMeta（廉价阶段：只算消息数、找标题、抓 sessionId）。
fn scan(fp: &Path) -> SessionMeta {
    let file_name = fp
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let size = fs::metadata(fp).map(|m| m.len()).unwrap_or(0);
    let modified = mtime_millis(fp);

    let mut id = String::new();
    let mut created: Option<String> = None;
    let mut renamed: Option<String> = None;
    let mut first_user: String = String::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut message_count = 0usize;

    if let Ok(file) = fs::File::open(fp) {
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let v: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if id.is_empty() {
                if let Some(s) = v.get("sessionId").and_then(|x| x.as_str()) {
                    id = s.to_string();
                    created = v
                        .get("startTime")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string());
                    continue;
                }
            }
            let t = match v.get("type").and_then(|x| x.as_str()) {
                Some(t) => t,
                None => continue,
            };
            if t == "$rename" {
                if let Some(n) = v.get("name").and_then(|x| x.as_str()) {
                    let trim = n.trim();
                    if !trim.is_empty() {
                        renamed = Some(trim.to_string());
                    }
                }
                continue;
            }
            if t != "user" && t != "gemini" {
                continue;
            }
            // 同 id 多次出现是 gemini 的渐进式写入，只计一次。
            let rid = v
                .get("id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if rid.is_empty() || seen.insert(rid) {
                message_count += 1;
            }
            if first_user.is_empty() && t == "user" {
                if let Some(c) = v.get("content") {
                    let cleaned = clean_title(&user_text_from_content(c));
                    if !cleaned.is_empty() {
                        first_user = cleaned;
                    }
                }
            }
        }
    }
    let id = if id.is_empty() {
        file_name.trim_end_matches(".jsonl").to_string()
    } else {
        id
    };
    let title = renamed.unwrap_or_else(|| {
        if first_user.is_empty() {
            "(无标题会话)".to_string()
        } else {
            first_user
        }
    });

    // 从文件路径反推 slug → cwd。
    let cwd = fp
        .parent() // chats/
        .and_then(|p| p.parent()) // <slug>/
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .and_then(|slug| cwd_for_slug(&slug));

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

/// 解析单个 gemini 记录的 toolCall，输出 `tool_use` + `tool_result` 一对 block。
fn tool_call_blocks(tc: &Value) -> Vec<Block> {
    let name = tc
        .get("name")
        .and_then(|x| x.as_str())
        .unwrap_or("tool")
        .to_string();
    let args = tc
        .get("args")
        .map(|a| serde_json::to_string_pretty(a).unwrap_or_default())
        .unwrap_or_default();
    let id = tc.get("id").and_then(|x| x.as_str()).map(|s| s.to_string());

    let mut blocks = vec![Block {
        kind: "tool_use".to_string(),
        tool_name: Some(name),
        tool_input: Some(args),
        tool_id: id.clone(),
        ..Default::default()
    }];

    // result 优先取 functionResponse.response.output；resultDisplay 是 TUI 的 ANSI
    // 富文本，含色码标记，肉眼可读但喂给 markdown 渲染就是噪音，回落而非首选。
    let mut text = String::new();
    if let Some(arr) = tc.get("result").and_then(|x| x.as_array()) {
        for r in arr {
            if let Some(out) = r
                .get("functionResponse")
                .and_then(|fr| fr.get("response"))
                .and_then(|resp| resp.get("output"))
            {
                let s = match out {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if !s.is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&s);
                }
            }
        }
    }
    if text.is_empty() {
        if let Some(s) = tc.get("resultDisplay").and_then(|x| x.as_str()) {
            text = s.to_string();
        }
    }
    let is_error = tc.get("status").and_then(|x| x.as_str()) == Some("error");
    blocks.push(Block {
        kind: "tool_result".to_string(),
        text: Some(text),
        tool_id: id,
        is_error,
        ..Default::default()
    });
    blocks
}

fn read(path: &str) -> Result<Vec<Msg>, String> {
    let file = fs::File::open(path).map_err(|e| format!("打开会话失败: {e}"))?;

    // 按出现顺序收集 user / gemini 记录，同 id 出现时**替换原位置**为最新版本
    // —— Gemini 会渐进式追加 thoughts。无 id 的行按到达顺序追加。
    let mut entries: Vec<Value> = Vec::new();
    let mut id_to_idx: HashMap<String, usize> = HashMap::new();

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // 跳过头行（无 type，但有 sessionId+startTime）和 $set 元数据补丁。
        if v.get("sessionId").is_some() && v.get("startTime").is_some() {
            continue;
        }
        if v.get("$set").is_some() {
            continue;
        }
        let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        // info / warning / error / $rename 全部当噪音/元数据，read 不渲染。
        if t != "user" && t != "gemini" {
            continue;
        }
        match v.get("id").and_then(|x| x.as_str()) {
            Some(id) if !id.is_empty() => {
                let key = id.to_string();
                if let Some(&idx) = id_to_idx.get(&key) {
                    entries[idx] = v;
                } else {
                    id_to_idx.insert(key, entries.len());
                    entries.push(v);
                }
            }
            _ => entries.push(v),
        }
    }

    let mut msgs = Vec::new();
    for v in entries {
        let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let ts = v
            .get("timestamp")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());

        match t {
            "user" => {
                let mut blocks: Vec<Block> = Vec::new();
                if let Some(content) = v.get("content") {
                    match content {
                        Value::String(s) if !s.trim().is_empty() => {
                            blocks.push(text_block("text", s));
                        }
                        Value::Array(arr) => {
                            let mut text = String::new();
                            for el in arr {
                                if let Some(s) = el.get("text").and_then(|x| x.as_str()) {
                                    if !text.is_empty() {
                                        text.push('\n');
                                    }
                                    text.push_str(s);
                                } else if let Some(src) = image_src(el) {
                                    blocks.push(Block {
                                        kind: "image".to_string(),
                                        image_src: Some(src),
                                        ..Default::default()
                                    });
                                }
                            }
                            if !text.trim().is_empty() {
                                blocks.push(text_block("text", &text));
                            }
                        }
                        _ => {}
                    }
                }
                if !blocks.is_empty() {
                    msgs.push(Msg {
                        uuid: None,
                        role: "user".to_string(),
                        timestamp: ts,
                        model: None,
                        sidechain: false,
                        blocks,
                    });
                }
            }
            "gemini" => {
                let mut blocks: Vec<Block> = Vec::new();
                // thoughts → thinking blocks（subject 标题 + description 正文）
                if let Some(thoughts) = v.get("thoughts").and_then(|x| x.as_array()) {
                    for th in thoughts {
                        let subject = th
                            .get("subject")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .trim();
                        let desc = th
                            .get("description")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .trim();
                        let body = match (subject.is_empty(), desc.is_empty()) {
                            (false, false) => format!("**{subject}**\n\n{desc}"),
                            (false, true) => subject.to_string(),
                            (true, false) => desc.to_string(),
                            _ => continue,
                        };
                        blocks.push(text_block("thinking", &body));
                    }
                }
                // toolCalls：每个调用一对 tool_use + tool_result block。
                if let Some(tcs) = v.get("toolCalls").and_then(|x| x.as_array()) {
                    for tc in tcs {
                        blocks.extend(tool_call_blocks(tc));
                    }
                }
                // 正文（content 是字符串）
                if let Some(s) = v.get("content").and_then(|x| x.as_str()) {
                    if !s.trim().is_empty() {
                        blocks.push(text_block("text", s));
                    }
                }
                if !blocks.is_empty() {
                    let model = v
                        .get("model")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string());
                    msgs.push(Msg {
                        uuid: None,
                        role: "assistant".to_string(),
                        timestamp: ts,
                        model,
                        sidechain: false,
                        blocks,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(msgs)
}

impl SessionSource for GeminiSource {
    fn name(&self) -> &'static str {
        "gemini"
    }

    fn list_projects(
        &self,
        _include_codex_internal: bool,
        _include_codex_archived: bool,
    ) -> Result<Vec<ProjectInfo>, String> {
        let mut out = Vec::new();
        let rd = match fs::read_dir(tmp_dir()) {
            Ok(rd) => rd,
            // 没运行过 Gemini CLI 时目录不存在，平静返回空列表。
            Err(_) => return Ok(out),
        };
        for e in rd.flatten() {
            let slug_path = e.path();
            if !slug_path.is_dir() {
                continue;
            }
            let slug = match slug_path.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => continue,
            };
            let chats = slug_path.join("chats");
            if !chats.is_dir() {
                continue;
            }
            let cwd = cwd_for_slug(&slug).unwrap_or_else(|| slug.clone());
            let mut count = 0usize;
            let mut last_mod = 0u64;
            if let Ok(rd2) = fs::read_dir(&chats) {
                for e2 in rd2.flatten() {
                    let p = e2.path();
                    if is_jsonl(&p) {
                        count += 1;
                        let mt = mtime_millis(&p);
                        if mt > last_mod {
                            last_mod = mt;
                        }
                    }
                }
            }
            if count == 0 {
                continue;
            }
            let exists = Path::new(&cwd).is_dir();
            out.push(ProjectInfo {
                dir_name: slug,
                display_path: cwd,
                session_count: count,
                last_modified: last_mod,
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
        let files = chat_files(project_key);
        let mut paired: Vec<(PathBuf, u64)> = files
            .into_iter()
            .map(|p| {
                let mt = mtime_millis(&p);
                (p, mt)
            })
            .collect();
        paired.sort_by_key(|(_, mt)| std::cmp::Reverse(*mt));
        let total = paired.len();
        let sessions = paired
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
        let now_ms = now_millis() as i64;
        let secs = now_ms / 1000;
        let ms = (now_ms % 1000) as u32;
        let ts = format_iso8601_utc(secs, ms);
        let line = serde_json::json!({
            "type": "$rename",
            "name": trimmed,
            "timestamp": ts,
        })
        .to_string();
        append_jsonl_line(path, &line)
    }

    fn trash_title(&self, path: &Path) -> String {
        first_user_text(path)
    }

    fn resume_cli(&self, session_id: &str, path: &str) -> String {
        // Gemini CLI 的 --resume 高度依赖当前目录（CWD）。
        // 我们强制回到该会话所属的 .project_root（由 App.vue 传入的 cwd 保证）。
        //
        // 恢复策略优先级：
        // 1. 如果是标准的 36 位 UUID，优先用 ID 恢复（最稳）。
        // 2. 否则，计算该文件在项目所有会话中的 1-based 索引（按 mtime 升序）。
        //    Gemini CLI 的 --list-sessions 索引就是按时间从旧到新排列的。
        if session_id.len() == 36 && session_id.contains('-') {
            return format!("gemini --resume {session_id}");
        }

        let p = Path::new(path);
        if let Some(parent) = p.parent() {
            let mut files = Vec::new();
            if let Ok(rd) = fs::read_dir(parent) {
                for e in rd.flatten() {
                    let fp = e.path();
                    if is_jsonl(&fp) {
                        let mt = mtime_millis(&fp);
                        files.push((fp, mt));
                    }
                }
            }
            // 按修改时间升序（从旧到新），匹配 gemini --list-sessions 的序号
            files.sort_by_key(|x| x.1);
            if let Some(pos) = files.iter().position(|x| x.0 == p) {
                return format!("gemini --resume {}", pos + 1);
            }
        }
        "gemini --resume latest".to_string()
    }

    fn new_session_cli(&self) -> String {
        "gemini".to_string()
    }

    fn image_src(&self, block: &Value) -> Option<String> {
        image_src(block)
    }

    /// Gemini CLI 目前在 JSONL 里没有稳定的 token 字段（`tokens?` 这一项实际上很少
    /// 写入，写也只是 prompt+completion 笼统计数），先占位返回零值。
    /// 等 Gemini CLI 把这块写规整了再切到真实解析。
    fn usage_summary(&self, _path: &str) -> Result<UsageSummary, String> {
        Ok(UsageSummary::default())
    }

    /// 单遍 JSONL → Vec<Turn>。同 id 的 gemini 行最新一份生效（read() 同款去重）。
    fn read_turns(&self, path: &str) -> Result<Vec<Turn>, String> {
        Ok(read_turns(Path::new(path)))
    }
}

// ---- read_turns（统计聚合用）---------------------------------------------
//
// Gemini JSONL 的 gemini-type 行携带：
//   - model: "gemini-3-flash-preview" / "gemini-2.5-flash" / ...
//   - tokens: {input, output, cached, thoughts, tool, total}
//   - toolCalls?: [{name, status, request, ...}]
//
// 注意：同 id 的 gemini 记录会**重复写入**（先 toolCalls，再追加 thoughts），
// 必须按 id 去重，否则同一次调用会被计两遍。`read()` 已经用 HashMap<id, idx> 做
// 这事；read_turns 复刻同样的去重策略。
//
// project_path：从 .project_root 拿，slug 推不出 cwd 时退到 slug 本身。
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

    // slug = chats/ 的父目录名；用 slug 反查 .project_root
    let project_path = fp
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .and_then(cwd_for_slug)
        .unwrap_or_default();

    // 第一遍：按 id 去重（保留首次位置、内容用最新版）
    let mut entries: Vec<Value> = Vec::new();
    let mut id_to_idx: HashMap<String, usize> = HashMap::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("sessionId").is_some() && v.get("startTime").is_some() {
            continue;
        }
        if v.get("$set").is_some() {
            continue;
        }
        let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        if t != "user" && t != "gemini" {
            continue;
        }
        match v.get("id").and_then(|x| x.as_str()) {
            Some(id) if !id.is_empty() => {
                let key = id.to_string();
                if let Some(&idx) = id_to_idx.get(&key) {
                    entries[idx] = v;
                } else {
                    id_to_idx.insert(key, entries.len());
                    entries.push(v);
                }
            }
            _ => entries.push(v),
        }
    }

    // 第二遍：line-by-line 折叠成 Turn —— 一条 user 启新 Turn，后续 gemini call 都附在它上。
    let mut turns: Vec<Turn> = Vec::new();
    let mut cur: Option<Turn> = None;

    for v in entries {
        let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let ts_ms = v
            .get("timestamp")
            .and_then(|x| x.as_str())
            .and_then(crate::util::parse_iso8601_ms)
            .unwrap_or(0);

        if t == "user" {
            if let Some(prev) = cur.take() {
                turns.push(prev);
            }
            let user_text = v
                .get("content")
                .map(user_text_from_content)
                .unwrap_or_default();
            cur = Some(Turn {
                user_message: user_text,
                project_path: project_path.clone(),
                session_id: session_id.clone(),
                calls: Vec::new(),
                timestamp_ms: ts_ms,
            });
            continue;
        }

        // gemini
        let model = v
            .get("model")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        // tokens.{input,output,cached,thoughts} 直接映射；tool 字段加进 output 末尾
        // —— UsageSummary 没有专门的 "tool tokens" 字段，少量并入 output 不会严重失真，
        // pricing 看的是 input/output 两条主线。
        let mut usage = UsageSummary::default();
        if let Some(tk) = v.get("tokens") {
            usage.input_tokens = tk.get("input").and_then(Value::as_u64).unwrap_or(0);
            usage.output_tokens = tk.get("output").and_then(Value::as_u64).unwrap_or(0);
            usage.cache_read_input_tokens = tk.get("cached").and_then(Value::as_u64).unwrap_or(0);
            usage.reasoning_output_tokens = tk.get("thoughts").and_then(Value::as_u64).unwrap_or(0);
            // tool tokens：折入 output（少量、avoid 凭空丢失）
            usage.output_tokens += tk.get("tool").and_then(Value::as_u64).unwrap_or(0);
            usage = usage.finalize();
        }

        // toolCalls：每个 {name, ...} 是一次调用名。
        // Gemini CLI 工具命名见 https://github.com/google-gemini/gemini-cli —— 主要工具有：
        //   read_file / list_directory / glob / replace / write_file / run_shell_command /
        //   save_memory / search_file_content / update_topic / web_fetch / web_search ...
        // 对 by_tool 统计，整张名字直接进。bash_commands 用 run_shell_command 抽。
        let mut tools: Vec<String> = Vec::new();
        let mut bash_commands: Vec<String> = Vec::new();
        let mut mcp_servers: Vec<String> = Vec::new();
        if let Some(tcs) = v.get("toolCalls").and_then(|x| x.as_array()) {
            for tc in tcs {
                let name = tc
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                if name == "run_shell_command" {
                    // request 里通常带 {command: "..."}；用统一的 shell_util 抽首词。
                    if let Some(req) = tc.get("request") {
                        let raw = match req {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        if let Some(cmd) = crate::stats::shell::extract_first_command(&raw) {
                            bash_commands.push(cmd);
                        }
                    }
                }
                if let Some(server) = crate::stats::shell::extract_mcp_server(&name) {
                    mcp_servers.push(server);
                }
                tools.push(name);
            }
        }

        let cost = if usage.total == 0 {
            0.0
        } else {
            crate::stats::pricing::cost_usd(&model, &usage)
        };
        let call = crate::stats::types::CallRecord {
            model,
            message_id: None,
            usage,
            cost_usd: cost,
            tools,
            bash_commands,
            mcp_servers,
            has_plan_mode: false,
            has_agent_spawn: false,
        };

        if let Some(turn) = cur.as_mut() {
            turn.calls.push(call);
        } else {
            // 没有 user 打头的 gemini 行：合成一个空 user 的 Turn 兜底。
            cur = Some(Turn {
                user_message: String::new(),
                project_path: project_path.clone(),
                session_id: session_id.clone(),
                calls: vec![call],
                timestamp_ms: ts_ms,
            });
        }
    }
    if let Some(last) = cur.take() {
        turns.push(last);
    }
    turns
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn user_text_handles_string_and_array() {
        assert_eq!(user_text_from_content(&json!("hello")), "hello");
        assert_eq!(
            user_text_from_content(&json!([{"text":"a"},{"text":"b"}])),
            "a\nb"
        );
        assert_eq!(user_text_from_content(&json!(42)), "");
    }

    #[test]
    fn image_src_extracts_inline_data() {
        let el = json!({"inlineData":{"mimeType":"image/png","data":"AAAA"}});
        assert_eq!(
            image_src(&el),
            Some("data:image/png;base64,AAAA".to_string())
        );
    }

    #[test]
    fn image_src_extracts_image_url() {
        let el = json!({"imageUrl":"https://e.com/a.png"});
        assert_eq!(image_src(&el), Some("https://e.com/a.png".to_string()));
    }

    #[test]
    fn image_src_returns_none_for_plain_text() {
        assert_eq!(image_src(&json!({"text":"hi"})), None);
    }

    #[test]
    fn tool_call_blocks_pairs_use_and_result() {
        let tc = json!({
            "id": "t1",
            "name": "run_shell_command",
            "args": {"command": "ls"},
            "status": "success",
            "result": [{"functionResponse":{"response":{"output":"file1\nfile2"}}}],
        });
        let blocks = tool_call_blocks(&tc);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, "tool_use");
        assert_eq!(blocks[0].tool_name.as_deref(), Some("run_shell_command"));
        assert_eq!(blocks[1].kind, "tool_result");
        assert_eq!(blocks[1].text.as_deref(), Some("file1\nfile2"));
        assert!(!blocks[1].is_error);
    }

    #[test]
    fn tool_call_marks_errors() {
        let tc = json!({"id":"t","name":"x","status":"error","result":[]});
        let blocks = tool_call_blocks(&tc);
        assert!(blocks[1].is_error);
    }

    #[test]
    fn read_turns_extracts_tokens_model_and_tools() {
        // 一条 user + 一条 gemini（带 tokens / model / toolCalls）—— 验证 read_turns 能
        // 把它折成 1 个 Turn / 1 个 CallRecord 并填好关键字段。
        let tmp = std::env::temp_dir().join(format!("csv-gem-turns-{}.jsonl", now_millis()));
        let lines = [
            r#"{"sessionId":"abc","startTime":"2026-05-15T09:18:37.148Z","lastUpdated":"...","kind":"main"}"#,
            r#"{"id":"u1","timestamp":"2026-05-15T09:19:00.000Z","type":"user","content":[{"text":"do it"}]}"#,
            r#"{"id":"g1","timestamp":"2026-05-15T09:19:01.000Z","type":"gemini","content":"ok","model":"gemini-2.5-flash","tokens":{"input":1000,"output":50,"cached":200,"thoughts":30,"tool":5,"total":1285},"toolCalls":[{"name":"read_file"},{"name":"run_shell_command","request":{"command":"git status"}}]}"#,
        ];
        std::fs::write(&tmp, lines.join("\n")).unwrap();
        let turns = read_turns(&tmp);
        std::fs::remove_file(&tmp).ok();

        assert_eq!(turns.len(), 1);
        let turn = &turns[0];
        assert_eq!(turn.user_message, "do it");
        assert_eq!(turn.calls.len(), 1);
        let call = &turn.calls[0];
        assert_eq!(call.model, "gemini-2.5-flash");
        assert_eq!(call.usage.input_tokens, 1000);
        // output 应该是 50 + tool(5) = 55
        assert_eq!(call.usage.output_tokens, 55);
        assert_eq!(call.usage.cache_read_input_tokens, 200);
        assert_eq!(call.usage.reasoning_output_tokens, 30);
        assert!(call.usage.total > 0);
        assert!(
            call.cost_usd > 0.0,
            "expected non-zero cost for gemini-2.5-flash"
        );
        // 工具
        assert!(call.tools.iter().any(|t| t == "read_file"));
        assert!(call.tools.iter().any(|t| t == "run_shell_command"));
        assert_eq!(call.bash_commands, vec!["git".to_string()]);
    }

    #[test]
    fn read_dedupes_progressive_gemini_writes() {
        // 写两条同 id 的 gemini —— 后者更完整。read 只保留最后版本。
        let tmp = std::env::temp_dir().join(format!("csv-gem-{}.jsonl", now_millis()));
        let lines = [
            r#"{"sessionId":"abc","startTime":"2026-05-15T09:18:37.148Z","lastUpdated":"2026-05-15T09:18:37.148Z","kind":"main"}"#,
            r#"{"id":"u1","timestamp":"2026-05-15T09:19:00.000Z","type":"user","content":[{"text":"hi"}]}"#,
            r#"{"id":"g1","timestamp":"2026-05-15T09:19:01.000Z","type":"gemini","content":"","toolCalls":[]}"#,
            r#"{"id":"g1","timestamp":"2026-05-15T09:19:02.000Z","type":"gemini","content":"hello!","thoughts":[{"subject":"Plan","description":"Think it through."}]}"#,
        ];
        std::fs::write(&tmp, lines.join("\n")).unwrap();
        let msgs = read(tmp.to_str().unwrap()).unwrap();
        std::fs::remove_file(&tmp).ok();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].blocks[0].text.as_deref(), Some("hi"));
        assert_eq!(msgs[1].role, "assistant");
        // 最新版应同时含 thinking 和 text，不再含旧 toolCalls。
        let kinds: Vec<&str> = msgs[1].blocks.iter().map(|b| b.kind.as_str()).collect();
        assert_eq!(kinds, vec!["thinking", "text"]);
        assert_eq!(msgs[1].blocks[1].text.as_deref(), Some("hello!"));
    }
}
