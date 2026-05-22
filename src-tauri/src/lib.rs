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

mod agents;
mod trash;
mod types;
mod util;

use std::fs;
use std::path::{Path, PathBuf};

use crate::types::{Msg, ProjectInfo, SessionPage, TrashItem, UpdateInfo};
use crate::util::is_jsonl;

// ============================ Tauri 命令：分派层 ============================

#[tauri::command]
fn list_projects(agent: String) -> Result<Vec<ProjectInfo>, String> {
    agents::source(&agent)?.list_projects()
}

#[tauri::command]
fn list_sessions(
    agent: String,
    project_key: String,
    offset: usize,
    limit: usize,
) -> Result<SessionPage, String> {
    agents::source(&agent)?.list_sessions(&project_key, offset, limit)
}

#[tauri::command]
fn read_session(agent: String, path: String) -> Result<Vec<Msg>, String> {
    agents::source(&agent)?.read_session(&path)
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
fn soft_delete_session(
    agent: String,
    path: String,
    project_label: String,
) -> Result<(), String> {
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
    let cli = agents::source(&agent)?.resume_cli(&session_id);
    spawn_terminal(&cli, &cwd)
}

/// 在终端里为某个项目目录开一个全新会话（不带 --resume）。
#[tauri::command]
fn new_session(agent: String, cwd: String) -> Result<(), String> {
    if !Path::new(&cwd).is_dir() {
        return Err("项目目录已不存在，无法创建会话".to_string());
    }
    let cli = agents::source(&agent)?.new_session_cli();
    spawn_terminal(&cli, &cwd)
}

fn spawn_terminal(cli: &str, cwd: &str) -> Result<(), String> {
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
        .plugin(tauri_plugin_dialog::init())
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
            new_session,
            reveal_in_finder,
            open_url,
            write_file,
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
