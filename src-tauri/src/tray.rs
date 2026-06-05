//! macOS 菜单栏托盘图标 + 菜单（仿 Trae）。
//!
//! 只负责"建图标 + 建菜单"。点击事件不在这里处理 —— 托盘菜单项的点击和应用主
//! 菜单一样，统一走 `menu.rs` 里注册的 `on_menu_event` 桥（emit `menu://action`），
//! 所以这里复用已有的菜单 id（如 `open-settings`），前端 menu router 直接接住。
//! 新增的 `show-window` 由 `menu.rs` 的 handler 直接显示并聚焦主窗口。
#![cfg(target_os = "macos")]

use tauri::menu::{MenuBuilder, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Runtime};

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(
        app,
        "show-window",
        "Show ai-sessions-viewer",
        true,
        None::<&str>,
    )?;
    // 复用主菜单已有的 id —— 前端 menu router 已处理它们（统计页 / 设置弹窗）。
    // 不带快捷键：主菜单已注册 ⌘⇧S，托盘再注册同一加速键会冲突。
    let stats = MenuItem::with_id(app, "open-stats", "Statistics", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "open-settings", "Settings…", true, None::<&str>)?;
    let quit = PredefinedMenuItem::quit(app, Some("Quit"))?;

    let menu = MenuBuilder::new(app)
        .item(&show)
        .separator()
        .item(&stats)
        .item(&settings)
        .separator()
        .item(&quit)
        .build()?;

    // 专用菜单栏图标：单色线条 glyph（放大镜 + 会话行），而非彩色 app 图标。
    // icon_as_template(true) → macOS 按 template 渲染，自动随明暗菜单栏变黑/白。
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))?;
    TrayIconBuilder::new()
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .tooltip("ai-sessions-viewer")
        .build(app)?;

    Ok(())
}
