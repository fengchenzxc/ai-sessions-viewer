export type Agent = 'claude' | 'codex' | 'gemini'
export type TerminalApp = 'warp' | 'terminal' | 'iterm2'

export interface ProjectInfo {
  dirName: string
  displayPath: string
  sessionCount: number
  lastModified: number
  /** 项目目录当前是否仍存在于磁盘上 */
  exists: boolean
}

export interface SessionMeta {
  id: string
  fileName: string
  path: string
  title: string
  cwd?: string
  created?: string
  modified: number
  size: number
  messageCount: number
  codexAppListRank?: number | null
  codexAppListScanned: number
  codexAppFirstPageSize: number
  codexAppFirstPagePosition: number
  codexInternal: boolean
  codexArchived: boolean
}

export interface SessionPage {
  total: number
  sessions: SessionMeta[]
}

export type BlockKind = 'text' | 'thinking' | 'tool_use' | 'tool_result' | 'image'

export interface DiffLine {
  kind: 'ctx' | 'add' | 'del'
  oldNo: number | null
  newNo: number | null
  text: string
}

export interface DiffHunk {
  oldStart: number
  newStart: number
  lines: DiffLine[]
}

export interface Block {
  kind: BlockKind
  text?: string
  toolName?: string
  toolInput?: string
  toolId?: string
  isError: boolean
  filePath?: string
  diff?: DiffHunk[]
  imageSrc?: string
}

export interface Msg {
  uuid?: string
  role: 'user' | 'assistant'
  timestamp?: string
  model?: string
  sidechain: boolean
  blocks: Block[]
}

/** 全局搜索的命中条目（与 Rust 端 SearchHit 同形）。 */
export type SearchField = 'title' | 'id' | 'path' | 'text'
export interface SearchHit {
  projectKey: string
  projectDisplay: string
  session: SessionMeta
  matchedField: SearchField
  /** 命中片段：title/id/path 等于原值；text 上是带前后文（带省略号）的小段。 */
  snippet: string
  /** 文本命中所在消息的索引（read_session 返回的数组下标）；metadata 命中为 undefined。 */
  matchMsgIndex?: number
  /** 文本命中所在消息的 uuid（若 agent 写了）；前端定位时优先用 uuid 兜底。 */
  matchMsgUuid?: string
}

/** 单个会话的 token 用量；与 Rust 端 UsageSummary 同形。
 *  `cacheCreation1hInputTokens` 是 `cacheCreationInputTokens` 的子集（1-hour tier），
 *  cost 公式额外按 1× 5min 价位再算一遍（合计 2×），别在 UI 上把它加进 total。 */
export interface UsageSummary {
  inputTokens: number
  outputTokens: number
  cacheCreationInputTokens: number
  cacheCreation1hInputTokens: number
  cacheReadInputTokens: number
  reasoningOutputTokens: number
  total: number
}

/** 统计 dashboard：单个项目的聚合（与 Rust ProjectStats 同形）。 */
export interface ProjectStats {
  dirName: string
  displayPath: string
  sessionCount: number
  messageCount: number
  callCount: number
  usage: UsageSummary
  costUsd: number
  lastModified: number
}

/** 统计 dashboard：某一天（UTC）的活动量。 */
export interface DailyActivity {
  date: string // YYYY-MM-DD
  sessionCount: number
  messageCount: number
  callCount: number
  tokens: number
  costUsd: number
}

/** Top Sessions 排行里的一条。 */
export interface SessionStat {
  agent: Agent
  sessionId: string
  path: string
  projectDisplay: string
  title: string
  lastModified: number
  callCount: number
  usage: UsageSummary
  costUsd: number
}

/** By Model 排行里的一条。 */
export interface ModelStat {
  model: string
  label: string
  callCount: number
  usage: UsageSummary
  costUsd: number
  /** 0..=1。cache_read / (input + cache_read + cache_creation)。 */
  cacheHitRate: number
}

/** By Tool / By Shell / By MCP 共用 name+count 对。 */
export interface NamedCount {
  name: string
  count: number
}

/** By Activity 一行：分类 key + 调用 / 成本。`key` 对应 stats.activity.* 翻译。 */
export interface ActivityStat {
  key: string
  turnCount: number
  callCount: number
  costUsd: number
}

/** 统计范围筛选 —— 前端 dropdown 切换。 */
export type StatsScope = 'all' | Agent

/** 时间范围筛选。 */
export type StatsRange = 'today' | 'days7' | 'days30' | 'month' | 'months6'

/** 流式统计的完整结果（与 Rust AgentStats 同形）。`scope` 标识维度。 */
export interface AgentStats {
  scope: 'all' | Agent | string
  sessionCount: number
  messageCount: number
  callCount: number
  daysActive: number
  usage: UsageSummary
  costUsd: number
  cacheHitRate: number
  /** 按 cost_usd 降序的项目列表。 */
  projects: ProjectStats[]
  /** 按日期升序的日活时间轴（稀疏，没活动的天不出现）。 */
  dailyActivity: DailyActivity[]
  /** 按 cost_usd 降序的 Top 10 会话。 */
  topSessions: SessionStat[]
  /** 按 cost_usd 降序的模型排行。 */
  byModel: ModelStat[]
  /** 按调用次数降序的工具排行。 */
  byTool: NamedCount[]
  /** 按调用次数降序的 shell 主命令排行。 */
  byShell: NamedCount[]
  /** 按调用次数降序的 MCP server 排行。 */
  byMcp: NamedCount[]
  /** 按 cost_usd 降序的活动分类排行。 */
  byActivity: ActivityStat[]
}

/** 流式推送的进度负载。`partial` 是到目前为止的累计快照，前端直接替换。 */
export interface StatsProgress {
  requestId: number
  processed: number
  total: number
  partial: AgentStats
}

export interface StatsDone {
  requestId: number
  stats: AgentStats
}

export interface StatsError {
  requestId: number
  error: string
}

export interface TrashItem {
  trashFile: string
  agent: Agent
  projectLabel: string
  originalPath: string
  /** 回收站里 JSONL 的绝对路径，用于在回收站里直接查看会话详情。 */
  trashPath: string
  deletedAt: number
  title: string
  size: number
}
