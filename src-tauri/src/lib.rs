// AI 会话管理器 —— 后端
// 同时管理 Claude Code (~/.claude/projects) 与 Codex (~/.codex/sessions) 的本地会话。

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn home() -> PathBuf {
    dirs::home_dir().expect("无法定位用户主目录")
}

fn claude_projects_dir() -> PathBuf {
    home().join(".claude").join("projects")
}

fn codex_sessions_dir() -> PathBuf {
    home().join(".codex").join("sessions")
}

/// 在 ~/.codex 下找编号最大的 state_<N>.sqlite —— codex 用版本号区分 schema，
/// 升级时会写到新文件（state_4.sqlite → state_5.sqlite），picker 用最新的那个。
/// 没找到时返回 None，调用方应静默跳过 sqlite 更新（codex 旧版本或从未运行）。
fn find_codex_state_db() -> Option<PathBuf> {
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

/// 软删除的会话存放目录（应用自管的"回收站备份目录"）。
fn trash_dir() -> PathBuf {
    let d = home().join(".claude").join(".session-viewer-trash");
    let _ = fs::create_dir_all(&d);
    d
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn mtime_millis(p: &Path) -> u64 {
    fs::metadata(p)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn is_jsonl(p: &Path) -> bool {
    p.extension().map(|x| x == "jsonl").unwrap_or(false)
}

// ============================ 公共数据结构 ============================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectInfo {
    /// 项目标识：Claude 为目录名，Codex 为 cwd 路径。
    dir_name: String,
    display_path: String,
    session_count: usize,
    last_modified: u64,
    /// 项目目录当前是否仍存在于磁盘上。
    exists: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionMeta {
    id: String,
    file_name: String,
    path: String,
    title: String,
    cwd: Option<String>,
    created: Option<String>,
    modified: u64,
    size: u64,
    message_count: usize,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct DiffLine {
    kind: String, // ctx | add | del
    old_no: Option<u32>,
    new_no: Option<u32>,
    text: String,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct DiffHunk {
    old_start: u32,
    new_start: u32,
    lines: Vec<DiffLine>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct Block {
    kind: String, // text | thinking | tool_use | tool_result | image
    text: Option<String>,
    tool_name: Option<String>,
    tool_input: Option<String>,
    tool_id: Option<String>,
    is_error: bool,
    /// 文件改动类工具结果携带的目标文件路径。
    file_path: Option<String>,
    /// 文件改动的结构化 diff（来自 Claude 的 structuredPatch）。
    diff: Option<Vec<DiffHunk>>,
    /// 图片源：通常为 data:<mime>;base64,<...> 的内联 URL。
    image_src: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Msg {
    uuid: Option<String>,
    role: String,
    timestamp: Option<String>,
    model: Option<String>,
    sidechain: bool,
    blocks: Vec<Block>,
}

// ============================ 命令分发 ============================

#[tauri::command]
fn list_projects(agent: String) -> Result<Vec<ProjectInfo>, String> {
    match agent.as_str() {
        "codex" => Ok(list_codex_projects()),
        _ => list_claude_projects(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionPage {
    /// 该项目会话总数（用于前端判断是否还有下一页）。
    total: usize,
    sessions: Vec<SessionMeta>,
}

/// 分页返回会话：先按修改时间排序（廉价），只对窗口内的文件做深度解析。
#[tauri::command]
fn list_sessions(
    agent: String,
    project_key: String,
    offset: usize,
    limit: usize,
) -> Result<SessionPage, String> {
    match agent.as_str() {
        "codex" => Ok(list_codex_sessions(&project_key, offset, limit)),
        _ => list_claude_sessions(&project_key, offset, limit),
    }
}

#[tauri::command]
fn read_session(agent: String, path: String) -> Result<Vec<Msg>, String> {
    match agent.as_str() {
        "codex" => read_codex_session(&path),
        _ => read_claude_session(&path),
    }
}

// ============================ Claude ============================

fn list_claude_projects() -> Result<Vec<ProjectInfo>, String> {
    let dir = claude_projects_dir();
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
                        cwd = claude_cwd(&fp);
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

fn list_claude_sessions(dir_name: &str, offset: usize, limit: usize) -> Result<SessionPage, String> {
    let pdir = claude_projects_dir().join(dir_name);
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
        .map(|(p, _)| scan_claude_session(p))
        .collect();
    Ok(SessionPage { total, sessions })
}

/// 单遍扫描一个 Claude jsonl，提取标题 / 时间 / 消息数等元信息。
fn scan_claude_session(fp: &Path) -> SessionMeta {
    let file_name = fp
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let id = file_name.trim_end_matches(".jsonl").to_string();
    let size = fs::metadata(fp).map(|m| m.len()).unwrap_or(0);
    let modified = mtime_millis(fp);

    // Claude Code `/rename <name>` 会追加一行 `{"type":"custom-title", ...}`，
    // 最后一条生效。优先使用它，否则回落到首条 user message。
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
            if t == "custom-title" {
                if let Some(ct) = v.get("customTitle").and_then(|x| x.as_str()) {
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
            if first_user_title.is_empty() && t == "user" {
                if let Some(txt) = claude_user_text(&v) {
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

fn read_claude_session(path: &str) -> Result<Vec<Msg>, String> {
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
                                let txt = stringify_claude_tool_result(el.get("content"));
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
                                if let Some(src) = claude_image_src(el) {
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
        // 我们已经把真实图渲染在上一条里，跳过 meta 那条避免出现重复气泡。
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

fn claude_cwd(fp: &Path) -> Option<String> {
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

fn claude_user_text(v: &Value) -> Option<String> {
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

/// 把 Claude 用户消息里 `type: "image"` 的元素转成可直接 `<img src=...>` 的串。
/// 目前只识别 base64 内联编码（这是 Claude Code 实际写入的形式）。
fn claude_image_src(el: &Value) -> Option<String> {
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
/// 实际贴图已经在上一条 user 记录里以 base64 渲染过了，所以这种纯元数据直接丢弃。
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
        // 支持 Claude Code 已知的两类元行：source: ... / original WxH, displayed at ...
        let inner = txt
            .trim_start_matches("[Image:")
            .trim_start();
        inner.starts_with("source:") || inner.starts_with("original")
    })
}

fn stringify_claude_tool_result(c: Option<&Value>) -> String {
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

// ============================ Codex ============================

struct CodexMeta {
    id: String,
    cwd: String,
    created: Option<String>,
}

/// 递归收集 ~/.codex/sessions 下所有 rollout-*.jsonl。
fn codex_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_jsonl(&codex_sessions_dir(), &mut out);
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
fn codex_meta(path: &Path) -> Option<CodexMeta> {
    let file = fs::File::open(path).ok()?;
    let mut first = String::new();
    BufReader::new(file).read_line(&mut first).ok()?;
    let v: Value = serde_json::from_str(first.trim()).ok()?;
    if v.get("type").and_then(|x| x.as_str()) != Some("session_meta") {
        return None;
    }
    let p = v.get("payload")?;
    Some(CodexMeta {
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

fn list_codex_projects() -> Vec<ProjectInfo> {
    let mut map: HashMap<String, (usize, u64)> = HashMap::new();
    for fp in codex_files() {
        if let Some(m) = codex_meta(&fp) {
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
    out
}

fn list_codex_sessions(cwd: &str, offset: usize, limit: usize) -> SessionPage {
    // 廉价阶段：只读每个文件首行 session_meta，筛出本项目的文件并取修改时间。
    let mut matched: Vec<(PathBuf, CodexMeta, u64)> = Vec::new();
    for fp in codex_files() {
        if let Some(m) = codex_meta(&fp) {
            if m.cwd == cwd {
                let mt = mtime_millis(&fp);
                matched.push((fp, m, mt));
            }
        }
    }
    matched.sort_by_key(|m| std::cmp::Reverse(m.2));
    let total = matched.len();
    // Codex 把会话标题缓存在 ~/.codex/session_index.jsonl（append-only，同 id 多条
    // 时最新一条胜出），无论 codex CLI 自带 rename 还是我们的 rename 都会写到这里。
    // 列表整页加载一次即可，避免每个会话都重读一次文件。
    let title_index = load_codex_title_index();
    let sessions = matched
        .iter()
        .skip(offset)
        .take(limit)
        .map(|(p, m, _)| scan_codex_session(p, m, &title_index))
        .collect();
    SessionPage { total, sessions }
}

/// 读取 `~/.codex/session_index.jsonl`，返回 thread_id → 最新 thread_name。
/// 文件不存在 / 不可读时返回空 map，调用方自动回落到旧的 JSONL 内联策略。
fn load_codex_title_index() -> HashMap<String, String> {
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

fn scan_codex_session(
    fp: &Path,
    meta: &CodexMeta,
    title_index: &HashMap<String, String>,
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
                if let Some(m) = p.get("message").and_then(|x| x.as_str()) {
                    let clean = clean_title(m);
                    if !clean.is_empty() {
                        first_user_title = clean;
                    }
                }
            }
        }
    }
    let id = if meta.id.is_empty() {
        file_name.trim_end_matches(".jsonl").to_string()
    } else {
        meta.id.clone()
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
        cwd: Some(meta.cwd.clone()),
        created: meta.created.clone(),
        modified,
        size,
        message_count,
    }
}

/// 解析 Codex rollout：用 event_msg 取干净的对话文本，用 response_item 取工具调用。
fn read_codex_session(path: &str) -> Result<Vec<Msg>, String> {
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
            ("event_msg", "user_message") => {
                if let Some(m) = p.get("message").and_then(|x| x.as_str()) {
                    if !m.trim().is_empty() {
                        msgs.push(simple_msg("user", ts, text_block("text", m)));
                    }
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
                let input = format_codex_args(p.get("arguments").or_else(|| p.get("input")));
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
                let out = codex_output_text(p.get("output"));
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
    Ok(msgs)
}

fn format_codex_args(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => match serde_json::from_str::<Value>(s) {
            Ok(parsed) => serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| s.clone()),
            Err(_) => s.clone(),
        },
        Some(other) => serde_json::to_string_pretty(other).unwrap_or_default(),
        None => String::new(),
    }
}

fn codex_output_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// 取 Codex 文件的首条用户输入作为标题（用于回收站展示）。
fn codex_first_user_text(fp: &Path) -> String {
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

// ============================ 回收站 ============================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrashItem {
    trash_file: String,
    agent: String,
    project_label: String,
    original_path: String,
    deleted_at: u64,
    title: String,
    size: u64,
}

/// 重命名会话：与 Claude Code `/rename` / Codex 内部重命名一致，
/// 在原 JSONL 末尾追加一条官方 schema 的元数据行（append-only），
/// 后续扫描时取最后一条 `custom-title` / `thread_name_updated` 作为标题。
#[tauri::command]
fn rename_session(agent: String, path: String, name: String) -> Result<(), String> {
    use std::io::Write;
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("名称不能为空".to_string());
    }
    if trimmed.chars().count() > 200 {
        return Err("名称过长".to_string());
    }
    let fp = PathBuf::from(&path);
    if !fp.exists() {
        return Err("会话文件不存在".to_string());
    }
    if !is_jsonl(&fp) {
        return Err("不是 JSONL 文件".to_string());
    }
    let id = fp
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.trim_end_matches(".jsonl").to_string())
        .unwrap_or_default();

    // Codex 文件名形如 rollout-<ts>-<uuid>.jsonl，需要从文件名末尾或 session_meta 头里
    // 取真正的 thread_id（UUID）。这里优先读首行 session_meta.payload.id。
    let codex_id = if agent == "codex" {
        let mut found: Option<String> = None;
        if let Ok(file) = fs::File::open(&fp) {
            for line in BufReader::new(file).lines().map_while(Result::ok).take(8) {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if v.get("type").and_then(|x| x.as_str()) == Some("session_meta") {
                        if let Some(idv) = v
                            .get("payload")
                            .and_then(|p| p.get("id"))
                            .and_then(|x| x.as_str())
                        {
                            found = Some(idv.to_string());
                            break;
                        }
                    }
                }
            }
        }
        found.unwrap_or_else(|| id.clone())
    } else {
        id.clone()
    };

    let line = match agent.as_str() {
        "claude" => serde_json::json!({
            "type": "custom-title",
            "customTitle": trimmed,
            "sessionId": id,
        })
        .to_string(),
        "codex" => {
            // ISO-8601 UTC 时间戳，毫秒精度，跟 codex-tui 写出来的一致
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let secs = (now / 1000) as i64;
            let ms = (now % 1000) as u32;
            let ts = format_iso8601_utc(secs, ms);
            serde_json::json!({
                "timestamp": ts,
                "type": "event_msg",
                "payload": {
                    "type": "thread_name_updated",
                    "thread_id": codex_id,
                    "thread_name": trimmed,
                },
            })
            .to_string()
        }
        _ => return Err(format!("未知 agent: {agent}")),
    };

    let mut f = fs::OpenOptions::new()
        .append(true)
        .open(&fp)
        .map_err(|e| format!("打开会话文件失败: {e}"))?;
    // 万一文件末尾没有换行符，补一个再追加，避免破坏既有最后一行
    let needs_nl = fs::metadata(&fp)
        .map(|m| m.len())
        .ok()
        .and_then(|len| {
            if len == 0 {
                Some(false)
            } else {
                use std::io::{Read, Seek, SeekFrom};
                let mut g = fs::File::open(&fp).ok()?;
                g.seek(SeekFrom::End(-1)).ok()?;
                let mut buf = [0u8; 1];
                g.read_exact(&mut buf).ok()?;
                Some(buf[0] != b'\n')
            }
        })
        .unwrap_or(false);
    if needs_nl {
        f.write_all(b"\n").map_err(|e| format!("追加换行失败: {e}"))?;
    }
    f.write_all(line.as_bytes())
        .map_err(|e| format!("写入 rename 行失败: {e}"))?;
    f.write_all(b"\n").map_err(|e| format!("写入换行失败: {e}"))?;

    // Codex 的 `codex resume` 选择列表读的是 `~/.codex/session_index.jsonl`。
    // 实测：同 id 多条时 codex picker **取首次出现的那条**（不是按 `updated_at`
    // 排序）。所以这里不能单纯 append，必须先把同 id 的旧条目过滤掉，再把新
    // 条目写到末尾——这样新 rename 一定能被读到，又跟 codex 自己写入的格式兼容。
    if agent == "codex" {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let secs = (now_ms / 1000) as i64;
        let ms = (now_ms % 1000) as u32;
        let updated_at = format_iso8601_utc(secs, ms).replace('Z', "000Z");
        let new_entry = serde_json::json!({
            "id": codex_id,
            "thread_name": trimmed,
            "updated_at": updated_at,
        })
        .to_string();

        let idx_path = home().join(".codex").join("session_index.jsonl");
        // 读取既有行，剔除同 id 的旧条目
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
        let parent = idx_path.parent().ok_or_else(|| "session_index 父目录不存在".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建 .codex 目录失败: {e}"))?;
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

        // codex 的 `/resume` picker 真正的权威数据源是 ~/.codex/state_<N>.sqlite
        // 的 threads.title 列。如果只改 session_index.jsonl 不改 sqlite，picker 仍会
        // 显示 sqlite 里的旧 title。这里把同一个 id 的 title 改掉，updated_at 自然
        // 触发 trigger 更新 updated_at_ms。文件不存在则跳过（codex 旧版本 / 用户从未
        // 启动过 codex CLI）。
        if let Some(db_path) = find_codex_state_db() {
            let now_secs = (now_ms / 1000) as i64;
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| format!("打开 codex sqlite 失败: {e}"))?;
            conn.execute(
                "UPDATE threads SET title = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![trimmed, now_secs, &codex_id],
            )
            .map_err(|e| format!("更新 threads.title 失败: {e}"))?;
        }
    }
    Ok(())
}

/// 简易 ISO-8601 UTC 时间字符串：1970-01-01T00:00:00.000Z 风格。
/// 只用于 codex thread_name_updated 行的 timestamp 字段，所以不需要复杂时区/月份逻辑外的精度。
fn format_iso8601_utc(secs: i64, ms: u32) -> String {
    // 算出 YYYY-MM-DDTHH:MM:SS.mmmZ
    let s = secs.rem_euclid(60) as u32;
    let m = (secs.div_euclid(60)).rem_euclid(60) as u32;
    let h = (secs.div_euclid(3600)).rem_euclid(24) as u32;
    let mut days = secs.div_euclid(86400) as i64;
    // 1970-01-01 是 Unix epoch
    let mut year: i64 = 1970;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let yd = if leap { 366 } else { 365 };
        if days < yd {
            break;
        }
        days -= yd;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let mdays = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month: usize = 0;
    while month < 12 && days >= mdays[month] as i64 {
        days -= mdays[month] as i64;
        month += 1;
    }
    let day = days as u32 + 1;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year,
        month + 1,
        day,
        h,
        m,
        s,
        ms
    )
}

/// 软删除：把会话移入备份目录，并写一个 .meta 旁车文件记录来源。
#[tauri::command]
fn soft_delete_session(
    agent: String,
    path: String,
    project_label: String,
) -> Result<(), String> {
    let src = PathBuf::from(&path);
    if !src.exists() {
        return Err("会话文件不存在".to_string());
    }
    let base = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "session.jsonl".to_string());
    let now = now_millis();
    let trash_name = format!("{now}-{base}");
    let td = trash_dir();
    let dest = td.join(&trash_name);
    fs::rename(&src, &dest)
        .or_else(|_| {
            fs::copy(&src, &dest)
                .and_then(|_| fs::remove_file(&src))
                .map(|_| ())
        })
        .map_err(|e| format!("移入回收站失败: {e}"))?;
    let meta = serde_json::json!({
        "agent": agent,
        "originalPath": path,
        "projectLabel": project_label,
        "deletedAt": now,
    });
    fs::write(td.join(format!("{trash_name}.meta")), meta.to_string())
        .map_err(|e| format!("写入回收站元数据失败: {e}"))?;
    Ok(())
}

#[tauri::command]
fn list_trash() -> Result<Vec<TrashItem>, String> {
    let td = trash_dir();
    let mut out = Vec::new();
    let entries = fs::read_dir(&td).map_err(|e| format!("读取回收站失败: {e}"))?;
    for f in entries.flatten() {
        let fp = f.path();
        if !is_jsonl(&fp) {
            continue;
        }
        let trash_file = f.file_name().to_string_lossy().to_string();
        let meta_path = td.join(format!("{trash_file}.meta"));
        let meta: Value = fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let agent = meta
            .get("agent")
            .and_then(|x| x.as_str())
            .unwrap_or("claude")
            .to_string();
        let original_path = meta
            .get("originalPath")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let project_label = meta
            .get("projectLabel")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let deleted_at = meta.get("deletedAt").and_then(|x| x.as_u64()).unwrap_or(0);
        let title = if agent == "codex" {
            codex_first_user_text(&fp)
        } else {
            scan_claude_session(&fp).title
        };
        out.push(TrashItem {
            trash_file,
            agent,
            project_label,
            original_path,
            deleted_at,
            title,
            size: fs::metadata(&fp).map(|m| m.len()).unwrap_or(0),
        });
    }
    out.sort_by_key(|t| std::cmp::Reverse(t.deleted_at));
    Ok(out)
}

#[tauri::command]
fn restore_session(trash_file: String) -> Result<(), String> {
    let td = trash_dir();
    let src = td.join(&trash_file);
    let meta_path = td.join(format!("{trash_file}.meta"));
    let s = fs::read_to_string(&meta_path).map_err(|_| "缺少元数据，无法确定恢复位置".to_string())?;
    let v: Value = serde_json::from_str(&s).map_err(|e| format!("元数据损坏: {e}"))?;
    let original_path = v
        .get("originalPath")
        .and_then(|x| x.as_str())
        .ok_or("元数据缺少原始路径")?;
    let dest = PathBuf::from(original_path);
    if dest.exists() {
        return Err("原位置已存在同名会话，无法恢复".to_string());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    fs::rename(&src, &dest).map_err(|e| format!("恢复失败: {e}"))?;
    let _ = fs::remove_file(&meta_path);
    Ok(())
}

#[tauri::command]
fn permanent_delete_trash(trash_file: String) -> Result<(), String> {
    let td = trash_dir();
    fs::remove_file(td.join(&trash_file)).map_err(|e| format!("永久删除失败: {e}"))?;
    let _ = fs::remove_file(td.join(format!("{trash_file}.meta")));
    Ok(())
}

#[tauri::command]
fn empty_trash() -> Result<(), String> {
    let td = trash_dir();
    let entries = fs::read_dir(&td).map_err(|e| format!("读取回收站失败: {e}"))?;
    for f in entries.flatten() {
        let _ = fs::remove_file(f.path());
    }
    Ok(())
}

/// 在终端中用对应 CLI 恢复（resume）一个会话。
#[tauri::command]
fn resume_session(agent: String, session_id: String, cwd: String) -> Result<(), String> {
    if !Path::new(&cwd).is_dir() {
        return Err("项目目录已不存在，无法恢复".to_string());
    }
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("会话 ID 非法".to_string());
    }
    let cli = match agent.as_str() {
        "codex" => format!("codex resume {session_id}"),
        _ => format!("claude --resume {session_id}"),
    };

    #[cfg(target_os = "macos")]
    {
        let cwd_quoted = cwd.replace('\'', "'\\''");
        let shell_cmd = format!("cd '{cwd_quoted}' && {cli}");
        let as_arg = shell_cmd.replace('\\', "\\\\").replace('"', "\\\"");
        let script =
            format!("tell application \"Terminal\"\nactivate\ndo script \"{as_arg}\"\nend tell");
        std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .spawn()
            .map_err(|e| format!("启动终端失败: {e}"))?;
    }

    #[cfg(target_os = "windows")]
    {
        let cwd_win = cwd.replace('/', "\\");
        let ps_cmd = format!("Set-Location \"{}\"; {}", cwd_win, cli);
        let launched = std::process::Command::new("cmd")
            .args(["/c", "start", "powershell", "-NoExit", "-Command", &ps_cmd])
            .spawn()
            .is_ok();
        if !launched {
            std::process::Command::new("cmd")
                .args(["/c", "start", "cmd", "/k", &format!("cd /d \"{}\" && {}", cwd_win, cli)])
                .spawn()
                .map_err(|e| format!("启动终端失败: {e}"))?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        let shell_cmd = format!("cd '{}' && {}", cwd.replace('\'', "'\\''"), cli);
        let terminals = ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"];
        let mut launched = false;
        for term in &terminals {
            let result = if *term == "gnome-terminal" {
                std::process::Command::new(term)
                    .args(["--", "bash", "-c", &shell_cmd])
                    .spawn()
            } else {
                std::process::Command::new(term)
                    .args(["-e", &format!("bash -c '{}'", shell_cmd.replace('\'', "'\\''"))])
                    .spawn()
            };
            if result.is_ok() {
                launched = true;
                break;
            }
        }
        if !launched {
            return Err("未找到可用的终端程序".to_string());
        }
    }

    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    current: String,
    latest: String,
    has_update: bool,
}

#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 检查更新（占位实现：当前没有发布渠道，直接返回"已是最新"）。
#[tauri::command]
fn check_update() -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    // TODO: 接入实际的远端版本源（如 GitHub Releases）
    Ok(UpdateInfo {
        current: current.clone(),
        latest: current,
        has_update: false,
    })
}

/// 在系统文件管理器中显示该文件。
#[tauri::command]
fn reveal_in_finder(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("打开访达失败: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.replace('/', "\\")))
            .spawn()
            .map_err(|e| format!("打开资源管理器失败: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        let parent = std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(path);
        std::process::Command::new("xdg-open")
            .arg(&parent)
            .spawn()
            .map_err(|e| format!("打开文件管理器失败: {e}"))?;
    }
    Ok(())
}

// ============================ 通用辅助 ============================

/// 把首条用户消息清洗成简短标题：去掉 <...> 标记块、折叠空白、截断。
fn clean_title(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("Caveat:") {
        return String::new();
    }
    let mut out = String::new();
    let mut depth = 0i32;
    for c in trimmed.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let collapsed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(100).collect()
}

fn text_block(kind: &str, s: &str) -> Block {
    Block {
        kind: kind.to_string(),
        text: Some(s.to_string()),
        ..Default::default()
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

fn simple_msg(role: &str, ts: Option<String>, block: Block) -> Msg {
    Msg {
        uuid: None,
        role: role.to_string(),
        timestamp: ts,
        model: None,
        sidechain: false,
        blocks: vec![block],
    }
}

/// Attach an empty `NSToolbar` with `unifiedCompact` style so AppKit grows the
/// titlebar to ~40px and auto-centers the traffic lights vertically inside it
/// — matching our 40px CSS topbar. This is the SUPPORTED AppKit way to extend
/// the titlebar; manually `setFrameOrigin`-ing the standardWindowButtons works
/// visually but appears to confuse AppKit's titlebar drag tracking (focused
/// click→drag stops working).
#[cfg(target_os = "macos")]
fn pin_traffic_lights(window: &tauri::WebviewWindow) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSToolbar, NSWindow, NSWindowToolbarStyle};

    let ns_window_ptr = match window.ns_window() {
        Ok(p) => p as *mut AnyObject,
        Err(_) => return,
    };
    if ns_window_ptr.is_null() {
        return;
    }

    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return;
    };
    unsafe {
        let ns_window: Retained<NSWindow> = match Retained::retain(ns_window_ptr.cast::<NSWindow>()) {
            Some(w) => w,
            None => return,
        };
        if ns_window.toolbar().is_some() {
            return; // 已挂好，避免重复
        }
        let toolbar = NSToolbar::new(mtm);
        ns_window.setToolbar(Some(&toolbar));
        ns_window.setToolbarStyle(NSWindowToolbarStyle::UnifiedCompact);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_projects,
            list_sessions,
            read_session,
            rename_session,
            soft_delete_session,
            list_trash,
            restore_session,
            permanent_delete_trash,
            empty_trash,
            resume_session,
            reveal_in_finder,
            app_version,
            check_update,
        ])
        .setup(|_app| {
            #[cfg(target_os = "macos")]
            {
                use tauri::Manager;
                if let Some(win) = _app.get_webview_window("main") {
                    pin_traffic_lights(&win);
                    // AppKit relays out standard window buttons on resize,
                    // so re-pin then. Avoid Focused / ThemeChanged: AppKit
                    // does NOT recreate the buttons on those events, and
                    // running Objective-C work inside the Focused handler
                    // can race the click→drag transition and break titlebar
                    // dragging when focusing the window from a click.
                    let win_clone = win.clone();
                    win.on_window_event(move |e| {
                        if matches!(e, tauri::WindowEvent::Resized(_)) {
                            pin_traffic_lights(&win_clone);
                        }
                    });
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
