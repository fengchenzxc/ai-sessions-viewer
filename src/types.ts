export type Agent = 'claude' | 'codex'

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
