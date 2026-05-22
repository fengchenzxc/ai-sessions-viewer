// Codex 会话源：~/.codex/sessions/<YYYY>/<MM>/<DD>/rollout-*.jsonl
//
// Codex 的 JSONL 比 Claude 更"事件流"一些 —— 每行要么是 `event_msg`（高层对话事件，
// 文本干净）要么是 `response_item`（OpenAI ChatCompletion 原始 item，包含工具调用 /
// 多模态 content 数组）。我们用 event_msg 拿对话文本，用 response_item 抢救图片
// 和工具调用细节。

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::SessionSource;
use crate::types::{Block, Msg, ProjectInfo, SessionMeta, SessionPage};
use crate::util::{
    append_jsonl_line, clean_title, format_iso8601_utc, home, is_jsonl, mtime_millis, simple_msg,
    text_block, validate_rename_name,
};

pub struct CodexSource;

fn sessions_dir() -> PathBuf {
    home().join(".codex").join("sessions")
}

/// 在 ~/.codex 下找编号最大的 state_<N>.sqlite —— codex 用版本号区分 schema，
/// 升级时会写到新文件（state_4.sqlite → state_5.sqlite），picker 用最新的那个。
/// 没找到时返回 None，调用方应静默跳过 sqlite 更新（codex 旧版本或从未运行）。
fn find_state_db() -> Option<PathBuf> {
    let dir = home().join(".codex");
    let mut best: Option<(u64, PathBuf)> = None;
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let n = name
                .strip_prefix("state_")
                .and_then(|s| s.strip_suffix(".sqlite"))
                .and_then(|s| s.parse::<u64>().ok());
            if let Some(n) = n {
                if best.as_ref().map(|(b, _)| n > *b).unwrap_or(true) {
                    best = Some((n, e.path()));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

struct Meta {
    id: String,
    cwd: String,
    created: Option<String>,
}

/// 递归收集 ~/.codex/sessions 下所有 rollout-*.jsonl。
fn all_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_jsonl(&sessions_dir(), &mut out);
    out
}

fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_jsonl(&p, out);
            } else if is_jsonl(&p) {
                out.push(p);
            }
        }
    }
}

/// 读取首行 session_meta，得到 id / cwd / 创建时间。
fn meta(path: &Path) -> Option<Meta> {
    let file = fs::File::open(path).ok()?;
    let mut first = String::new();
    BufReader::new(file).read_line(&mut first).ok()?;
    let v: Value = serde_json::from_str(first.trim()).ok()?;
    if v.get("type").and_then(|x| x.as_str()) != Some("session_meta") {
        return None;
    }
    let p = v.get("payload")?;
    Some(Meta {
        id: p
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        cwd: p
            .get("cwd")
            .and_then(|x| x.as_str())
            .unwrap_or("(未知目录)")
            .to_string(),
        created: p
            .get("timestamp")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
    })
}

/// 读取 `~/.codex/session_index.jsonl`，返回 thread_id → 最新 thread_name。
/// 文件不存在 / 不可读时返回空 map，调用方自动回落到旧的 JSONL 内联策略。
fn load_title_index() -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    let path = home().join(".codex").join("session_index.jsonl");
    let file = match fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return map,
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = match v.get("id").and_then(|x| x.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        if let Some(name) = v.get("thread_name").and_then(|x| x.as_str()) {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                // append-only：后写入的覆盖先写入的
                map.insert(id, trimmed.to_string());
            }
        }
    }
    map
}

/// 取首条用户输入作为标题（用于回收站展示）。
fn first_user_text(fp: &Path) -> String {
    if let Ok(file) = fs::File::open(fp) {
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if v.get("type").and_then(|x| x.as_str()) == Some("event_msg") {
                    let p = v.get("payload");
                    let pt = p
                        .and_then(|p| p.get("type"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    if pt == "user_message" {
                        if let Some(m) =
                            p.and_then(|p| p.get("message")).and_then(|x| x.as_str())
                        {
                            let c = clean_title(m);
                            if !c.is_empty() {
                                return c;
                            }
                        }
                    }
                }
            }
        }
    }
    "(无标题会话)".to_string()
}

/// Codex: `{"type":"input_image","image_url":"data:...|http..."}`
/// 兼容 `image_url` 为对象 `{"url":"..."}` 的旧/上游 OpenAI 格式。
fn image_src(el: &Value) -> Option<String> {
    if el.get("type").and_then(|x| x.as_str()) != Some("input_image") {
        return None;
    }
    let v = el.get("image_url")?;
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Object(_) => v
            .get("url")
            .and_then(|x| x.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string()),
        _ => None,
    }
}

fn format_args(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => match serde_json::from_str::<Value>(s) {
            Ok(parsed) => serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| s.clone()),
            Err(_) => s.clone(),
        },
        Some(other) => serde_json::to_string_pretty(other).unwrap_or_default(),
        None => String::new(),
    }
}

fn output_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn scan(fp: &Path, m: &Meta, title_index: &HashMap<String, String>) -> SessionMeta {
    let file_name = fp
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let size = fs::metadata(fp).map(|m| m.len()).unwrap_or(0);
    let modified = mtime_millis(fp);

    // Codex rename 会追加 `event_msg.payload.type == "thread_name_updated"`，
    // 最后一条 `thread_name` 生效。优先用它，没有则回落首条 user_message。
    let mut first_user_title = String::new();
    let mut thread_name: Option<String> = None;
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
            if v.get("type").and_then(|x| x.as_str()) != Some("event_msg") {
                continue;
            }
            let p = match v.get("payload") {
                Some(p) => p,
                None => continue,
            };
            let pt = p.get("type").and_then(|x| x.as_str()).unwrap_or("");
            if pt == "thread_name_updated" {
                if let Some(name) = p.get("thread_name").and_then(|x| x.as_str()) {
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        thread_name = Some(trimmed.to_string());
                    }
                }
                continue;
            }
            if pt == "user_message" || pt == "agent_message" {
                message_count += 1;
            }
            if first_user_title.is_empty() && pt == "user_message" {
                if let Some(msg) = p.get("message").and_then(|x| x.as_str()) {
                    let clean = clean_title(msg);
                    if !clean.is_empty() {
                        first_user_title = clean;
                    }
                }
            }
        }
    }
    let id = if m.id.is_empty() {
        file_name.trim_end_matches(".jsonl").to_string()
    } else {
        m.id.clone()
    };
    // 标题优先级：session_index.jsonl（codex 自带 rename 的权威来源） >
    // rollout 内 thread_name_updated（旧版 app 的写入或 codex 在会话运行时的事件）
    // > 首条 user_message。
    let title = title_index
        .get(&id)
        .cloned()
        .or(thread_name)
        .unwrap_or_else(|| {
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
        cwd: Some(m.cwd.clone()),
        created: m.created.clone(),
        modified,
        size,
        message_count,
    }
}

/// 解析 Codex rollout：用 event_msg 取干净的对话文本，用 response_item 取工具调用 / 图片。
///
/// 图片处理：Codex 把贴图的 user message 同时写两条：
///   1. `response_item.message` (role=user)，content 数组里夹着 `input_image`
///      （真正的 base64 / URL 在这里）；
///   2. 紧接着一条 `event_msg.user_message`，message 字段是去掉图片占位
///      （`<image name=[Image #N]>...</image>`）之后的纯文本（用户键入的部分）。
///
/// 用 event_msg 那条作为最终用户气泡的文本来源，扫到对应 response_item 时
/// 先把里面的 `input_image` 块缓存起来，等到下一条 user_message 出现时一起渲染。
fn read(path: &str) -> Result<Vec<Msg>, String> {
    let file = fs::File::open(path).map_err(|e| format!("打开会话失败: {e}"))?;
    let mut msgs = Vec::new();
    let mut pending_user_images: Vec<Block> = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let ts = v
            .get("timestamp")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let p = match v.get("payload") {
            Some(p) => p,
            None => continue,
        };
        let pt = p.get("type").and_then(|x| x.as_str()).unwrap_or("");

        match (t, pt) {
            ("response_item", "message")
                if p.get("role").and_then(|x| x.as_str()) == Some("user") =>
            {
                // 不渲染整条 response_item.message —— 它还包含 <environment_context>
                // 等内部包裹，由 event_msg.user_message 负责干净文本。这里只抢救图片。
                if let Some(arr) = p.get("content").and_then(|x| x.as_array()) {
                    for el in arr {
                        if let Some(src) = image_src(el) {
                            pending_user_images.push(Block {
                                kind: "image".to_string(),
                                image_src: Some(src),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
            ("event_msg", "user_message") => {
                let text = p.get("message").and_then(|x| x.as_str()).unwrap_or("");
                let mut blocks: Vec<Block> = std::mem::take(&mut pending_user_images);
                if !text.trim().is_empty() {
                    blocks.push(text_block("text", text));
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
            ("event_msg", "agent_message") => {
                if let Some(m) = p.get("message").and_then(|x| x.as_str()) {
                    if !m.trim().is_empty() {
                        msgs.push(simple_msg("assistant", ts, text_block("text", m)));
                    }
                }
            }
            ("response_item", "function_call") | ("response_item", "custom_tool_call") => {
                let name = p
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let input = format_args(p.get("arguments").or_else(|| p.get("input")));
                let id = p
                    .get("call_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                msgs.push(simple_msg(
                    "assistant",
                    ts,
                    Block {
                        kind: "tool_use".to_string(),
                        tool_name: Some(name),
                        tool_input: Some(input),
                        tool_id: id,
                        ..Default::default()
                    },
                ));
            }
            ("response_item", "function_call_output")
            | ("response_item", "custom_tool_call_output") => {
                let out = output_text(p.get("output"));
                let id = p
                    .get("call_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                msgs.push(simple_msg(
                    "user",
                    ts,
                    Block {
                        kind: "tool_result".to_string(),
                        text: Some(out),
                        tool_id: id,
                        ..Default::default()
                    },
                ));
            }
            ("response_item", "web_search_call") => {
                let query = p
                    .get("action")
                    .and_then(|a| a.get("query"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                msgs.push(simple_msg(
                    "assistant",
                    ts,
                    Block {
                        kind: "tool_use".to_string(),
                        tool_name: Some("web_search".to_string()),
                        tool_input: Some(query),
                        ..Default::default()
                    },
                ));
            }
            _ => {}
        }
    }
    // 兜底：若文件结尾仍有未消费的图片（异常截断），别把它们丢掉。
    if !pending_user_images.is_empty() {
        msgs.push(Msg {
            uuid: None,
            role: "user".to_string(),
            timestamp: None,
            model: None,
            sidechain: false,
            blocks: std::mem::take(&mut pending_user_images),
        });
    }
    Ok(msgs)
}

impl SessionSource for CodexSource {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn list_projects(&self) -> Result<Vec<ProjectInfo>, String> {
        let mut map: HashMap<String, (usize, u64)> = HashMap::new();
        for fp in all_files() {
            if let Some(m) = meta(&fp) {
                let mt = mtime_millis(&fp);
                let entry = map.entry(m.cwd).or_insert((0, 0));
                entry.0 += 1;
                if mt > entry.1 {
                    entry.1 = mt;
                }
            }
        }
        let mut out: Vec<ProjectInfo> = map
            .into_iter()
            .map(|(cwd, (count, last))| {
                let exists = Path::new(&cwd).is_dir();
                ProjectInfo {
                    dir_name: cwd.clone(),
                    display_path: cwd,
                    session_count: count,
                    last_modified: last,
                    exists,
                }
            })
            .collect();
        out.sort_by_key(|p| std::cmp::Reverse(p.last_modified));
        Ok(out)
    }

    fn list_sessions(
        &self,
        project_key: &str,
        offset: usize,
        limit: usize,
    ) -> Result<SessionPage, String> {
        // 廉价阶段：只读每个文件首行 session_meta，筛出本项目的文件并取修改时间。
        let mut matched: Vec<(PathBuf, Meta, u64)> = Vec::new();
        for fp in all_files() {
            if let Some(m) = meta(&fp) {
                if m.cwd == project_key {
                    let mt = mtime_millis(&fp);
                    matched.push((fp, m, mt));
                }
            }
        }
        matched.sort_by_key(|m| std::cmp::Reverse(m.2));
        let total = matched.len();
        // Codex 把会话标题缓存在 ~/.codex/session_index.jsonl（append-only，同 id
        // 多条时最新一条胜出）。列表整页加载一次即可，避免每个会话都重读一次文件。
        let title_index = load_title_index();
        let sessions = matched
            .iter()
            .skip(offset)
            .take(limit)
            .map(|(p, m, _)| scan(p, m, &title_index))
            .collect();
        Ok(SessionPage { total, sessions })
    }

    fn read_session(&self, path: &str) -> Result<Vec<Msg>, String> {
        read(path)
    }

    fn rename_session(&self, path: &Path, name: &str) -> Result<(), String> {
        let trimmed = validate_rename_name(name)?;
        let filename_id = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.trim_end_matches(".jsonl").to_string())
            .unwrap_or_default();

        // Codex 文件名形如 rollout-<ts>-<uuid>.jsonl，真正的 thread_id 在
        // 首行 session_meta.payload.id 里。
        let mut codex_id: Option<String> = None;
        if let Ok(file) = fs::File::open(path) {
            for line in BufReader::new(file).lines().map_while(Result::ok).take(8) {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if v.get("type").and_then(|x| x.as_str()) == Some("session_meta") {
                        if let Some(idv) = v
                            .get("payload")
                            .and_then(|p| p.get("id"))
                            .and_then(|x| x.as_str())
                        {
                            codex_id = Some(idv.to_string());
                            break;
                        }
                    }
                }
            }
        }
        let codex_id = codex_id.unwrap_or(filename_id);

        // 1) 在 rollout JSONL 末尾追加 thread_name_updated 事件（跟 codex-tui 自己写的一致）。
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let secs = (now_ms / 1000) as i64;
        let ms = (now_ms % 1000) as u32;
        let ts = format_iso8601_utc(secs, ms);
        let line = serde_json::json!({
            "timestamp": ts,
            "type": "event_msg",
            "payload": {
                "type": "thread_name_updated",
                "thread_id": codex_id,
                "thread_name": trimmed,
            },
        })
        .to_string();
        append_jsonl_line(path, &line)?;

        // 2) 更新 ~/.codex/session_index.jsonl —— codex picker 读这个文件。
        // 实测：同 id 多条时 codex picker 取**首次出现**的那条（不是按 updated_at
        // 排序）。所以不能单纯 append，必须先把同 id 的旧条目过滤掉，再把新条目
        // 写到末尾——这样新 rename 一定能被读到，又跟 codex 自己的格式兼容。
        let updated_at = format_iso8601_utc(secs, ms).replace('Z', "000Z");
        let new_entry = serde_json::json!({
            "id": codex_id,
            "thread_name": trimmed,
            "updated_at": updated_at,
        })
        .to_string();

        let idx_path = home().join(".codex").join("session_index.jsonl");
        let mut retained: Vec<String> = Vec::new();
        if idx_path.exists() {
            if let Ok(file) = fs::File::open(&idx_path) {
                for line in BufReader::new(file).lines().map_while(Result::ok) {
                    let raw = line.trim_end_matches(['\r', '\n']);
                    if raw.is_empty() {
                        continue;
                    }
                    let same_id = serde_json::from_str::<Value>(raw)
                        .ok()
                        .and_then(|v| v.get("id").and_then(|x| x.as_str()).map(str::to_owned))
                        .map(|id| id == codex_id)
                        .unwrap_or(false);
                    if !same_id {
                        retained.push(raw.to_string());
                    }
                }
            }
        }
        retained.push(new_entry);

        // 原子替换：先写到同目录下的临时文件，再 rename 覆盖
        let parent = idx_path
            .parent()
            .ok_or_else(|| "session_index 父目录不存在".to_string())?;
        fs::create_dir_all(parent).map_err(|e| format!("创建 .codex 目录失败: {e}"))?;
        let tmp_path = parent.join(format!(".session_index.{}.tmp", now_ms));
        {
            let mut tmp = fs::File::create(&tmp_path)
                .map_err(|e| format!("打开 session_index 临时文件失败: {e}"))?;
            for line in &retained {
                tmp.write_all(line.as_bytes())
                    .map_err(|e| format!("写入 session_index 行失败: {e}"))?;
                tmp.write_all(b"\n")
                    .map_err(|e| format!("写入 session_index 换行失败: {e}"))?;
            }
            tmp.flush().map_err(|e| format!("flush 失败: {e}"))?;
        }
        fs::rename(&tmp_path, &idx_path)
            .map_err(|e| format!("替换 session_index 失败: {e}"))?;

        // 3) 真正权威：~/.codex/state_<N>.sqlite 的 threads.title 列。
        // 如果只改 session_index.jsonl 不改 sqlite，picker 仍会显示旧 title。
        // 文件不存在则跳过（codex 旧版本 / 用户从未启动过 codex CLI）。
        if let Some(db_path) = find_state_db() {
            let now_secs = (now_ms / 1000) as i64;
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| format!("打开 codex sqlite 失败: {e}"))?;
            conn.execute(
                "UPDATE threads SET title = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![trimmed, now_secs, &codex_id],
            )
            .map_err(|e| format!("更新 threads.title 失败: {e}"))?;
        }
        Ok(())
    }

    fn trash_title(&self, path: &Path) -> String {
        first_user_text(path)
    }

    fn resume_cli(&self, session_id: &str) -> String {
        format!("codex resume {session_id}")
    }

    fn new_session_cli(&self) -> String {
        "codex".to_string()
    }

    fn image_src(&self, block: &Value) -> Option<String> {
        image_src(block)
    }
}
