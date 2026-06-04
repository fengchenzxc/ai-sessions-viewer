// 流式统计编排：把 (scope, range, requestId) 翻译成 SessionFeed 序列，喂给
// `Aggregator`，并在合适的节奏 emit `stats://progress` / `stats://done` / `stats://error`。
//
// 关键决定：
//   - **后台线程**：start_agent_stats 是 #[tauri::command]，但不能阻塞主线程
//     等所有 JSONL 解析完；扔到 std::thread::spawn 里跑，前端 listen 接事件。
//   - **取消代际**：和 search 用同一套 AtomicU64 模式。新请求 / 显式 cancel_stats
//     都 bump 全局 gen；正在跑的 worker 每处理一个文件 check 一次，过时即 bail。
//   - **进度节奏**：每处理 16 个文件或每 250 ms（看哪个先到）emit 一次 partial
//     快照，避免太频繁的 IPC 抖动。完成时 emit 一次 final done。
//   - **数据源**：SessionSource::read_turns(path) 走的是与 read_session 不同的轻量
//     解析路径——只抽 model / usage / tools / bash / mcp，不构造 UI Block。
//   - **scope**：'all' = claude + codex + gemini 全部聚合；'claude' / 'codex' /
//     'gemini' = 单 agent；'session:<path>:<agent>' = 单个 session（per-session
//     统计页面用）。
//   - **range**：'today' / 'days7' / 'days30' / 'all'。按文件 mtime 过滤；'today'
//     用本地日，其余按 24h 滚动窗口。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

use crate::agents;
use crate::stats::aggregate::{Aggregator, SessionFeed};
use crate::types::{StatsDone, StatsError, StatsProgress};

/// 单调代际。每次 start_agent_stats / cancel_stats 都 bump 一次；老的 worker
/// 看到 gen ≠ 自己的 request_id 立即 bail。
static STATS_GEN: AtomicU64 = AtomicU64::new(0);

/// 立刻取消任何在跑的统计 worker。bump 一次 gen 即可，worker 自己探到差异退出。
pub fn cancel() {
    STATS_GEN.fetch_add(1, Ordering::SeqCst);
}

/// 让 #[tauri::command] start_agent_stats / start_session_stats 调用。
/// 函数立即返回；后续工作在后台线程里跑，结果通过 `stats://progress` /
/// `stats://done` / `stats://error` 事件 emit 给前端。
pub fn start(app: AppHandle, scope: String, range: String, request_id: u64) {
    // 注册本次请求：让旧的 worker 立刻让位
    STATS_GEN.store(request_id, Ordering::SeqCst);

    thread::spawn(move || {
        let result = run_worker(&app, &scope, &range, request_id);
        if request_id != STATS_GEN.load(Ordering::SeqCst) {
            // 这一轮在跑过程中被取消（新请求 / cancel）—— 沉默退出，不 emit。
            return;
        }
        match result {
            Ok(final_stats) => {
                let _ = app.emit(
                    "stats://done",
                    StatsDone {
                        request_id,
                        stats: final_stats,
                    },
                );
            }
            Err(e) => {
                let _ = app.emit(
                    "stats://error",
                    StatsError {
                        request_id,
                        error: e,
                    },
                );
            }
        }
    });
}

/// 单次扫描：把所有匹配 scope/range 的 (project, session, turns) 喂进 Aggregator。
/// 进度节奏：每 16 个文件 或 每 250 ms emit 一次 partial。
fn run_worker(
    app: &AppHandle,
    scope: &str,
    range: &str,
    request_id: u64,
) -> Result<crate::types::AgentStats, String> {
    let agents_to_scan: Vec<&'static str> = match scope {
        "all" => vec!["claude", "codex", "gemini"],
        "claude" => vec!["claude"],
        "codex" => vec!["codex"],
        "gemini" => vec!["gemini"],
        other => {
            // session 模式：scope = "session:<agent>:<path>"
            if let Some(rest) = other.strip_prefix("session:") {
                return run_session_scope(app, rest, request_id);
            }
            return Err(format!("unknown stats scope: {other}"));
        }
    };

    // 时间窗口（毫秒 unix）。返回 (lo, hi) —— hi 为 None 时 = 无上限。
    let (lo_ms, hi_ms) = parse_range(range)?;

    // 1) 先把所有要扫的 (agent, session_meta) 收集起来，得到 total。
    //    用 list_sessions(.., 0, usize::MAX) 拿到全量元数据 —— 这一步本身很轻
    //    （只读文件 mtime，不解析）。
    struct Pending {
        agent_name: &'static str,
        project_dir_name: String,
        project_display: String,
        session: crate::types::SessionMeta,
    }
    let mut pending: Vec<Pending> = Vec::new();

    for agent_name in &agents_to_scan {
        if request_id != STATS_GEN.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        let src = match agents::source(agent_name) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let projects = match src.list_projects(false, false) {
            Ok(p) => p,
            Err(_) => continue,
        };
        for p in projects {
            if request_id != STATS_GEN.load(Ordering::SeqCst) {
                return Err("cancelled".into());
            }
            // 用 discover_stats_sessions 而不是 list_sessions —— 前者会带上
            // Claude 的 `<sessionId>/subagents/*.jsonl`（独立计费的子代理 JSONL），
            // 否则统计会少掉一整块（cost / 调用数 / 模型分布都被低估）。
            let sessions = match src.discover_stats_sessions(&p.dir_name) {
                Ok(s) => s,
                Err(_) => continue,
            };
            for s in sessions {
                if !in_window(s.modified, lo_ms, hi_ms) {
                    continue;
                }
                pending.push(Pending {
                    agent_name,
                    project_dir_name: p.dir_name.clone(),
                    project_display: p.display_path.clone(),
                    session: s,
                });
            }
        }
    }
    let total = pending.len();

    // 起一个空快照让前端立刻渲染骨架 —— 不必等首个文件解析完。
    let mut agg = Aggregator::new();
    emit_progress(app, request_id, 0, total, &agg, scope);

    let mut processed: usize = 0;
    let mut last_emit = Instant::now();
    const EMIT_EVERY_N: usize = 16;
    const EMIT_EVERY: Duration = Duration::from_millis(250);

    for pp in pending {
        if request_id != STATS_GEN.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        let src = match agents::source(pp.agent_name) {
            Ok(s) => s,
            Err(_) => {
                processed += 1;
                continue;
            }
        };
        // 单个文件坏掉不要整盘挂；空 turns 走聚合器，session_count 仍递增。
        let turns = src.read_turns(&pp.session.path).unwrap_or_default();
        let feed = SessionFeed {
            agent: pp.agent_name,
            project_dir_name: &pp.project_dir_name,
            project_display: &pp.project_display,
            session_id: &pp.session.id,
            path: &pp.session.path,
            title: &pp.session.title,
            last_modified: pp.session.modified,
            message_count: pp.session.message_count,
            turns: &turns,
        };
        agg.feed_session(&feed);
        processed += 1;

        if processed.is_multiple_of(EMIT_EVERY_N) || last_emit.elapsed() >= EMIT_EVERY {
            emit_progress(app, request_id, processed, total, &agg, scope);
            last_emit = Instant::now();
        }
    }
    Ok(agg.snapshot(scope))
}

/// session-scope：只扫一个文件，多次进度对一个文件意义不大，直接一次性 done。
fn run_session_scope(
    app: &AppHandle,
    rest: &str,
    request_id: u64,
) -> Result<crate::types::AgentStats, String> {
    // 拼接形式："<agent>:<path>"；agent 不含 ':' 所以 splitn 2 足够。
    let mut it = rest.splitn(2, ':');
    let agent_name = it.next().ok_or_else(|| "missing agent".to_string())?;
    let path = it.next().ok_or_else(|| "missing path".to_string())?;
    let src = agents::source(agent_name)?;
    // 反查 session meta —— 用 list_sessions 找到这个 path（昂贵但一次性）
    // 更高效的做法是 read_turns + scan path 自身的 file_name，但需要先有 project；
    // 这里追求实现简单。
    let projects = src.list_projects(false, false).unwrap_or_default();
    let mut meta: Option<crate::types::SessionMeta> = None;
    let mut project_display = String::new();
    let mut project_dir = String::new();
    'outer: for p in projects {
        if let Ok(page) = src.list_sessions(&p.dir_name, 0, usize::MAX, false, false) {
            for s in page.sessions {
                if s.path == path {
                    project_display = p.display_path.clone();
                    project_dir = p.dir_name.clone();
                    meta = Some(s);
                    break 'outer;
                }
            }
        }
    }
    let meta = meta.ok_or_else(|| format!("session not found: {path}"))?;
    let scope_label = format!("session:{agent_name}");

    let mut agg = Aggregator::new();
    emit_progress(app, request_id, 0, 1, &agg, &scope_label);
    let turns = src.read_turns(path).unwrap_or_default();
    agg.feed_session(&SessionFeed {
        agent: agent_name,
        project_dir_name: &project_dir,
        project_display: &project_display,
        session_id: &meta.id,
        path: &meta.path,
        title: &meta.title,
        last_modified: meta.modified,
        message_count: meta.message_count,
        turns: &turns,
    });
    emit_progress(app, request_id, 1, 1, &agg, &scope_label);
    Ok(agg.snapshot(&scope_label))
}

fn emit_progress(
    app: &AppHandle,
    request_id: u64,
    processed: usize,
    total: usize,
    agg: &Aggregator,
    scope: &str,
) {
    let partial = agg.snapshot(scope);
    let _ = app.emit(
        "stats://progress",
        StatsProgress {
            request_id,
            processed,
            total,
            partial,
        },
    );
}

fn parse_range(range: &str) -> Result<(Option<u64>, Option<u64>), String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let day_ms: u64 = 86_400_000;
    match range {
        "all" => Ok((None, None)),
        // 滚动 24h 窗口：足够用，避免引入本地时区
        "today" => Ok((Some(now.saturating_sub(day_ms)), None)),
        "days7" => Ok((Some(now.saturating_sub(7 * day_ms)), None)),
        "days30" => Ok((Some(now.saturating_sub(30 * day_ms)), None)),
        _ => Err(format!("unknown stats range: {range}")),
    }
}

fn in_window(mtime: u64, lo: Option<u64>, hi: Option<u64>) -> bool {
    if let Some(l) = lo {
        if mtime < l {
            return false;
        }
    }
    if let Some(h) = hi {
        if mtime > h {
            return false;
        }
    }
    true
}

// ============================ 锁包装 ============================
// in-progress 请求标识，给 cancel 用。Mutex 而非 Atomic：要存 String scope/range
// 不只是 u64。当前没人读它，仅记一份请求元数据备查。
pub struct InProgress {
    pub scope: String,
    pub range: String,
}
pub struct InProgressLock(Mutex<Option<InProgress>>);
impl InProgressLock {
    pub fn new() -> Self {
        Self(Mutex::new(None))
    }
}
impl Default for InProgressLock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_range_handles_known_values() {
        assert_eq!(parse_range("all").unwrap(), (None, None));
        let (lo, hi) = parse_range("days7").unwrap();
        assert!(lo.is_some());
        assert!(hi.is_none());
        assert!(parse_range("nope").is_err());
    }

    #[test]
    fn in_window_uses_lo_correctly() {
        assert!(in_window(100, Some(50), None));
        assert!(!in_window(10, Some(50), None));
        assert!(in_window(100, None, None));
    }
}
