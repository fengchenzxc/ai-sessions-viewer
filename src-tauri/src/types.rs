// 前端 & 各 agent 模块共享的可序列化类型。
// 这里只放数据形状定义，所有字段都 `pub`，方便各 agent 实现直接构造。
// 字段命名规则：Rust snake_case → JS camelCase（serde 全局 rename_all）。
//
// `#[allow(dead_code)]`：流式统计模块（stats/stream.rs）尚未接入，
// `StatsProgress` / `StatsDone` / `StatsError` / `TimeRange` 等只在
// 下一批改动里被消费。允许暂时未使用，避免 clippy 报错阻塞构建。

#![allow(dead_code)]

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
    pub codex_app_list_rank: Option<usize>,
    pub codex_app_list_scanned: usize,
    pub codex_app_first_page_size: usize,
    pub codex_app_first_page_position: usize,
    pub codex_internal: bool,
    pub codex_archived: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPage {
    /// 该项目会话总数（用于前端判断是否还有下一页）。
    pub total: usize,
    pub sessions: Vec<SessionMeta>,
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: String, // ctx | add | del
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Serialize, Default, Clone)]
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

#[derive(Serialize, Clone)]
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

/// 全局搜索的命中条目 —— 包含足以「打开这条会话 + 滚到那条消息」的所有上下文。
/// `matched_field` 是字符串而非枚举，方便前端按 i18n key 直接拼一行说明。
/// `snippet` 是命中文本周围一小段（约 120 字符）；前端再按关键词高亮。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// 命中所属项目，给前端「先 selectProject 再 openSession」的跳转用。
    pub project_key: String,
    pub project_display: String,
    pub session: SessionMeta,
    /// "title" | "id" | "path" | "text"
    pub matched_field: String,
    /// 命中片段；title/id/path 上等于原值，text 上是带前后文的一小段。
    pub snippet: String,
    /// 文本命中所在消息的索引（在 read_session 返回的 Msg 数组里）。
    /// metadata 命中（title/id/path）时为 None —— 这种情况只需打开会话，不需要滚动。
    pub match_msg_index: Option<usize>,
    /// 文本命中所在消息的 uuid（若该 agent 写了 uuid）。和 index 同源；前端优先用 uuid，
    /// 万一从打开会话到滚动之间消息数组发生重排，uuid 能比 index 更稳。
    pub match_msg_uuid: Option<String>,
}

/// 一个会话的 token 用量汇总。三个 agent 用的字段名各不相同，这里统一抽象：
///   - `input_tokens` / `output_tokens` —— 新鲜进 / 出的 token
///   - `cache_creation_input_tokens` —— 写入缓存（仅 Claude 有这个概念，含 5min + 1h 两档）
///   - `cache_creation_1h_input_tokens` —— 上面那个之中属于 1-hour tier 的子集。
///     Anthropic 1h cache write 单价 = 5min 的 2×，所以 cost 公式要单独再加一遍；
///     这个字段是 `cache_creation_input_tokens` 的子集，不要双计 token 数（只在 cost 上加）。
///   - `cache_read_input_tokens` —— 从缓存读（Claude / Codex 都用，字段名不同）
///   - `reasoning_output_tokens` —— 推理 token（仅 Codex / 部分模型）
///   - `total` —— 五项之和；前端通常只展示这一项，hover 展开看细分。
///     `cache_creation_1h_input_tokens` **不** 进 total，因为它已经被 `cache_creation_input_tokens` 包含。
///
/// 任一字段缺失（agent 没记 / 该轮没产生）记 0，结构永远完整，不出 Optional。
#[derive(Serialize, Default, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_creation_1h_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub total: u64,
}

impl UsageSummary {
    /// 把 total 字段算上五项之和；构造完直接 `.finalize()` 一下即可，避免调用方各自累加。
    /// `cache_creation_1h_input_tokens` 是 `cache_creation_input_tokens` 的子集，不进 total。
    pub fn finalize(mut self) -> Self {
        self.total = self.input_tokens
            + self.output_tokens
            + self.cache_creation_input_tokens
            + self.cache_read_input_tokens
            + self.reasoning_output_tokens;
        self
    }

    /// 累加另一个 UsageSummary 进来；total 自动重算。聚合统计用。
    pub fn add_assign(&mut self, other: &UsageSummary) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
        self.cache_creation_1h_input_tokens += other.cache_creation_1h_input_tokens;
        self.cache_read_input_tokens += other.cache_read_input_tokens;
        self.reasoning_output_tokens += other.reasoning_output_tokens;
        self.total += other.total;
    }
}

// ============================ 统计 dashboard 用的聚合类型 ============================
// `agent_stats` 命令一次性算齐当前 agent 的所有项目 + 会话，返回这一坨。
// 数据量不大（一个用户最多大概几千个会话），一次性 IPC 传过去比前端逐项 fetch 划算。

/// 某个项目（dirName 级别）的统计聚合：会话数、消息数、token 用量、cost、最后活跃时间。
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStats {
    pub dir_name: String,
    pub display_path: String,
    pub session_count: usize,
    pub message_count: usize,
    pub call_count: u64,
    pub usage: UsageSummary,
    pub cost_usd: f64,
    pub last_modified: u64,
}

/// 某一天（UTC YYYY-MM-DD）的活动量。前端按这串数据画热图 + 时间线图。
/// 用 UTC 是为了不引 chrono 维护本地时区；对国内用户最多差 8h，可接受。
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DailyActivity {
    pub date: String,
    pub session_count: usize,
    pub message_count: usize,
    pub call_count: u64,
    pub tokens: u64,
    pub cost_usd: f64,
}

/// Top Sessions 排行里的一条 —— 一次"贵会话"。
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionStat {
    /// 该会话所属 agent（"claude" / "codex" / "gemini"），跨 agent 聚合时区分用。
    pub agent: String,
    pub session_id: String,
    pub path: String,
    pub project_display: String,
    pub title: String,
    pub last_modified: u64,
    pub call_count: u64,
    pub usage: UsageSummary,
    pub cost_usd: f64,
}

/// By Model 排行里的一条 —— 按模型聚合的 cost / 调用次数 / cache 命中率。
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelStat {
    /// 模型原始名（前端用 short_name 做展示，也保留这个用于 tooltip）。
    pub model: String,
    pub label: String,
    pub call_count: u64,
    pub usage: UsageSummary,
    pub cost_usd: f64,
    /// cache_read / (input + cache_read + cache_creation)。0..=1。
    pub cache_hit_rate: f64,
}

/// By Tool / By Shell / By MCP 通用条目：name + calls。
#[derive(Serialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NamedCount {
    pub name: String,
    pub count: u64,
}

/// By Activity 一行：分类 + 调用次数 + 成本。
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ActivityStat {
    /// 分类 key —— 跟 stats.activity.* 翻译对齐。
    pub key: String,
    pub turn_count: u64,
    pub call_count: u64,
    pub cost_usd: f64,
}

/// 时间范围筛选 —— 前端按按钮切，每次切都触发一次新扫描。
#[derive(Serialize, Default, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TimeRange {
    Today,
    Days7,
    Days30,
    #[default]
    All,
}

/// 流式统计的完整结果。整个 agent 的统计概览：顶层标量 + 各排行 + 日活时间线。
///
/// `cost_usd` 用 USD 计；前端按需展示美元。`days_active` = UTC 日历日。
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentStats {
    /// "all" / "claude" / "codex" / "gemini"。前端按这值给小标题。
    pub scope: String,
    pub session_count: usize,
    pub message_count: usize,
    pub call_count: u64,
    /// 至少出现过一条会话的 UTC 天数。
    pub days_active: usize,
    pub usage: UsageSummary,
    pub cost_usd: f64,
    /// 顶层 cache 命中率（cache_read / (input + cache_read + cache_creation)）。
    pub cache_hit_rate: f64,
    /// 按 cost_usd 降序的项目列表。
    pub projects: Vec<ProjectStats>,
    /// 按日期升序的日活时间线；可能稀疏。
    pub daily_activity: Vec<DailyActivity>,
    /// 按 cost_usd 降序的 Top 10 会话。
    pub top_sessions: Vec<SessionStat>,
    /// 按 cost_usd 降序的模型排行。
    pub by_model: Vec<ModelStat>,
    /// 按调用次数降序的工具排行。
    pub by_tool: Vec<NamedCount>,
    /// 按调用次数降序的 shell 主命令排行（first-token of Bash input）。
    pub by_shell: Vec<NamedCount>,
    /// 按调用次数降序的 MCP server 排行。
    pub by_mcp: Vec<NamedCount>,
    /// 按 cost_usd 降序的活动分类排行。
    pub by_activity: Vec<ActivityStat>,
}

/// 流式推送时的进度负载（事件名：`stats://progress`）。
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatsProgress {
    /// 这次流的标识 —— 前端比对 requestId，过时的进度直接丢弃。
    pub request_id: u64,
    pub processed: usize,
    pub total: usize,
    /// 增量快照：到目前为止已处理文件聚合出的 AgentStats。前端可以直接替换 ref。
    pub partial: AgentStats,
}

/// 流式推送完成时的最终负载（事件名：`stats://done`）。
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatsDone {
    pub request_id: u64,
    pub stats: AgentStats,
}

/// 流式推送出错时的负载（事件名：`stats://error`）。
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatsError {
    pub request_id: u64,
    pub error: String,
}
