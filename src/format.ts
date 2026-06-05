// 轻量文本格式化：把会话内容渲染成可读的 HTML（无第三方依赖）。
import { t } from './i18n'

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

const URL_RE = /https?:\/\/[^\s<>&)}\]]+/g

function inline(text: string): string {
  let s = escapeHtml(text)
  s = s.replace(URL_RE, (url) => `<a href="${url}" target="_blank" rel="noopener">${url}</a>`)
  s = s.replace(/`([^`\n]+)`/g, '<code>$1</code>')
  s = s.replace(/\*\*([^*\n]+)\*\*/g, '<strong>$1</strong>')
  s = s.replace(/^###\s+(.+)$/gm, '<h4>$1</h4>')
  s = s.replace(/^##\s+(.+)$/gm, '<h3>$1</h3>')
  s = s.replace(/^#\s+(.+)$/gm, '<h3>$1</h3>')
  s = s.replace(/\n/g, '<br>')
  return s
}

// Claude Code / Codex inject slash-command markup into the user message as
// pseudo-XML: <command-name>/init</command-name>, <command-message>init</…>,
// <command-args>foo bar</…>. Rendering them literally is ugly.
//
// <command-message> is just the slash command name without the leading "/" —
// fully redundant with <command-name>. We drop it. <command-name> and
// <command-args> get re-emitted as inline <code> chips via a placeholder pass
// so the inner text still goes through escapeHtml safely.
const COMMAND_MESSAGE_RE = /\s*<command-message>[\s\S]*?<\/command-message>\s*/g
const COMMAND_TAG_RE = /<(command-(?:name|args))>([\s\S]*?)<\/\1>/g
// Claude Code injects a `<local-command-caveat>…</local-command-caveat>` user
// message right before every shell-output relay (e.g. when the user types `!ls`).
// It's plumbing for the model and pure noise to humans — hide it everywhere.
const LOCAL_COMMAND_CAVEAT_RE = /^\s*<local-command-caveat>[\s\S]*?<\/local-command-caveat>\s*$/

/** True if a user "Me" message is just a Claude Code local-command caveat
 *  (no other text/image/tool content). Such messages should be hidden in
 *  the chat view and skipped in exports. */
export function isCaveatOnlyMsg(m: { role: string; blocks: Array<{ kind: string; text?: string }> }): boolean {
  if (m.role !== 'user') return false
  if (m.blocks.length === 0) return false
  return m.blocks.every(
    (b) => b.kind === 'text' && LOCAL_COMMAND_CAVEAT_RE.test(b.text ?? ''),
  )
}

// Claude Code wraps various app-level facts in <system-reminder> tags inside a
// synthetic user message. The /rename command shows up as:
//   <system-reminder>
//   The user named this session "批量导入". This may indicate the session's focus or intent.
//   </system-reminder>
// Rendering that verbatim looks like a "Me" said an English meta-line. We turn
// it into a centered, localized system-event line instead.
const SYSTEM_REMINDER_RE = /<system-reminder>([\s\S]*?)<\/system-reminder>/
const RENAME_INNER_RE = /The user named this session\s+"([^"]+)"/i

export type SystemEvent = { kind: 'rename'; name: string }

/** Parse a user message into a SystemEvent if it consists solely of a
 *  recognized <system-reminder>. Returns null otherwise. */
export function parseSystemEvent(m: {
  role: string
  blocks: Array<{ kind: string; text?: string }>
}): SystemEvent | null {
  if (m.role !== 'user') return null
  if (m.blocks.length !== 1 || m.blocks[0].kind !== 'text') return null
  const text = (m.blocks[0].text ?? '').trim()
  const sr = SYSTEM_REMINDER_RE.exec(text)
  if (!sr) return null
  // The whole message must be just the reminder — no other prose around it.
  if (text.replace(SYSTEM_REMINDER_RE, '').trim() !== '') return null
  const rn = RENAME_INNER_RE.exec(sr[1])
  if (rn) return { kind: 'rename', name: rn[1] }
  return null
}
function extractCommandTags(raw: string): { text: string; codes: string[] } {
  const codes: string[] = []
  const stripped = raw.replace(COMMAND_MESSAGE_RE, '')
  const text = stripped.replace(COMMAND_TAG_RE, (_m, _tag, inner) => {
    // 无参 slash 命令（`/clear` / `/init` 等）会带一个空的 <command-args></…>。
    // 留着就会渲染出一个只有 padding+背景的空 chip —— 像个小色块挂在命令后面。
    // inner 是空 / 全空白时直接吞掉整个标签。
    if (!inner.trim()) return ''
    const idx = codes.push(inner) - 1
    return `CMD${idx}`
  })
  return { text, codes }
}

// ─── GFM-lite 表格 ──────────────────────────────────────────────────
// 检测 markdown table 并渲染成 <table>。之前用户反馈："table 渲染出来是
// `| 路由 | 路径 | 文件 |\n|---|---|---|` 一坨原始字符 + inline code 把每个
// `|` 单元包成小灰块" —— 完全不能读。这里加最小可用版：
//   - 表头行：`| col | col |`（前后 `|` 可省）
//   - 分隔行：`|---|---|`（可带 `:` 做对齐）
//   - 表体行：跟表头同形态
// 单元格内容仍走 inline()，所以行内强调 / inline code / 链接照常生效。
// 转义 `\|` 不处理（罕见，遇到再加）。
const TABLE_SEP_CELL_RE = /^\s*:?-{3,}:?\s*$/

function isTableSeparator(line: string): boolean {
  const cells = line.trim().replace(/^\||\|$/g, '').split('|')
  if (cells.length < 1) return false
  return cells.every((c) => TABLE_SEP_CELL_RE.test(c))
}

function splitTableRow(line: string): string[] {
  return line.trim().replace(/^\||\|$/g, '').split('|').map((c) => c.trim())
}

type CellAlign = 'left' | 'center' | 'right' | null
function getAlignments(separator: string): CellAlign[] {
  const cells = separator.trim().replace(/^\||\|$/g, '').split('|')
  return cells.map((c) => {
    const tt = c.trim()
    const l = tt.startsWith(':')
    const r = tt.endsWith(':')
    if (l && r) return 'center'
    if (r) return 'right'
    if (l) return 'left'
    return null
  })
}

function renderTableHtml(
  headerCells: string[],
  alignments: CellAlign[],
  bodyRows: string[][],
): string {
  const cell = (tag: 'th' | 'td', text: string, idx: number) => {
    const a = alignments[idx]
    const style = a ? ` style="text-align:${a}"` : ''
    return `<${tag}${style}>${inline(text)}</${tag}>`
  }
  const head = '<tr>' + headerCells.map((c, i) => cell('th', c, i)).join('') + '</tr>'
  const body = bodyRows
    .map((row) => '<tr>' + row.map((c, i) => cell('td', c, i)).join('') + '</tr>')
    .join('')
  // 外面套一层 .md-table-wrap 提供 overflow-x —— 列多 / 单元格内容长时
  // 整张表才能横向滚动；不套的话 table 要么撑爆父容器要么 cells 被挤换行。
  return `<div class="md-table-wrap"><table class="md-table"><thead>${head}</thead><tbody>${body}</tbody></table></div>`
}

type MdSegment = { kind: 'table'; html: string } | { kind: 'text'; text: string }

/** 把一段非代码块文本按 markdown table 切片。非 table 部分保留原换行，
 *  之后交由 inline() 处理。 */
function extractTables(text: string): MdSegment[] {
  const lines = text.split('\n')
  const segs: MdSegment[] = []
  let buf: string[] = []
  const flushBuf = () => {
    if (!buf.length) return
    segs.push({ kind: 'text', text: buf.join('\n') })
    buf = []
  }
  let i = 0
  while (i < lines.length) {
    const line = lines[i]
    // 起点：当前行像数据行（含 `|`）+ 下一行是分隔行（dashes/colons）
    if (
      line.trim().includes('|') &&
      i + 1 < lines.length &&
      isTableSeparator(lines[i + 1])
    ) {
      flushBuf()
      const headerCells = splitTableRow(line)
      const alignments = getAlignments(lines[i + 1])
      const bodyRows: string[][] = []
      let j = i + 2
      while (j < lines.length && lines[j].trim() !== '' && lines[j].trim().includes('|')) {
        bodyRows.push(splitTableRow(lines[j]))
        j++
      }
      segs.push({ kind: 'table', html: renderTableHtml(headerCells, alignments, bodyRows) })
      i = j
      continue
    }
    buf.push(line)
    i++
  }
  flushBuf()
  return segs
}

/** 渲染 Markdown 子集：围栏代码块 + 行内强调 + GFM table。 */
export function renderText(raw: string): string {
  const { text: pre, codes } = extractCommandTags(raw)
  const parts = pre.split('```')
  let html = ''
  parts.forEach((part, i) => {
    if (i % 2 === 1) {
      const nl = part.indexOf('\n')
      const lang = nl >= 0 ? part.slice(0, nl).trim().toLowerCase() : ''
      const code = nl >= 0 ? part.slice(nl + 1) : part
      const src = code.replace(/\n$/, '')
      if (lang === 'mermaid') {
        // mermaid 块用占位符发出去，渲染管线（ChatView 那边的 hookMermaidRender）
        // 后置扫描 .md-mermaid 调 mermaid.render() 替换。原文存 data-source，主题
        // 切换时可以重新渲染。
        html += `<div class="md-mermaid" data-source="${encodeURIComponent(src)}"><pre class="md-mermaid-source">${escapeHtml(src)}</pre></div>`
      } else {
        html += `<pre class="code-block"><code>${escapeHtml(src)}</code></pre>`
      }
    } else if (part) {
      for (const seg of extractTables(part)) {
        if (seg.kind === 'table') html += seg.html
        else if (seg.text) html += `<div class="text-run">${inline(seg.text)}</div>`
      }
    }
  })
  if (codes.length) {
    html = html.replace(
      /CMD(\d+)/g,
      (_m, n) => `<code class="cmd-tag">${escapeHtml(codes[Number(n)])}</code>`,
    )
  }
  return html
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

/** 紧凑 token 数：≤ 999 直接写，1000-999_999 显示 `12.3K`，≥ 1M 显示 `1.2M`。
 *  整 K / 整 M 去掉尾随零（`10K` 而不是 `10.0K`），非整数永远保留 1 位小数
 *  —— 跟 codeburn 一致，否则 `240.5K out` 会被显示成 `241K`，对账时看着像 bug。 */
export function formatTokens(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0'
  if (n < 1000) return `${Math.round(n)}`
  const unit = n < 1_000_000 ? 'K' : 'M'
  const scaled = n / (n < 1_000_000 ? 1000 : 1_000_000)
  return `${scaled.toFixed(1).replace(/\.0$/, '')}${unit}`
}

function pad(n: number): string {
  return n < 10 ? `0${n}` : `${n}`
}

/** 把毫秒时间戳或 ISO 字符串格式化为本地时间。 */
export function formatTime(input: number | string | undefined): string {
  if (input === undefined || input === '') return '—'
  const d = new Date(input)
  if (isNaN(d.getTime())) return '—'
  const now = new Date()
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate()
  // 也判断"昨天"，让相对日期更有用
  const ms = 24 * 60 * 60 * 1000
  const yesterday = new Date(now.getTime() - ms)
  const isYesterday =
    d.getFullYear() === yesterday.getFullYear() &&
    d.getMonth() === yesterday.getMonth() &&
    d.getDate() === yesterday.getDate()
  const hm = `${pad(d.getHours())}:${pad(d.getMinutes())}`
  if (sameDay) return `${t('time.today')} ${hm}`
  if (isYesterday) return `${t('time.yesterday')} ${hm}`
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${hm}`
}

/** 从完整路径取最后一段，作为项目短名。 */
export function shortName(path: string): string {
  const parts = path.split('/').filter(Boolean)
  return parts.length ? parts[parts.length - 1] : path
}

/** 关键词高亮用的文本片段：hit 为 true 的片段是命中段。 */
export interface HlSegment {
  text: string
  hit: boolean
}

/** 把 text 按 query（大小写不敏感）的出现位置切成片段，hit 标记命中段。
 *  query 为空 / text 为空 / 无匹配时返回单段未命中。用 indexOf 而非正则，
 *  天然免疫 query 里的正则特殊字符。供会话列表的关键词高亮使用。 */
export function highlightSegments(text: string, query: string): HlSegment[] {
  const q = query.trim().toLowerCase()
  if (!q || !text) return [{ text, hit: false }]
  const lower = text.toLowerCase()
  const segs: HlSegment[] = []
  let i = 0
  let at = lower.indexOf(q)
  while (at !== -1) {
    if (at > i) segs.push({ text: text.slice(i, at), hit: false })
    segs.push({ text: text.slice(at, at + q.length), hit: true })
    i = at + q.length
    at = lower.indexOf(q, i)
  }
  if (i < text.length) segs.push({ text: text.slice(i), hit: false })
  return segs
}
