// AI 会话管理器 —— 后端入口。
//
// 这个文件只做两件事：
//   1. 注册 Tauri 命令，把请求路由到对应模块（`agents` / `trash`）。
//   2. macOS 启动期 setup（unifiedCompact 标题栏）。
//
// 所有 agent 相关的解析、读写、重命名逻辑都在 `agents/*.rs` 里；
// 回收站逻辑在 `trash.rs`；跨模块共用的小工具在 `util.rs`；
// 跟前端共享的序列化类型在 `types.rs`。
// 接入新 agent 的步骤详见 `agents/mod.rs` 顶部注释。

// agents / stats are `pub` so the `examples/test_dedup.rs` binary (compiled as
// an external consumer of the lib crate) can call into the dedup pipeline
// directly. Everything else stays crate-private.
pub mod agents;
mod menu;
pub mod stats;
mod trash;
mod types;
mod util;
mod watch;

use std::fs;
use std::path::{Path, PathBuf};

use crate::types::{AgentStats, Msg, ProjectInfo, SearchHit, SessionPage, TrashItem, UsageSummary};
use crate::util::is_jsonl;

/// 全局搜索的取消代际 —— 每次新搜索把自己的 `request_id` 写进来，正在跑的搜索循环
/// 不停 check；一旦发现 gen ≠ 自己的 id 就主动 bail。`cancel_search()` 直接 bump 它。
static SEARCH_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// ============================ Tauri 命令：分派层 ============================

#[tauri::command]
fn list_projects(
    agent: String,
    include_codex_internal: bool,
    include_codex_archived: bool,
) -> Result<Vec<ProjectInfo>, String> {
    agents::source(&agent)?.list_projects(include_codex_internal, include_codex_archived)
}

#[tauri::command]
fn list_sessions(
    agent: String,
    project_key: String,
    offset: usize,
    limit: usize,
    include_codex_internal: bool,
    include_codex_archived: bool,
) -> Result<SessionPage, String> {
    agents::source(&agent)?.list_sessions(
        &project_key,
        offset,
        limit,
        include_codex_internal,
        include_codex_archived,
    )
}

#[tauri::command]
fn read_session(agent: String, path: String) -> Result<Vec<Msg>, String> {
    agents::source(&agent)?.read_session(&path)
}

/// 实时 tail：开始监听 path 文件的写入事件。
/// 同一时刻只允许一个 watch；再次调用会替换上一个 watcher。
/// 文件不存在返回 Err，前端可以静默降级（仅一次性读取）。
#[tauri::command]
fn watch_session(app: tauri::AppHandle, agent: String, path: String) -> Result<(), String> {
    watch::watch_session(app, agent, path)
}

/// 停止当前 tail；空操作可重入。前端 unmount / 切会话时调用。
#[tauri::command]
fn unwatch_session() -> Result<(), String> {
    watch::unwatch_session()
}

/// 单个会话的 token 用量汇总（按 path + mtime 缓存）。
/// 前端 ChatTopbar / SessionsView 卡片懒加载这条；Gemini 暂占位返零。
#[tauri::command]
fn session_usage(agent: String, path: String) -> Result<UsageSummary, String> {
    let src = agents::source(&agent)?;
    agents::session_usage(&*src, &path)
}

/// 当前 agent 的统计概览：顶层标量 + 项目排行（按 token 降序）+ 日活时间轴。
/// **保留作兼容入口** —— 旧版同步路径仍然可用，但内容比 start_agent_stats 简化（没有
/// cost / by_model / by_tool 等）。前端默认走流式接口，这里只作兜底。
#[tauri::command]
fn agent_stats(agent: String) -> Result<AgentStats, String> {
    let src = agents::source(&agent)?;
    agents::agent_stats(&*src, &agent)
}

/// 流式启动一次统计扫描。函数立刻返回；后台 worker 通过 `stats://progress` /
/// `stats://done` / `stats://error` 三个事件把结果推回前端。新请求会让旧请求让位
/// （`STATS_GEN` 代际计数器）。前端用 `requestId` 比对，丢掉旧数据。
///
/// `scope`：`all` / `claude` / `codex` / `gemini` / `session:<agent>:<absolute path>`。
/// `range`：`today` / `days7` / `days30` / `all`（session-scope 下忽略）。
#[tauri::command]
fn start_agent_stats(app: tauri::AppHandle, scope: String, range: String, request_id: u64) {
    stats::stream::start(app, scope, range, request_id);
}

/// 立刻取消任何正在跑的统计 worker。本质上是把全局代际 +1，跑中的 worker 自己 bail。
#[tauri::command]
fn cancel_stats() {
    stats::stream::cancel();
}

/// 全局搜索：跨当前 agent 的所有项目 / 会话查关键词。
/// 命中范围在 `agents::search` 里：标题 / id / 项目路径 / 文本（仅 text + thinking 块）；
/// 工具调用 / 工具结果 / 文件改动默认不参与匹配。
/// 空字符串返回空数组（避免一次性把所有会话当结果返回）。
///
/// **可取消**：每次调用都会把 `request_id` 写进全局 SEARCH_GEN；之后任何 `cancel_search()`
/// 或更大 id 的 `search_sessions` 都会让旧的搜索循环立刻 bail（返回空数组）。前端的
/// reqSeq 守卫负责丢掉过期结果，所以即使后端返回了一堆结果也不会污染 UI。
#[tauri::command]
fn search_sessions(
    agent: String,
    query: String,
    request_id: u64,
    project_key: Option<String>,
) -> Result<Vec<SearchHit>, String> {
    SEARCH_GEN.store(request_id, std::sync::atomic::Ordering::SeqCst);
    let src = agents::source(&agent)?;
    let cancel = agents::Cancel {
        request_id,
        gen: &SEARCH_GEN,
    };
    agents::search(&*src, &query, project_key.as_deref(), cancel)
}

/// 显式取消正在跑的全局搜索 —— 前端每次新输入立即调一次，让 CPU 让位给打字。
/// 仅仅 bump 一下 SEARCH_GEN —— 在跑的 search 循环下次 check 时就会 bail。
#[tauri::command]
fn cancel_search() {
    SEARCH_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// 重命名会话：与 Claude Code `/rename` / Codex 内部重命名一致，
/// 在原 JSONL 末尾追加一条官方 schema 的元数据行（append-only），
/// 后续扫描时取最后一条 `custom-title` / `thread_name_updated` 作为标题。
/// 各 agent 还可能写额外的旁路文件（codex 会同步更新 session_index.jsonl / state_<N>.sqlite）。
#[tauri::command]
fn rename_session(agent: String, path: String, name: String) -> Result<(), String> {
    let fp = PathBuf::from(&path);
    if !fp.exists() {
        return Err("会话文件不存在".to_string());
    }
    if !is_jsonl(&fp) {
        return Err("不是 JSONL 文件".to_string());
    }
    agents::source(&agent)?.rename_session(&fp, &name)
}

#[tauri::command]
fn soft_delete_session(agent: String, path: String, project_label: String) -> Result<(), String> {
    trash::soft_delete(&agent, &path, &project_label)
}

#[tauri::command]
fn list_trash() -> Result<Vec<TrashItem>, String> {
    trash::list()
}

#[tauri::command]
fn restore_session(trash_file: String) -> Result<(), String> {
    trash::restore(&trash_file)
}

#[tauri::command]
fn permanent_delete_trash(trash_file: String) -> Result<(), String> {
    trash::permanent_delete(&trash_file)
}

#[tauri::command]
fn empty_trash() -> Result<(), String> {
    trash::empty()
}

/// 在终端中用对应 CLI 恢复（resume）一个会话。
#[tauri::command]
fn resume_session(
    agent: String,
    session_id: String,
    cwd: String,
    path: String,
    terminal: Option<String>,
) -> Result<(), String> {
    ensure_project_dir(&cwd, "无法恢复")?;
    // id 校验：Claude/Codex 为 UUID，Gemini 为 session-<startTime>-<id8>
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("会话 ID 非法".to_string());
    }
    let cli = agents::source(&agent)?.resume_cli(&session_id, &path);
    spawn_terminal(&cli, &cwd, terminal.as_deref())
}

/// 在终端里为某个项目目录开一个全新会话（不带 --resume）。
#[tauri::command]
fn new_session(agent: String, cwd: String, terminal: Option<String>) -> Result<(), String> {
    ensure_project_dir(&cwd, "无法创建会话")?;
    let cli = agents::source(&agent)?.new_session_cli();
    spawn_terminal(&cli, &cwd, terminal.as_deref())
}

fn ensure_project_dir(cwd: &str, action: &str) -> Result<(), String> {
    if cwd.trim().is_empty() {
        return Err(format!("项目目录为空，{action}"));
    }
    let path = Path::new(cwd);
    if path.exists() {
        if path.is_dir() {
            return Ok(());
        }
        return Err(format!("项目路径不是目录，{action}"));
    }
    fs::create_dir_all(path).map_err(|e| format!("创建项目目录失败: {e}"))
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn create_terminal_script(cwd: &Path, cli: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(cwd).map_err(|e| format!("创建项目目录失败: {e}"))?;

    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (std::process::id() as u64);
    let mut script_path = cwd.join(format!(".tmp{:06x}", seed & 0x00ff_ffff));
    for i in 0..100u64 {
        if !script_path.exists() {
            break;
        }
        script_path = cwd.join(format!(
            ".tmp{:06x}",
            (seed.wrapping_add(i + 1)) & 0x00ff_ffff
        ));
    }

    let cleanup_script = script_path.to_string_lossy().to_string();
    let script = format!(
        r#"#!/bin/zsh
set +e
cleanup_script={cleanup_script}
cleanup() {{
  rm -f -- "$cleanup_script"
}}
trap cleanup EXIT
cd {cwd}
{cli}
status=$?
if [ "$status" -ne 0 ]; then
  echo ""
  echo "command exited with status $status"
  echo "Press Enter to close this window..."
  read _unused
fi
exit "$status"
"#,
        cwd = shell_quote(&cwd.to_string_lossy()),
        cleanup_script = shell_quote(&cleanup_script),
        cli = cli
    );
    fs::write(&script_path, script).map_err(|e| format!("创建终端脚本失败: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("设置终端脚本权限失败: {e}"))?;
    }

    Ok(script_path)
}

struct CommandSpec {
    program: String,
    args: Vec<String>,
}

fn terminal_open_command(terminal: &str, _cwd: &Path, script_path: &Path) -> CommandSpec {
    let script = script_path.to_string_lossy().to_string();
    match terminal.to_ascii_lowercase().as_str() {
        "warp" => CommandSpec {
            program: "open".to_string(),
            args: vec!["-a".to_string(), "Warp".to_string(), script],
        },
        "iterm" | "iterm2" => CommandSpec {
            program: "open".to_string(),
            args: vec!["-a".to_string(), "iTerm".to_string(), script],
        },
        _ => CommandSpec {
            program: "open".to_string(),
            args: vec!["-a".to_string(), "Terminal".to_string(), script],
        },
    }
}

fn delayed_cleanup_command(script_path: &Path) -> CommandSpec {
    CommandSpec {
        program: "sh".to_string(),
        args: vec![
            "-c".to_string(),
            "sleep 20; rm -f -- \"$1\"".to_string(),
            "cleanup".to_string(),
            script_path.to_string_lossy().to_string(),
        ],
    }
}

fn spawn_terminal(cli: &str, cwd: &str, terminal: Option<&str>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let cwd_path = Path::new(cwd);
        let script_path = create_terminal_script(cwd_path, cli)?;
        let spec = terminal_open_command(terminal.unwrap_or("terminal"), cwd_path, &script_path);
        std::process::Command::new(&spec.program)
            .args(&spec.args)
            .spawn()
            .map_err(|e| format!("启动终端失败: {e}"))?;
        let cleanup = delayed_cleanup_command(&script_path);
        let _ = std::process::Command::new(&cleanup.program)
            .args(&cleanup.args)
            .spawn();
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
                .args([
                    "/c",
                    "start",
                    "cmd",
                    "/k",
                    &format!("cd /d \"{}\" && {}", cwd_win, cli),
                ])
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
                    .args([
                        "-e",
                        &format!("bash -c '{}'", shell_cmd.replace('\'', "'\\''")),
                    ])
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

#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 把字符串内容写到用户指定的绝对路径。
///
/// 历史：早期版本叫 save_to_downloads，自动落到 ~/Downloads；现在已经接入
/// tauri-plugin-dialog 的 save 对话框由前端拿到目标路径，所以后端只负责
/// 把字节安全写入指定位置。Tauri WKWebView 不支持 `<a download>`/blob URL，
/// 写盘必须经过 Rust。
#[tauri::command]
fn write_file(path: String, content: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
        }
    }
    fs::write(&p, content).map_err(|e| format!("写入文件失败: {e}"))?;
    Ok(p.to_string_lossy().to_string())
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

/// 在系统默认浏览器中打开一个外部链接。只放行 http/https，避免 url 被
/// 当成本地文件或其它协议处理。
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("仅支持 http(s) 链接".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("打开链接失败: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", &url])
            .spawn()
            .map_err(|e| format!("打开链接失败: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("打开链接失败: {e}"))?;
    }
    Ok(())
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
        let ns_window: Retained<NSWindow> = match Retained::retain(ns_window_ptr.cast::<NSWindow>())
        {
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
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            list_projects,
            list_sessions,
            read_session,
            watch_session,
            unwatch_session,
            session_usage,
            agent_stats,
            start_agent_stats,
            cancel_stats,
            search_sessions,
            cancel_search,
            rename_session,
            soft_delete_session,
            list_trash,
            restore_session,
            permanent_delete_trash,
            empty_trash,
            resume_session,
            new_session,
            reveal_in_finder,
            open_url,
            write_file,
            app_version,
        ])
        .setup(|app| {
            // 原生应用菜单 —— 主要价值在 macOS 顶部菜单栏。
            // Windows / Linux 也会挂菜单，但视觉上不那么重要。
            menu::build(app.handle())?;
            menu::install_bridges(app.handle());

            #[cfg(target_os = "macos")]
            {
                use tauri::Manager;
                if let Some(win) = app.get_webview_window("main") {
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

#[cfg(test)]
mod terminal_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn create_terminal_script_uses_hidden_tmp_file_and_self_deletes() {
        let dir = std::env::temp_dir().join(format!(
            "ai-sessions-viewer-terminal-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let script = create_terminal_script(&dir, "codex resume abc-123").unwrap();
        let name = script.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with(".tmp"));
        assert_eq!(name.len(), 10);

        let content = fs::read_to_string(&script).unwrap();
        assert!(content.contains("cd '/"));
        assert!(content.contains("codex resume abc-123"));
        assert!(!content.contains("rm -f \"$0\""));
        assert!(content.contains(&format!(
            "cleanup_script={}",
            shell_quote(&script.to_string_lossy())
        )));
        assert!(content.contains("rm -f -- \"$cleanup_script\""));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&script).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_command_removes_the_script_path_after_a_short_delay() {
        let script = Path::new("/tmp/.tmpABC123");
        let spec = delayed_cleanup_command(script);
        assert_eq!(spec.program, "sh");
        assert_eq!(
            spec.args,
            vec![
                "-c",
                "sleep 20; rm -f -- \"$1\"",
                "cleanup",
                "/tmp/.tmpABC123"
            ]
        );
    }

    #[test]
    fn warp_open_command_opens_the_executable_script_directly() {
        let spec = terminal_open_command(
            "warp",
            Path::new("/Users/me/My Project"),
            Path::new("/Users/me/My Project/.tmpABC123"),
        );
        assert_eq!(spec.program, "open");
        assert_eq!(spec.args[0], "-a");
        assert_eq!(spec.args[1], "Warp");
        assert_eq!(spec.args[2], "/Users/me/My Project/.tmpABC123");
        assert!(!spec.args.iter().any(|arg| arg.contains("command=")));
    }

    #[test]
    fn terminal_and_iterm_open_script_files_directly() {
        let script = Path::new("/tmp/.tmpABC123");
        let terminal = terminal_open_command("terminal", Path::new("/tmp"), script);
        assert_eq!(terminal.program, "open");
        assert_eq!(terminal.args, vec!["-a", "Terminal", "/tmp/.tmpABC123"]);

        let iterm = terminal_open_command("iterm2", Path::new("/tmp"), script);
        assert_eq!(iterm.program, "open");
        assert_eq!(iterm.args, vec!["-a", "iTerm", "/tmp/.tmpABC123"]);
    }
}
