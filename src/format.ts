// 轻量文本格式化：把会话内容渲染成可读的 HTML（无第三方依赖）。
import { t } from './i18n'

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

function inline(text: string): string {
  let s = escapeHtml(text)
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
    const idx = codes.push(inner) - 1
    return `CMD${idx}`
  })
  return { text, codes }
}

/** 渲染 Markdown 子集：围栏代码块 + 行内强调。 */
export function renderText(raw: string): string {
  const { text: pre, codes } = extractCommandTags(raw)
  const parts = pre.split('```')
  let html = ''
  parts.forEach((part, i) => {
    if (i % 2 === 1) {
      const nl = part.indexOf('\n')
      const code = nl >= 0 ? part.slice(nl + 1) : part
      html += `<pre class="code-block"><code>${escapeHtml(
        code.replace(/\n$/, ''),
      )}</code></pre>`
    } else if (part) {
      html += `<div class="text-run">${inline(part)}</div>`
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
