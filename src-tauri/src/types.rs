// 前端 & 各 agent 模块共享的可序列化类型。
// 这里只放数据形状定义，所有字段都 `pub`，方便各 agent 实现直接构造。
// 字段命名规则：Rust snake_case → JS camelCase（serde 全局 rename_all）。

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    /// 项目标识：Claude 为目录名，Codex 为 cwd 路径。
    pub dir_name: String,
    pub display_path: String,
    pub session_count: usize,
    pub last_modified: u64,
    /// 项目目录当前是否仍存在于磁盘上。
    pub exists: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub id: String,
    pub file_name: String,
    pub path: String,
    pub title: String,
    pub cwd: Option<String>,
    pub created: Option<String>,
    pub modified: u64,
    pub size: u64,
    pub message_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPage {
    /// 该项目会话总数（用于前端判断是否还有下一页）。
    pub total: usize,
    pub sessions: Vec<SessionMeta>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: String, // ctx | add | del
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub kind: String, // text | thinking | tool_use | tool_result | image
    pub text: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<String>,
    pub tool_id: Option<String>,
    pub is_error: bool,
    /// 文件改动类工具结果携带的目标文件路径。
    pub file_path: Option<String>,
    /// 文件改动的结构化 diff（如 Claude 的 structuredPatch）。
    pub diff: Option<Vec<DiffHunk>>,
    /// 图片源：通常为 data:<mime>;base64,<...> 的内联 URL 或 http(s) URL。
    pub image_src: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Msg {
    pub uuid: Option<String>,
    pub role: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub sidechain: bool,
    pub blocks: Vec<Block>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashItem {
    pub trash_file: String,
    pub agent: String,
    pub project_label: String,
    pub original_path: String,
    /// 回收站里 JSONL 的绝对路径，供「在回收站里直接查看会话详情」读取。
    pub trash_path: String,
    pub deleted_at: u64,
    pub title: String,
    pub size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub has_update: bool,
}
