// 各 agent 的会话源抽象。
//
// 接入新 agent（如 gemini）的步骤：
//   1. 新建 `agents/<name>.rs`，定义一个 unit struct（如 `GeminiSource`），
//      为它实现下面的 `SessionSource` trait（每个方法各自调用 agent 自己的解析逻辑）。
//   2. 在文件末尾 `pub mod <name>;` 声明 module，并在 `source()` 里加一个 match 分支。
//   3. 前端 `types.ts` 的 `Agent` 联合类型里加上 `"<name>"`，sidebar / 切换 UI 自然支持。
//   4. 所有 Tauri 命令（list_projects / list_sessions / read_session / rename /
//      resume / 回收站）会自动通过 trait 分派下去，调用方零改动。
//
// 不要把 agent-specific 的解析细节漏到 lib.rs 或 trash.rs —— 加 agent 应该是
// 一个文件加一个 match 分支，超出这个范围就说明 trait 的抽象出了问题，需要重新设计。

use serde_json::Value;
use std::path::Path;

use crate::types::{Msg, ProjectInfo, SessionPage};

pub mod claude;
pub mod codex;

#[allow(dead_code)] // `name` / `image_src` 暂时只在调试/未来扩展中使用，但保留在 trait 上让 agent 契约完整。
pub trait SessionSource: Send + Sync {
    /// agent 标识，跟前端 `Agent` 联合类型保持一致（"claude" / "codex" / ...）。
    fn name(&self) -> &'static str;

    /// 列出该 agent 下的所有项目（已折叠到磁盘 / cwd 的逻辑各自负责）。
    fn list_projects(&self) -> Result<Vec<ProjectInfo>, String>;

    /// 分页返回某项目下的会话元信息。`project_key` 的含义由 agent 自己决定：
    /// Claude 是项目目录名，Codex 是 cwd 路径。
    fn list_sessions(
        &self,
        project_key: &str,
        offset: usize,
        limit: usize,
    ) -> Result<SessionPage, String>;

    /// 解析一个 JSONL 文件并返回标准 `Msg[]`（前端只认这一个形状）。
    fn read_session(&self, path: &str) -> Result<Vec<Msg>, String>;

    /// 实施重命名：写入合适的元数据行 + 必要的旁路（如 codex 还要更新 session_index / sqlite）。
    /// path 已经被 lib.rs 预校验（存在且是 .jsonl），不必再重复检查。
    fn rename_session(&self, path: &Path, name: &str) -> Result<(), String>;

    /// 回收站标题：用 agent 自己的解析逻辑提取展示名。
    fn trash_title(&self, path: &Path) -> String;

    /// 终端里 resume 一个会话用的 CLI 命令。`session_id` 已经过 [A-Za-z0-9-]+ 校验。
    fn resume_cli(&self, session_id: &str) -> String;

    /// 终端里开一个全新会话用的 CLI 命令（不带 --resume）。
    fn new_session_cli(&self) -> String;

    /// 从单个 content 块中尝试提取图片 src（data:URL 或外链）。
    /// 主要供该 agent 自己的 `read_session` 内部使用，放在 trait 上也方便外部预览图片块。
    fn image_src(&self, block: &Value) -> Option<String>;
}

/// 按 agent 名拿到一个具体的会话源。未知 agent 返回错误，调用方应直接透传给前端。
pub fn source(agent: &str) -> Result<Box<dyn SessionSource>, String> {
    match agent {
        "claude" => Ok(Box::new(claude::ClaudeSource)),
        "codex" => Ok(Box::new(codex::CodexSource)),
        other => Err(format!("未知 agent: {other}")),
    }
}
