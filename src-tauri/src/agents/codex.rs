// Codex 会话源：~/.codex/sessions/<YYYY>/<MM>/<DD>/rollout-*.jsonl
//
// Codex 的 JSONL 比 Claude 更"事件流"一些 —— 每行要么是 `event_msg`（高层对话事件，
// 文本干净）要么是 `response_item`（OpenAI ChatCompletion 原始 item，包含工具调用 /
// 多模态 content 数组）。我们用 event_msg 拿对话文本，用 response_item 抢救图片
// 和工具调用细节。

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use serde_json::{json, Map};

use super::SessionSource;
use crate::stats::{
    pricing, shell as shell_util,
    types::{CallRecord, Turn},
};
use crate::types::{Block, Msg, ProjectInfo, SessionMeta, SessionPage, UsageSummary};
use crate::util::{
    append_jsonl_line, clean_title, format_iso8601_utc, home, is_jsonl, mtime_millis,
    parse_iso8601_ms, simple_msg, text_block, validate_rename_name,
};

pub struct CodexSource;

const CODEX_APP_FIRST_PAGE_SIZE: usize = 50;
const CODEX_APP_LIST_PAGE_SIZE: usize = 100;
const CODEX_APP_LIST_MAX_THREADS: usize = 1_000;

fn sessions_dir() -> PathBuf {
    home().join(".codex").join("sessions")
}

fn archived_sessions_dir() -> PathBuf {
    home().join(".codex").join("archived_sessions")
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

#[derive(Debug, Clone)]
struct CodexAppThreadInfo {
    rank: usize,
}

#[derive(Debug)]
struct CodexAppListSnapshot {
    available: bool,
    scanned: usize,
    first_page_size: usize,
    threads: HashMap<String, CodexAppThreadInfo>,
}

#[derive(Debug, Default, Clone, Copy)]
struct CodexThreadFlags {
    internal: bool,
    archived: bool,
}

#[derive(Debug, Default)]
struct CodexThreadFlagsIndex {
    by_id: HashMap<String, CodexThreadFlags>,
    by_path: HashMap<String, CodexThreadFlags>,
}

fn is_archived_path(path: &Path) -> bool {
    path.starts_with(archived_sessions_dir())
}

fn load_thread_flags_index() -> CodexThreadFlagsIndex {
    let Some(db_path) = find_state_db() else {
        return CodexThreadFlagsIndex::default();
    };
    let conn = match rusqlite::Connection::open(db_path) {
        Ok(conn) => conn,
        Err(_) => return CodexThreadFlagsIndex::default(),
    };
    let mut stmt = match conn.prepare(
        "SELECT id, rollout_path, archived, has_user_event, source, thread_source, model FROM threads",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return CodexThreadFlagsIndex::default(),
    };
    let rows = match stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let rollout_path: String = row.get(1)?;
        let archived: i64 = row.get(2)?;
        let has_user_event: i64 = row.get(3)?;
        let source: String = row.get(4).unwrap_or_default();
        let thread_source: Option<String> = row.get(5).unwrap_or_default();
        let model: Option<String> = row.get(6).unwrap_or_default();
        let flags = thread_flags_from_fields(
            archived != 0,
            has_user_event,
            &source,
            thread_source.as_deref(),
            model.as_deref(),
        );
        Ok((id, rollout_path, flags))
    }) {
        Ok(rows) => rows,
        Err(_) => return CodexThreadFlagsIndex::default(),
    };
    let mut index = CodexThreadFlagsIndex::default();
    for row in rows.flatten() {
        let (id, rollout_path, flags) = row;
        index.by_path.insert(rollout_path, flags);
        index.by_id.insert(id, flags);
    }
    index
}

fn thread_flags_from_fields(
    archived: bool,
    _has_user_event: i64,
    source: &str,
    thread_source: Option<&str>,
    model: Option<&str>,
) -> CodexThreadFlags {
    let source_lc = source.to_lowercase();
    let thread_source_lc = thread_source.unwrap_or_default().to_lowercase();
    let model_lc = model.unwrap_or_default().to_lowercase();
    let internal = thread_source_lc == "subagent"
        || source_lc.contains("guardian")
        || model_lc == "codex-auto-review";
    CodexThreadFlags { internal, archived }
}

fn flags_for(fp: &Path, meta: &Meta, index: &CodexThreadFlagsIndex) -> CodexThreadFlags {
    let path = fp.to_string_lossy().to_string();
    let mut flags = index
        .by_id
        .get(&meta.id)
        .or_else(|| index.by_path.get(&path))
        .copied()
        .unwrap_or_default();
    if is_archived_path(fp) {
        flags.archived = true;
    }
    flags
}

fn include_by_flags(
    flags: CodexThreadFlags,
    include_internal: bool,
    include_archived: bool,
) -> bool {
    if flags.archived {
        return include_archived;
    }
    if flags.internal {
        return include_internal;
    }
    true
}

impl Default for CodexAppListSnapshot {
    fn default() -> Self {
        Self {
            available: false,
            scanned: 0,
            first_page_size: CODEX_APP_FIRST_PAGE_SIZE,
            threads: HashMap::new(),
        }
    }
}

/// 递归收集 Codex rollout JSONL。默认只扫 ~/.codex/sessions；
/// 用户显式打开“已归档会话”时再额外扫 ~/.codex/archived_sessions。
fn all_files(include_archived: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_jsonl(&sessions_dir(), &mut out);
    if include_archived {
        collect_jsonl(&archived_sessions_dir(), &mut out);
    }
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

fn augmented_path() -> String {
    let current = env::var("PATH").unwrap_or_default();
    let additions = [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ];
    let mut parts: Vec<String> = additions.iter().map(|value| value.to_string()).collect();
    parts.extend(
        current
            .split(':')
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
    );
    parts.dedup();
    parts.join(":")
}

fn codex_cli_path() -> PathBuf {
    if let Ok(path) = env::var("CODEX_CLI") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return candidate;
        }
    }
    for candidate in [
        "/opt/homebrew/bin/codex",
        "/usr/local/bin/codex",
        "/usr/bin/codex",
    ] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return path;
        }
    }
    PathBuf::from("codex")
}

fn app_server_response(
    rx: &mpsc::Receiver<String>,
    id: i64,
    timeout: Duration,
) -> Result<Value, String> {
    loop {
        let line = rx
            .recv_timeout(timeout)
            .map_err(|_| format!("等待 app-server 响应超时: {id}"))?;
        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("id").and_then(Value::as_i64) != Some(id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            return Err(format!("app-server 错误: {error}"));
        }
        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
    }
}

fn query_codex_app_thread_list() -> CodexAppListSnapshot {
    let result = (|| -> Result<CodexAppListSnapshot, String> {
        let mut child = Command::new(codex_cli_path())
            .args(["app-server", "--stdio"])
            .env("PATH", augmented_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("启动 codex app-server 失败: {e}"))?;

        let scan = (|| -> Result<CodexAppListSnapshot, String> {
            if let Some(stderr) = child.stderr.take() {
                thread::spawn(move || {
                    let mut reader = BufReader::new(stderr);
                    let mut sink = String::new();
                    let _ = reader.read_to_string(&mut sink);
                });
            }

            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| "app-server stdout 不可用".to_string())?;
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| "app-server stdin 不可用".to_string())?;

            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    let _ = tx.send(line);
                }
            });

            writeln!(
                stdin,
                "{}",
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "clientInfo": {
                            "name": "cc-sessions-viewer",
                            "version": env!("CARGO_PKG_VERSION"),
                        },
                        "capabilities": { "experimentalApi": true },
                    },
                })
            )
            .map_err(|e| format!("写入 initialize 失败: {e}"))?;
            stdin
                .flush()
                .map_err(|e| format!("flush initialize 失败: {e}"))?;
            let _ = app_server_response(&rx, 1, Duration::from_secs(5))?;

            let mut threads = HashMap::new();
            let mut cursor: Option<String> = None;
            let mut rank = 0usize;
            let mut request_id = 2i64;

            loop {
                let limit = if cursor.is_none() {
                    CODEX_APP_FIRST_PAGE_SIZE
                } else {
                    CODEX_APP_LIST_PAGE_SIZE
                };
                let mut params = Map::new();
                params.insert("limit".into(), json!(limit));
                params.insert("archived".into(), json!(false));
                params.insert("sortKey".into(), json!("updated_at"));
                params.insert("sortDirection".into(), json!("desc"));
                if let Some(cursor_value) = cursor.clone() {
                    params.insert("cursor".into(), json!(cursor_value));
                }

                writeln!(
                    stdin,
                    "{}",
                    json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "method": "thread/list",
                        "params": Value::Object(params),
                    })
                )
                .map_err(|e| format!("写入 thread/list 失败: {e}"))?;
                stdin
                    .flush()
                    .map_err(|e| format!("flush thread/list 失败: {e}"))?;
                let response = app_server_response(&rx, request_id, Duration::from_secs(8))?;
                request_id += 1;

                if let Some(data) = response.get("data").and_then(Value::as_array) {
                    for item in data {
                        if let Some(id) = item.get("id").and_then(Value::as_str) {
                            rank += 1;
                            threads
                                .entry(id.to_string())
                                .or_insert(CodexAppThreadInfo { rank });
                        }
                    }
                }

                cursor = response
                    .get("nextCursor")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if cursor.is_none() || rank >= CODEX_APP_LIST_MAX_THREADS {
                    break;
                }
            }

            Ok(CodexAppListSnapshot {
                available: true,
                scanned: rank,
                first_page_size: CODEX_APP_FIRST_PAGE_SIZE,
                threads,
            })
        })();
        let _ = child.kill();
        let _ = child.wait();
        scan
    })();

    result.unwrap_or_default()
}

fn apply_codex_app_list_snapshot(sessions: &mut [SessionMeta], snapshot: &CodexAppListSnapshot) {
    for session in sessions {
        session.codex_app_first_page_size = snapshot.first_page_size;
        if !snapshot.available {
            session.codex_app_list_rank = None;
            session.codex_app_list_scanned = 0;
            session.codex_app_first_page_position = 0;
            continue;
        }
        let info = snapshot.threads.get(&session.id);
        session.codex_app_list_scanned = snapshot.scanned;
        session.codex_app_list_rank = info.map(|item| item.rank);
        session.codex_app_first_page_position = info
            .filter(|item| item.rank <= snapshot.first_page_size)
            .map(|item| item.rank)
            .unwrap_or(0);
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
                        if let Some(m) = p.and_then(|p| p.get("message")).and_then(|x| x.as_str()) {
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

fn scan(
    fp: &Path,
    m: &Meta,
    title_index: &HashMap<String, String>,
    flags: CodexThreadFlags,
) -> SessionMeta {
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
        codex_app_list_rank: None,
        codex_app_list_scanned: 0,
        codex_app_first_page_size: 50,
        codex_app_first_page_position: 0,
        codex_internal: flags.internal,
        codex_archived: flags.archived,
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

    fn list_projects(
        &self,
        include_codex_internal: bool,
        include_codex_archived: bool,
    ) -> Result<Vec<ProjectInfo>, String> {
        let mut map: HashMap<String, (usize, u64)> = HashMap::new();
        let flags_index = load_thread_flags_index();
        for fp in all_files(include_codex_archived) {
            if let Some(m) = meta(&fp) {
                let flags = flags_for(&fp, &m, &flags_index);
                if !include_by_flags(flags, include_codex_internal, include_codex_archived) {
                    continue;
                }
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
        include_codex_internal: bool,
        include_codex_archived: bool,
    ) -> Result<SessionPage, String> {
        // 廉价阶段：只读每个文件首行 session_meta，筛出本项目的文件并取修改时间。
        let mut matched: Vec<(PathBuf, Meta, u64, CodexThreadFlags)> = Vec::new();
        let flags_index = load_thread_flags_index();
        for fp in all_files(include_codex_archived) {
            if let Some(m) = meta(&fp) {
                if m.cwd == project_key {
                    let flags = flags_for(&fp, &m, &flags_index);
                    if !include_by_flags(flags, include_codex_internal, include_codex_archived) {
                        continue;
                    }
                    let mt = mtime_millis(&fp);
                    matched.push((fp, m, mt, flags));
                }
            }
        }
        matched.sort_by_key(|m| std::cmp::Reverse(m.2));
        let total = matched.len();
        // Codex 把会话标题缓存在 ~/.codex/session_index.jsonl（append-only，同 id
        // 多条时最新一条胜出）。列表整页加载一次即可，避免每个会话都重读一次文件。
        let title_index = load_title_index();
        let mut sessions: Vec<SessionMeta> = matched
            .iter()
            .skip(offset)
            .take(limit)
            .map(|(p, m, _, flags)| scan(p, m, &title_index, *flags))
            .collect();
        if limit != usize::MAX {
            let snapshot = query_codex_app_thread_list();
            apply_codex_app_list_snapshot(&mut sessions, &snapshot);
        }
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
        fs::rename(&tmp_path, &idx_path).map_err(|e| format!("替换 session_index 失败: {e}"))?;

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

    fn resume_cli(&self, session_id: &str, _path: &str) -> String {
        format!("codex resume {session_id}")
    }

    fn new_session_cli(&self) -> String {
        "codex".to_string()
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

// ---- read_turns（统计聚合用）---------------------------------------------
//
// Codex 的 JSONL 单遍：
//   - 起 turn：`event_msg.user_message` —— message 是干净文本
//   - 起 call：assistant 这边没有"单一消息"概念，每个 `response_item.function_call`
//     / `custom_tool_call` / `web_search_call` 都算一次工具调用；`event_msg.agent_message`
//     的纯文本回复也单独算一次 call（model 取最近 turn_context.payload.model）。
//   - model：来源是 `turn_context` 事件的 `payload.model`（mid-session 可能切换，譬如
//     gpt-5.5 / gpt-5.3-codex，每次出现就更新 `model_hint`）。
//     旧 `session_meta` 里**没有** model 字段（`originator` 是 "codex-tui" 这种字符串），
//     所以历史代码全部走 fallback 拿到空串，导致 pricing 算出 $0 —— 这里必须读 turn_context。
//   - usage：codex 把整段对话的 token 累积总数写在 `event_msg.token_count.info.total_token_usage`，
//     每次更新都是累积值。读到最后一行后整段 usage 归到该 session 最后一个 call
//     （所以以 session 结束时刻的 model_hint 计价 —— 这是单模型简化）。
//
// 这样 By Model / By Tool / Shell / MCP / Activity 都能拿到合理数据；
// 单 session 内只显示一个模型（没法分摊到多个）。
fn read_turns(fp: &Path) -> Vec<Turn> {
    let file = match fs::File::open(fp) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let mut turns: Vec<Turn> = Vec::new();
    let mut cur: Option<Turn> = None;
    let mut project_path: String = String::new();
    let mut session_id: String = String::new();
    let mut model_hint: String = String::new();
    let mut last_usage = UsageSummary::default();

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let payload = match v.get("payload") {
            Some(p) => p,
            None => continue,
        };
        let pt = payload.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let ts_ms = v
            .get("timestamp")
            .and_then(|x| x.as_str())
            .and_then(parse_iso8601_ms)
            .unwrap_or(0);

        match (t, pt) {
            ("session_meta", _) => {
                if session_id.is_empty() {
                    if let Some(id) = payload.get("id").and_then(|x| x.as_str()) {
                        session_id = id.to_string();
                    }
                }
                if project_path.is_empty() {
                    if let Some(c) = payload.get("cwd").and_then(|x| x.as_str()) {
                        project_path = c.to_string();
                    }
                }
                // 兜底：极少数老格式直接在 session_meta 里写 model 字段。
                // 现在的 codex 走 turn_context.model，下面有专门分支处理。
                if model_hint.is_empty() {
                    if let Some(m) = payload.get("model").and_then(|x| x.as_str()) {
                        model_hint = m.to_string();
                    }
                }
            }
            ("turn_context", _) => {
                // 每个 turn 之前 codex 会写一个 turn_context，model 就在 payload.model。
                // mid-session 切模型时也以最后一条为准。
                if let Some(m) = payload.get("model").and_then(|x| x.as_str()) {
                    if !m.is_empty() {
                        model_hint = m.to_string();
                    }
                }
            }
            ("event_msg", "user_message") => {
                if let Some(prev) = cur.take() {
                    turns.push(prev);
                }
                let text = payload
                    .get("message")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                cur = Some(Turn {
                    user_message: text.to_string(),
                    project_path: project_path.clone(),
                    session_id: session_id.clone(),
                    calls: Vec::new(),
                    timestamp_ms: ts_ms,
                });
            }
            ("event_msg", "agent_message") => {
                push_call(
                    &mut cur,
                    &project_path,
                    &session_id,
                    ts_ms,
                    CallRecord {
                        model: model_hint.clone(),
                        message_id: None,
                        usage: UsageSummary::default(),
                        cost_usd: 0.0,
                        tools: Vec::new(),
                        bash_commands: Vec::new(),
                        mcp_servers: Vec::new(),
                        has_plan_mode: false,
                        has_agent_spawn: false,
                    },
                );
            }
            ("response_item", "function_call") | ("response_item", "custom_tool_call") => {
                let name = payload
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let raw_args = payload
                    .get("arguments")
                    .or_else(|| payload.get("input"))
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                let mut bash_commands: Vec<String> = Vec::new();
                let mut mcp_servers: Vec<String> = Vec::new();
                if name == "shell" || name == "Bash" || name == "BashTool" {
                    if let Some(cmd) = shell_util::extract_first_command(&raw_args) {
                        bash_commands.push(cmd);
                    }
                }
                if let Some(server) = shell_util::extract_mcp_server(&name) {
                    mcp_servers.push(server);
                }
                let spawn = matches!(name.as_str(), "Task" | "Agent" | "task_spawn");
                push_call(
                    &mut cur,
                    &project_path,
                    &session_id,
                    ts_ms,
                    CallRecord {
                        model: model_hint.clone(),
                        message_id: None,
                        usage: UsageSummary::default(),
                        cost_usd: 0.0,
                        tools: vec![name],
                        bash_commands,
                        mcp_servers,
                        has_plan_mode: false,
                        has_agent_spawn: spawn,
                    },
                );
            }
            ("response_item", "web_search_call") => {
                push_call(
                    &mut cur,
                    &project_path,
                    &session_id,
                    ts_ms,
                    CallRecord {
                        model: model_hint.clone(),
                        message_id: None,
                        usage: UsageSummary::default(),
                        cost_usd: 0.0,
                        tools: vec!["WebSearch".to_string()],
                        bash_commands: Vec::new(),
                        mcp_servers: Vec::new(),
                        has_plan_mode: false,
                        has_agent_spawn: false,
                    },
                );
            }
            ("event_msg", "token_count") => {
                let Some(info) = payload.get("info") else {
                    continue;
                };
                if info.is_null() {
                    continue;
                }
                let Some(tt) = info.get("total_token_usage") else {
                    continue;
                };
                last_usage = read_codex_total_usage(tt);
            }
            _ => {}
        }
    }
    if let Some(t) = cur {
        turns.push(t);
    }
    // 把累积 usage 灌到最后一个 call。如果完全没有 call，就丢弃 usage（前端不显示）。
    if last_usage.total > 0 {
        if let Some(last_turn) = turns.last_mut() {
            if let Some(last_call) = last_turn.calls.last_mut() {
                last_call.usage = last_usage;
                last_call.cost_usd = pricing::cost_usd(&last_call.model, &last_call.usage);
            }
        }
    }
    turns
}

/// 起 / 追加一个 call —— 没有进行中的 user-turn 时起一个空 user_message 的占位。
fn push_call(
    cur: &mut Option<Turn>,
    project_path: &str,
    session_id: &str,
    ts_ms: i64,
    call: CallRecord,
) {
    if let Some(turn) = cur.as_mut() {
        turn.calls.push(call);
    } else {
        *cur = Some(Turn {
            user_message: String::new(),
            project_path: project_path.to_string(),
            session_id: session_id.to_string(),
            calls: vec![call],
            timestamp_ms: ts_ms,
        });
    }
}

/// Codex 把 token 用量写在 event_msg.token_count 事件里，且每次更新都是**累积值**
/// （`total_token_usage`）—— 所以只需要扫到最后一行非空的就行。
///
/// 形状：
///   {"type":"event_msg","payload":{"type":"token_count","info":{
///       "total_token_usage":{"input_tokens":N,"cached_input_tokens":N,
///         "output_tokens":N,"reasoning_output_tokens":N,"total_tokens":N},
///       ...}}}
///
/// 早期写入时 `info` 可能为 null（订阅尚未拿到 usage），跳过；后续的覆盖前面的。
fn usage_summary(fp: &Path) -> Result<UsageSummary, String> {
    let file = match fs::File::open(fp) {
        Ok(f) => f,
        Err(_) => return Ok(UsageSummary::default()),
    };
    let mut last = UsageSummary::default();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if v.get("type").and_then(Value::as_str) != Some("event_msg") {
            continue;
        }
        let payload = match v.get("payload") {
            Some(p) => p,
            None => continue,
        };
        if payload.get("type").and_then(Value::as_str) != Some("token_count") {
            continue;
        }
        let Some(info) = payload.get("info") else {
            continue;
        };
        if info.is_null() {
            continue;
        }
        let Some(t) = info.get("total_token_usage") else {
            continue;
        };
        last = read_codex_total_usage(t);
    }
    Ok(last)
}

/// Codex 的 `total_token_usage.input_tokens` **包含** cached_input_tokens
/// （上游 API 报的就是含 cache 的总输入），所以前端展示 "in / cached" 两栏时
/// 必须减出来 —— 否则汇总里的 in 就把 cache 多算了一遍，cache hit 高（90%+）
/// 时被夸大到 8~10×（codeburn 同样按减法处理）。
fn read_codex_total_usage(t: &Value) -> UsageSummary {
    let total_input = t.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
    let cached = t
        .get("cached_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = t.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
    let reasoning = t
        .get("reasoning_output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    UsageSummary {
        // saturating_sub 防御性：极少数情况下 cached > total_input（API 抖动），
        // 此时把 new-input 当 0 处理，cached 仍然保留。
        input_tokens: total_input.saturating_sub(cached),
        output_tokens: output,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: cached,
        reasoning_output_tokens: reasoning,
        total: 0,
    }
    .finalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, lines: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("csv-codex-usage-tests");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        p
    }

    #[test]
    fn usage_takes_the_last_non_null_token_count_event() {
        // 早期 info:null 的事件被跳过；后面的累积值（total_token_usage）覆盖。
        // input_tokens 字段 codex 报的是"含 cached"的总输入，本函数会减出来。
        let p = write_temp(
            "codex-last.jsonl",
            &[
                r#"{"type":"event_msg","payload":{"type":"token_count","info":null}}"#,
                r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":30,"output_tokens":40,"reasoning_output_tokens":20,"total_tokens":190}}}}"#,
                r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":200,"cached_input_tokens":60,"output_tokens":80,"reasoning_output_tokens":35,"total_tokens":375}}}}"#,
            ],
        );
        let u = usage_summary(&p).unwrap();
        // 最后一条 total: input=200 含 60 cached → new input = 140
        assert_eq!(u.input_tokens, 140);
        assert_eq!(u.cache_read_input_tokens, 60);
        assert_eq!(u.output_tokens, 80);
        assert_eq!(u.reasoning_output_tokens, 35);
        // total = (200-60) + 80 + 60 (cache_read) + 0 (cache_creation) + 35 (reasoning) = 315
        assert_eq!(u.total, 315);
    }

    #[test]
    fn usage_handles_cached_greater_than_input_defensively() {
        // 防御性：API 抖动时 cached > input —— new input 应该按 0 处理而不是 panic。
        let p = write_temp(
            "codex-defensive.jsonl",
            &[
                r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":50,"cached_input_tokens":100,"output_tokens":10,"reasoning_output_tokens":0,"total_tokens":160}}}}"#,
            ],
        );
        let u = usage_summary(&p).unwrap();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.cache_read_input_tokens, 100);
    }

    #[test]
    fn usage_ignores_unrelated_events() {
        let p = write_temp(
            "codex-noise.jsonl",
            &[
                r#"{"type":"response_item","payload":{"type":"message"}}"#,
                r#"{"type":"event_msg","payload":{"type":"user_message"}}"#,
            ],
        );
        assert_eq!(usage_summary(&p).unwrap(), UsageSummary::default());
    }

    #[test]
    fn usage_returns_default_when_file_missing() {
        let p = std::path::PathBuf::from("/tmp/csv-codex-usage-tests/nope.jsonl");
        assert_eq!(usage_summary(&p).unwrap(), UsageSummary::default());
    }

    #[test]
    fn thread_flags_do_not_treat_missing_user_event_alone_as_internal() {
        let flags =
            thread_flags_from_fields(false, 0, r#"{"local":true}"#, None, Some("gpt-5-codex"));
        assert!(!flags.internal);
        assert!(!flags.archived);
    }

    #[test]
    fn thread_flags_detect_guardian_subagent_and_archive_independently() {
        let flags = thread_flags_from_fields(
            true,
            0,
            r#"{"subagent":{"other":"guardian"}}"#,
            Some("subagent"),
            Some("codex-auto-review"),
        );
        assert!(flags.internal);
        assert!(flags.archived);
        assert!(include_by_flags(flags, false, true));
    }

    #[test]
    fn read_turns_picks_up_model_from_turn_context_so_cost_is_nonzero() {
        // 回归：早期实现只看 session_meta.originator.model / .model；真实 codex JSONL 的
        // session_meta.originator 是字符串（"codex-tui"），model 字段不存在 → 全 session $0。
        // 现在 turn_context.payload.model 是真正的 model 源。
        let p = write_temp(
            "codex-turn-context-model.jsonl",
            &[
                r#"{"type":"session_meta","payload":{"id":"abc","cwd":"/tmp","originator":"codex-tui"}}"#,
                r#"{"type":"turn_context","payload":{"turn_id":"t1","model":"gpt-5"}}"#,
                r#"{"type":"event_msg","payload":{"type":"user_message","message":"hi"}}"#,
                r#"{"type":"event_msg","payload":{"type":"agent_message","message":"hey"}}"#,
                r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":500,"reasoning_output_tokens":0,"total_tokens":1500}}}}"#,
            ],
        );
        let turns = read_turns(&p);
        let last_call = turns
            .last()
            .and_then(|t| t.calls.last())
            .expect("expected at least one call");
        assert_eq!(last_call.model, "gpt-5");
        assert!(
            last_call.cost_usd > 0.0,
            "expected non-zero cost, got {}",
            last_call.cost_usd
        );
    }

    #[test]
    fn read_turns_uses_latest_turn_context_when_model_changes_mid_session() {
        // mid-session 切模型（gpt-5.3-codex → gpt-5.5），最后一条 turn_context 胜出。
        let p = write_temp(
            "codex-model-switch.jsonl",
            &[
                r#"{"type":"session_meta","payload":{"id":"abc","cwd":"/tmp","originator":"codex-tui"}}"#,
                r#"{"type":"turn_context","payload":{"turn_id":"t1","model":"gpt-5.3-codex"}}"#,
                r#"{"type":"event_msg","payload":{"type":"user_message","message":"a"}}"#,
                r#"{"type":"event_msg","payload":{"type":"agent_message","message":"b"}}"#,
                r#"{"type":"turn_context","payload":{"turn_id":"t2","model":"gpt-5.5"}}"#,
                r#"{"type":"event_msg","payload":{"type":"user_message","message":"c"}}"#,
                r#"{"type":"event_msg","payload":{"type":"agent_message","message":"d"}}"#,
                r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":500,"reasoning_output_tokens":0,"total_tokens":1500}}}}"#,
            ],
        );
        let turns = read_turns(&p);
        let last_call = turns.last().and_then(|t| t.calls.last()).expect("call");
        assert_eq!(last_call.model, "gpt-5.5");
    }

    #[test]
    #[ignore = "manual full-scan; reads every Codex rollout on disk"]
    fn dedup_full_codex_scan() {
        let src = CodexSource;
        let projects = src.list_projects(false, false).unwrap();
        let mut agg = crate::stats::aggregate::Aggregator::new();
        for p in &projects {
            let sessions = src.discover_stats_sessions(&p.dir_name).unwrap_or_default();
            for s in sessions {
                let turns = read_turns(std::path::Path::new(&s.path));
                agg.feed_session(&crate::stats::aggregate::SessionFeed {
                    agent: "codex",
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
        let s = agg.snapshot("codex");
        eprintln!("\n=== FULL CODEX SCAN ===");
        eprintln!("sessions: {}", s.session_count);
        eprintln!("calls: {}", s.call_count);
        eprintln!("cost: ${:.2}", s.cost_usd);
        eprintln!(
            "input: {} ({:.1}M)",
            s.usage.input_tokens,
            s.usage.input_tokens as f64 / 1e6
        );
        eprintln!(
            "output: {} ({:.1}M)",
            s.usage.output_tokens,
            s.usage.output_tokens as f64 / 1e6
        );
        eprintln!(
            "cache_read: {} ({:.1}M)",
            s.usage.cache_read_input_tokens,
            s.usage.cache_read_input_tokens as f64 / 1e6
        );
    }
}
