// 实时 tail：监听打开会话所在 JSONL 文件的写入事件。
//
// 设计：
//   - 单订阅模型 —— 同一时刻只追一个文件（当前 ChatView 打开的那条）。
//     watch_session(agent, path) 替换上一个 watcher；unwatch_session() 清空。
//   - 触发：notify 派来 Modify / Create 事件后，debounce 一小段（避免 IDE / agent
//     频繁追加 1 行就 emit 一次），再走一次"整文件 read_session"，把新增的 Msg
//     切片 emit 给前端。
//   - 整文件 re-parse 的代价：Claude 的解析器有跨行状态（queued user 消息缓冲、
//     工具结果配对等），增量解析需要重写解析器；MVP 选择"整文件再读一次 +
//     基于 Msg 数量取尾巴"，简单、可读、足够快（实测十几 MB 会话 < 50 ms）。
//   - 文件截断 / 删除：emit `session:reset`（前端整文件重拉）或 `session:gone`。
//
// 前端事件契约：
//   session:append   { path, messages: Msg[] }    新增的尾段
//   session:reset    { path }                      文件被截断或替换 → 整文件重拉
//   session:gone     { path }                      文件不再存在
//
// 这一层不缓存 mtime —— 文件系统事件本身就是触发源，不需要轮询。

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::agents;
use crate::types::Msg;

/// 内部状态：当前 watcher + 上次已 emit 的 Msg 数量 + 文件路径 / agent。
struct WatchState {
    /// 让 watcher 活着 —— drop 后回调停。Option 方便 swap 时先取走。
    _watcher: RecommendedWatcher,
    /// 监听中的 JSONL 绝对路径。供 #[cfg(test)] current_path() 用。
    #[allow(dead_code)]
    path: PathBuf,
    /// 对应 agent 名。回调里有自己的 clone；这里留作调试/排错时一眼能看到。
    #[allow(dead_code)]
    agent: String,
}

/// 全局单订阅槽。Mutex 保护：watch / unwatch 串行；notify 回调线程也走这把锁
/// 取上下文。锁的临界区只做"读 last_count → 解析整文件 → 写 last_count → emit"，
/// 远比文件解析快，竞争忽略不计。
static STATE: OnceLock<Mutex<Option<WatchState>>> = OnceLock::new();

/// 每个文件路径独立维护"上次 emit 的 Msg 数量"。即使 watch 被换了又换回来，
/// 仍能用这个 cache 接上上次的进度，避免误把整段当 append。
/// key = 绝对路径串；value = (last_msg_count, last_modify_instant)
static LAST_COUNT: OnceLock<Mutex<std::collections::HashMap<String, (usize, Instant)>>> =
    OnceLock::new();

fn state() -> &'static Mutex<Option<WatchState>> {
    STATE.get_or_init(|| Mutex::new(None))
}

fn last_count_map() -> &'static Mutex<std::collections::HashMap<String, (usize, Instant)>> {
    LAST_COUNT.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// debounce 窗口：notify 一次写入可能拆成多条事件，攒一拨再 emit。
/// 200ms 平衡：人类感知接近实时（<300ms 觉得是即时），又能压平 IDE / agent 的多次
/// 小写入。
const DEBOUNCE_MS: u64 = 200;

#[derive(Serialize, Clone)]
struct AppendPayload {
    path: String,
    messages: Vec<Msg>,
}

#[derive(Serialize, Clone)]
struct PathPayload {
    path: String,
}

/// 启动监听。如果已有别的 watcher，先停掉再起新的。
/// 不存在的路径返回错误；前端可以选择降级到不 tail。
pub fn watch_session(app: AppHandle, agent: String, path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("文件不存在: {path}"));
    }
    // 先释放旧 watcher（drop = stop）。
    {
        let mut guard = state().lock().map_err(|e| e.to_string())?;
        *guard = None;
    }

    // 初始化 last_count：把当前文件的 Msg 数量记下来，后续只 emit 增量。
    let src = agents::source(&agent)?;
    let initial = src.read_session(&path).unwrap_or_default();
    {
        let mut m = last_count_map().lock().map_err(|e| e.to_string())?;
        m.insert(path.clone(), (initial.len(), Instant::now()));
    }

    let app_handle = app.clone();
    let agent_for_cb = agent.clone();
    let path_for_cb = path.clone();
    let mut watcher: RecommendedWatcher =
        notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(ev) = res else { return };
            if !matches!(
                ev.kind,
                EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
            ) {
                return;
            }
            // 删除事件：立刻 emit gone，不再 process append。
            if matches!(ev.kind, EventKind::Remove(_)) {
                let _ = app_handle.emit(
                    "session:gone",
                    PathPayload {
                        path: path_for_cb.clone(),
                    },
                );
                return;
            }
            process_change(&app_handle, &agent_for_cb, &path_for_cb);
        })
        .map_err(|e| format!("notify init 失败: {e}"))?;

    // 仅监听这个具体文件 —— notify 内部会按平台 fallback 监听父目录后再过滤。
    watcher
        .watch(&p, RecursiveMode::NonRecursive)
        .map_err(|e| format!("watch 失败: {e}"))?;

    {
        let mut guard = state().lock().map_err(|e| e.to_string())?;
        *guard = Some(WatchState {
            _watcher: watcher,
            path: p,
            agent,
        });
    }
    Ok(())
}

/// 停止监听。空操作可重入 —— 前端 unmount 调用很安全。
pub fn unwatch_session() -> Result<(), String> {
    let mut guard = state().lock().map_err(|e| e.to_string())?;
    *guard = None;
    Ok(())
}

/// 单次文件变更处理：整文件重解析 → 跟上次 emit 的数量比 → emit 尾段或 reset。
fn process_change(app: &AppHandle, agent: &str, path: &str) {
    // debounce：如果上一次处理距现在不到 DEBOUNCE_MS，跳过 —— 后续事件会再触发一次。
    {
        let mut m = match last_count_map().lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if let Some((_, ts)) = m.get(path) {
            if ts.elapsed() < Duration::from_millis(DEBOUNCE_MS) {
                return;
            }
        }
        // 占位更新 ts，避免并发回调挤进来。真正的 count 在解析后再写。
        let prev_count = m.get(path).map(|(c, _)| *c).unwrap_or(0);
        m.insert(path.to_string(), (prev_count, Instant::now()));
    }

    let src = match agents::source(agent) {
        Ok(s) => s,
        Err(_) => return,
    };
    let msgs = match src.read_session(path) {
        Ok(m) => m,
        Err(_) => return,
    };

    let prev_count = {
        let m = match last_count_map().lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        m.get(path).map(|(c, _)| *c).unwrap_or(0)
    };

    if msgs.len() < prev_count {
        // 文件被截断 / 替换 → 让前端整段重拉。
        let _ = app.emit(
            "session:reset",
            PathPayload {
                path: path.to_string(),
            },
        );
        let mut m = match last_count_map().lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        m.insert(path.to_string(), (msgs.len(), Instant::now()));
        return;
    }

    if msgs.len() == prev_count {
        return;
    }

    // 真有新增 —— 切尾 emit。
    let tail = msgs[prev_count..].to_vec();
    let _ = app.emit(
        "session:append",
        AppendPayload {
            path: path.to_string(),
            messages: tail,
        },
    );
    let mut m = match last_count_map().lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    m.insert(path.to_string(), (msgs.len(), Instant::now()));
}

/// 测试用：当前是否有活跃 watch。
#[cfg(test)]
pub fn is_watching() -> bool {
    state().lock().map(|g| g.is_some()).unwrap_or(false)
}

/// 测试用：当前 watch 的路径（如果有）。
#[cfg(test)]
pub fn current_path() -> Option<String> {
    state()
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.path.to_string_lossy().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 没起过 watcher 时 is_watching 必须是 false；unwatch 永远是 Ok。
    /// 注意：unit test 共用进程，OnceLock 状态跨测试持续，所以这条要先 unwatch 一次清场。
    #[test]
    fn unwatch_is_idempotent_and_state_starts_empty() {
        let _ = unwatch_session();
        assert!(!is_watching());
        assert!(current_path().is_none());
        // 再次 unwatch 仍 Ok，不会 panic
        assert!(unwatch_session().is_ok());
    }

    /// last_count_map 的 entry 是按 path 隔离的；不同 path 互不污染。
    /// 这条直接走内部 map，避开 notify watcher（需要真实文件 + AppHandle）。
    #[test]
    fn last_count_map_is_keyed_per_path() {
        let m = last_count_map();
        {
            let mut g = m.lock().unwrap();
            g.insert("/tmp/a.jsonl".into(), (3, Instant::now()));
            g.insert("/tmp/b.jsonl".into(), (7, Instant::now()));
        }
        let g = m.lock().unwrap();
        assert_eq!(g.get("/tmp/a.jsonl").map(|(c, _)| *c), Some(3));
        assert_eq!(g.get("/tmp/b.jsonl").map(|(c, _)| *c), Some(7));
    }
}
