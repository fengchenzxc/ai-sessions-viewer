// 把 `Msg[]` 序列化成 Markdown / HTML，弹出原生 Save As 对话框让用户选位置，
// 然后通过 Tauri 命令把字节落到选中的路径。
//
// 不要用 Blob + <a download> —— Tauri 的 WKWebView（macOS）不识别 download
// 属性，blob URL 直接被吞（dev mode 浏览器里看上去正常，原生包里完全没反应）。
// 走 dialog.save() + write_file 是稳的路径，同时让用户能选目录/改文件名。

import type { Msg, Block, SessionMeta, Agent, DiffHunk } from './types'
import { writeFile } from './api'
import { save as saveDialog, open as openDialog } from '@tauri-apps/plugin-dialog'
import { t } from './i18n'
import { formatTime, isCaveatOnlyMsg, parseSystemEvent, renderText } from './format'
import {
  highlightJsonInPlace,
  looksLikeJson,
  prettifyAndHighlightJson,
} from './jsonHighlight'
import { highlightDiff, looksLikeDiff } from './diffHighlight'

function sanitizeFilename(name: string): string {
  const cleaned = name.replace(/[\\/:*?"<>|\n\r\t]/g, '_').trim()
  return (cleaned.slice(0, 80) || 'session').replace(/\s+/g, ' ')
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

// 头像 SVG（与会话详情用的图标字典对齐：claude/codex 取自 iconify 在 src/components/icons.ts
// 的同名导入；user/tool 用 lucide 标准路径）。导出 HTML 是离线静态文件，
// 不能依赖 Vue runtime，所以这里直接内联 SVG 字符串。
const AVATAR_SVG = {
  claude: '<svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><g fill="#ff7043"><path d="m14.375 6.48l.49.28v.209l-.14.489l-5.937 1.397l-.558-1.387zm0 0"/><path d="m12.155 2.373l.683.143l.182.224l.173.535l-.072.342l-3.983 5.447L7.81 7.737l3.673-4.82z"/><path d="m8.719 1.522l.419-.28l.349.14l.349.49l-.957 5.748l-.65-.441l-.279-.769l.49-4.33z"/><path d="m4.239 1.614l.43-.55L4.95 1l.558.081l.275.216l2.004 4.442l.724 2.11l-.848.471l-3.231-5.864z"/><path d="m2.154 4.665l-.14-.56l.42-.488l.488.07h.14l2.933 2.165l.908.698l1.257.978l-.698 1.187l-.629-.489l-.419-.419l-4.05-2.863z"/><path d="M1.316 8.296L1 7.946v-.31l.316-.108l3.562.21l3.491.279l-.113.695l-6.66-.346z"/><path d="M3.411 11.931h-.698l-.278-.32v-.382l1.186-.838l4.82-3.068l.487.833z"/><path d="m4.738 13.883l-.28.07l-.418-.21l.07-.35l4.12-5.446l.558.768l-3.072 4.05z"/><path d="m8.23 14.581l-.21.28l-.419.14l-.349-.28l-.21-.42L8.09 8.646l.629.07z"/><path d="M11.791 13.045v.558l-.07.21l-.279.14l-.489-.066l-3.356-4.996l1.331-1.014l1.117 2.025l.105.733z"/><path d="m13.398 12.207l.07.349l-.21.279l-.21-.07l-1.187-.838l-1.815-1.606l-1.397-.978l.419-1.326l.698.419l.42.768z"/><path d="m12.49 8.645l1.746.14l.419.28l.279.418v.302l-.768.327l-3.911-.978l-1.606-.07l.419-1.466l1.117.838z"/></g></svg>',
  codex: '<svg viewBox="0 0 48 48" width="18" height="18" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-width="3" stroke-linejoin="round"><path d="M18.38 27.94v-14.4l11.19-6.46c6.2-3.58 17.3 5.25 12.64 13.33"/><path d="m18.38 20.94l12.47-7.2l11.19 6.46c6.2 3.58 4.1 17.61-5.23 17.61"/><path d="m24.44 17.44l12.47 7.2v12.93c0 7.16-13.2 12.36-17.86 4.28"/><path d="M30.5 21.2v14.14L19.31 41.8c-6.2 3.58-17.3-5.25-12.64-13.33"/><path d="m30.5 27.94l-12.47 7.2l-11.19-6.46c-6.21-3.59-4.11-17.61 5.22-17.61"/><path d="m24.44 31.44l-12.47-7.2V11.31c0-7.16 13.2-12.36 17.86-4.28"/></g></svg>',
  // Gemini icon body 取自 material-icon-theme/gemini-ai（与应用内 IconGemini 同一资产）。
  gemini: '<svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path fill="#448aff" d="M15 8.014A7.457 7.457 0 0 0 8.014 15h-.028A7.456 7.456 0 0 0 1 8.014v-.028A7.456 7.456 0 0 0 7.986 1h.028A7.457 7.457 0 0 0 15 7.986z"/></svg>',
  user: '<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="8" r="5"/><path d="M20 21a8 8 0 0 0-16 0"/></svg>',
  tool: '<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>',
  arrowUp: '<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 19V5"/><path d="m5 12 7-7 7 7"/></svg>',
  arrowDown: '<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 5v14"/><path d="m19 12-7 7-7-7"/></svg>',
} as const

function avatarSvg(role: string, agent: Agent): string {
  if (role === 'tool') return AVATAR_SVG.tool
  if (role === 'user') return AVATAR_SVG.user
  if (agent === 'codex') return AVATAR_SVG.codex
  if (agent === 'gemini') return AVATAR_SVG.gemini
  return AVATAR_SVG.claude
}

function roleLabel(role: string, agent: Agent): string {
  if (role === 'tool') return t('chat.role.tool')
  if (role === 'user') return t('chat.role.me')
  if (agent === 'codex') return 'Codex'
  if (agent === 'gemini') return 'Gemini'
  return 'Claude'
}

// 在 Claude 的 JSONL 中，tool_result 块被装在 role:"user" 的消息里
// （表示用户"把"工具输出回传给模型）。视觉上这其实是 agent 这边的产物，
// 跟 ChatView.isToolOnly 一致：整条消息都是 tool_result 时不算作用户输入。
function isToolOnly(m: Msg): boolean {
  return m.role === 'user' && m.blocks.length > 0 && m.blocks.every((b) => b.kind === 'tool_result')
}

// 把 system event（目前只有 /rename）翻成本地化的句子；非 system event 返回 null。
function systemEventText(m: Msg): string | null {
  const ev = parseSystemEvent(m)
  if (!ev) return null
  if (ev.kind === 'rename') return t('chat.systemEvent.rename', { name: ev.name })
  return null
}

// 与 ChatView.stats 同步：u = 真正的用户消息条数（排除 tool-only / caveat-only /
// system-event），a = 助手消息条数。
function computeStats(messages: Msg[]): { u: number; a: number } {
  let u = 0
  let a = 0
  for (const m of messages) {
    if (
      m.role === 'user' &&
      !isToolOnly(m) &&
      !isCaveatOnlyMsg(m) &&
      !systemEventText(m)
    ) {
      u++
    } else if (m.role === 'assistant') a++
  }
  return { u, a }
}

// 跟 ChatView 一致：这些工具的 result 单独以一行 diff 块展示；
// 其它工具（Read/Bash/Grep/…）的 result 折叠回它对应的 tool_use 内。
const FILE_MUTATING_TOOLS = new Set([
  'Write',
  'Edit',
  'MultiEdit',
  'NotebookEdit',
  'apply_patch',
])

// 把 toolId 对应的 tool_result 找出来；非 file-mutating 的会被内联到 tool_use 里展示，
// 这些 result 不再单独成行。
function buildInlinedResults(messages: Msg[]): {
  resultByToolId: Map<string, Block>
  inlinedIds: Set<string>
} {
  const resultByToolId = new Map<string, Block>()
  for (const m of messages) {
    for (const b of m.blocks) {
      if (b.kind === 'tool_result' && b.toolId) resultByToolId.set(b.toolId, b)
    }
  }
  const inlinedIds = new Set<string>()
  for (const m of messages) {
    for (const b of m.blocks) {
      if (
        b.kind === 'tool_use' &&
        b.toolId &&
        !FILE_MUTATING_TOOLS.has(b.toolName ?? '') &&
        resultByToolId.has(b.toolId)
      ) {
        inlinedIds.add(b.toolId)
      }
    }
  }
  return { resultByToolId, inlinedIds }
}

function diffToText(hunks: DiffHunk[]): string {
  const lines: string[] = []
  for (const h of hunks) {
    lines.push(`@@ -${h.oldStart},_ +${h.newStart},_ @@`)
    for (const l of h.lines) {
      const prefix = l.kind === 'add' ? '+' : l.kind === 'del' ? '-' : ' '
      lines.push(`${prefix}${l.text}`)
    }
  }
  return lines.join('\n')
}

// ============================ Markdown ============================

function toolResultMd(b: Block): string {
  const head = b.filePath
    ? `> 📄 **${t('tool.resultDiff', { file: b.filePath })}**`
    : b.isError
      ? `> ⚠️ **${t('tool.resultError')}**`
      : `> 📤 **${t('tool.result')}**`
  if (b.diff && b.diff.length) {
    return [head, '', '```diff', diffToText(b.diff), '```'].join('\n')
  }
  const txt = (b.text ?? '').trim()
  if (!txt) return head
  return [head, '', '```', txt, '```'].join('\n')
}

function blockToMd(b: Block, ctx: { resultByToolId: Map<string, Block>; inlinedIds: Set<string> }): string {
  switch (b.kind) {
    case 'text':
      return (b.text ?? '').trim()
    case 'thinking':
      return [
        '<details>',
        `<summary>🧠 ${t('tool.thinking')}</summary>`,
        '',
        (b.text ?? '').trim(),
        '',
        '</details>',
      ].join('\n')
    case 'tool_use': {
      const head = `> 🔧 **${t('tool.call', { name: b.toolName ?? '' })}**`
      const args = (b.toolInput ?? '').trim()
      const lines = [head]
      if (args) lines.push('', '```json', args, '```')
      // 把对应的非 file-mutating result 内联在 tool_use 下方
      if (b.toolId && ctx.inlinedIds.has(b.toolId)) {
        const r = ctx.resultByToolId.get(b.toolId)
        if (r) lines.push('', toolResultMd(r))
      }
      return lines.join('\n')
    }
    case 'tool_result': {
      // 被 tool_use 吸收的不再单独输出
      if (b.toolId && ctx.inlinedIds.has(b.toolId)) return ''
      return toolResultMd(b)
    }
    case 'image':
      return b.imageSrc ? `![image](${b.imageSrc})` : ''
    default:
      return ''
  }
}

function msgToMd(
  m: Msg,
  agent: Agent,
  ctx: { resultByToolId: Map<string, Block>; inlinedIds: Set<string> },
): string {
  // System event (e.g. /rename) — emit as a horizontal-rule-bracketed italic line.
  const sysText = systemEventText(m)
  if (sysText) {
    const ts = m.timestamp ? ` · ${formatTime(m.timestamp)}` : ''
    return `_${sysText}${ts}_`
  }
  const ts = m.timestamp ? ` · ${formatTime(m.timestamp)}` : ''
  const model = m.model ? ` · ${m.model}` : ''
  const displayRole = isToolOnly(m) ? 'tool' : m.role
  const head = `## ${roleLabel(displayRole, agent)}${model}${ts}`
  const body = m.blocks.map((b) => blockToMd(b, ctx)).filter(Boolean).join('\n\n')
  return body ? `${head}\n\n${body}` : head
}

export function messagesToMarkdown(
  session: SessionMeta,
  messages: Msg[],
  agent: Agent,
): string {
  const ctx = buildInlinedResults(messages)
  const { u, a } = computeStats(messages)
  const statsLine = t('chat.stats', {
    u,
    a,
    time: session.created ? formatTime(session.created) : '—',
  })
  const meta = [
    `# ${session.title}`,
    '',
    `- ${statsLine}`,
    `- ${t('export.meta.agent')}: \`${agent}\``,
    session.cwd ? `- ${t('export.meta.cwd')}: \`${session.cwd}\`` : '',
    session.id ? `- ${t('export.meta.id')}: \`${session.id}\`` : '',
    '',
    '---',
  ]
    .filter(Boolean)
    .join('\n')
  // 过滤：1) 整条都是被内联 tool_result 的行（避免空 "## Tool"）
  //       2) Claude Code 的 local-command-caveat 噪音
  const visible = messages.filter((m) => {
    if (isCaveatOnlyMsg(m)) return false
    const blocks = m.blocks.map((b) => blockToMd(b, ctx)).filter(Boolean)
    return blocks.length > 0 || !isToolOnly(m)
  })
  const body = visible.map((m) => msgToMd(m, agent, ctx)).join('\n\n')
  return `${meta}\n\n${body}\n`
}

// ============================ HTML ============================

// Geist-style tokens. The light/dark palettes mirror src/style.css so exported
// transcripts look like the app. `data-theme="dark"` on <html> picks dark; the
// in-page toggle button flips that attribute (and persists to localStorage for
// the standalone file).
const HTML_STYLE = `
:root {
  color-scheme: light dark;
  --bg: hsl(0 0% 100%);
  --surface: hsl(0 0% 100%);
  --surface-2: hsl(0 0% 98%);
  --surface-hover: hsl(0 0% 95%);
  --border: hsl(0 0% 92%);
  --border-strong: hsl(0 0% 79%);
  --text: hsl(0 0% 9%);
  --text-dim: hsl(0 0% 30%);
  --text-mute: hsl(0 0% 56%);
  --user-bg: hsl(0 0% 96%);
  --code-bg: hsl(0 0% 96%);
  --diff-add: rgba(22, 163, 74, 0.14);
  --diff-del: rgba(220, 38, 38, 0.14);
  --link: hsl(212 100% 48%);
}
:root[data-theme="dark"] {
  --bg: hsl(0 0% 4%);
  --surface: hsl(0 0% 4%);
  --surface-2: hsl(0 0% 0%);
  --surface-hover: hsl(0 0% 10%);
  --border: hsl(0 0% 16%);
  --border-strong: hsl(0 0% 27%);
  --text: hsl(0 0% 93%);
  --text-dim: hsl(0 0% 63%);
  --text-mute: hsl(0 0% 53%);
  --user-bg: hsl(0 0% 8%);
  --code-bg: hsl(0 0% 10%);
  --diff-add: rgba(22, 163, 74, 0.22);
  --diff-del: rgba(220, 38, 38, 0.22);
  --link: hsl(210 100% 66%);
}
* { box-sizing: border-box; }
body {
  font: 14px/1.6 'Inter', -apple-system, BlinkMacSystemFont, 'SF Pro Text', 'PingFang SC', 'Helvetica Neue', Arial, sans-serif;
  max-width: 1200px; margin: 0 auto; padding: 0 24px 80px;
  color: var(--text); background: var(--bg);
  font-feature-settings: 'cv11', 'ss01';
}
a { color: var(--link); text-decoration: none; }
a:hover { text-decoration: underline; }
/* Sticky title + meta strip. We keep it inside the 1200px max-width column
   so it lines up with the body; background must be opaque to mask scrolling
   content underneath. The thin bottom border doubles as the meta divider. */
.sticky-head {
  position: sticky; top: 0; z-index: 20;
  background: var(--bg);
  border-bottom: 1px solid var(--border);
  margin: 0 -24px 24px; padding: 24px 24px 16px;
}
.header {
  display: flex; align-items: center; justify-content: space-between;
  gap: 16px; margin: 0 0 12px;
}
h1 { font-size: 22px; font-weight: 600; margin: 0; letter-spacing: -0.01em; }
.theme-toggle {
  appearance: none; background: var(--surface); color: var(--text-dim);
  border: 1px solid var(--border); border-radius: 8px;
  padding: 6px 12px; font: inherit; font-size: 12px; cursor: pointer;
  display: inline-flex; align-items: center; gap: 6px;
  transition: background .15s, color .15s, border-color .15s;
}
.theme-toggle:hover { background: var(--surface-hover); color: var(--text); border-color: var(--border-strong); }
.meta { color: var(--text-mute); font-size: 12px; }
.meta code { background: transparent; padding: 0; color: var(--text-dim); }

/* WeChat-style chat layout: user on the right, assistant on the left.
   Avatar + bubble side-by-side; bubble has an asymmetric corner pointing
   toward the avatar to mimic the speech-bubble tail. */
.msg {
  display: flex; align-items: flex-start; gap: 10px;
  margin: 18px 0;
}
.msg.user { flex-direction: row-reverse; }
.avatar {
  flex: 0 0 32px; width: 32px; height: 32px;
  border-radius: 50%; background: var(--surface-2);
  border: 1px solid var(--border);
  display: inline-flex; align-items: center; justify-content: center;
  color: var(--text-dim);
  user-select: none;
}
.avatar svg { display: block; }
.msg.user .avatar { color: var(--text); }
.bubble {
  max-width: min(75%, 880px);
  padding: 12px 16px;
  border: 1px solid var(--border);
  border-radius: 14px;
  background: var(--surface);
}
.msg.user .bubble {
  background: var(--user-bg);
  border-top-right-radius: 4px;
}
.msg.assistant .bubble {
  border-top-left-radius: 4px;
}
.msg.tool .bubble {
  background: var(--surface-2);
  border-top-left-radius: 4px;
}
.msg.tool .avatar {
  background: var(--surface-2); color: var(--text-mute);
}
.tool-result-inline {
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px dashed var(--border);
}
/* System events (e.g. /rename) render as a small centered meta line. */
.msg.system { justify-content: center; margin: 14px 0; }
.system-event {
  color: var(--text-mute);
  font-size: 12px;
  text-align: center;
  padding: 2px 12px;
}
.role-tag {
  font-size: 11px; color: var(--text-mute);
  text-transform: uppercase; letter-spacing: 0.08em;
  margin-bottom: 8px; font-weight: 500;
}
.msg.user .role-tag { text-align: right; }
.text { white-space: pre-wrap; word-break: break-word; }
.text-run { white-space: pre-wrap; word-break: break-word; }
.text-run h3 { font-size: 15px; font-weight: 600; margin: 14px 0 6px; }
.text-run h4 { font-size: 13.5px; font-weight: 600; margin: 10px 0 4px; }
/* renderText emit 的 fenced code 块。沿用上面 pre / code 的样式，class 留作钩子。 */
.code-block { display: block; }
/* GFM 表格 —— 行容器 .md-table-wrap 提供横向滚动；表格本身用 design tokens 上色。 */
.md-table-wrap {
  max-width: 100%; overflow-x: auto; margin: 10px 0;
  border: 1px solid var(--border); border-radius: 8px;
  -webkit-overflow-scrolling: touch;
  /* 浅色模式下默认 native scrollbar 几乎是黑色，把 thumb 改成跟正文同色 22% 透明 */
  scrollbar-width: thin;
  scrollbar-color: color-mix(in srgb, var(--text) 22%, transparent) transparent;
}
.md-table-wrap::-webkit-scrollbar { height: 7px; width: 7px; }
.md-table-wrap::-webkit-scrollbar-track { background: transparent; }
.md-table-wrap::-webkit-scrollbar-thumb {
  background: color-mix(in srgb, var(--text) 22%, transparent);
  border-radius: 999px;
}
.md-table-wrap::-webkit-scrollbar-thumb:hover {
  background: color-mix(in srgb, var(--text) 38%, transparent);
}
.md-table {
  width: max-content; min-width: 100%;
  border-collapse: separate; border-spacing: 0;
  font-size: 13px; line-height: 1.5; background: var(--surface);
}
.md-table thead { background: var(--surface-2); }
.md-table th, .md-table td {
  padding: 7px 12px; text-align: left; vertical-align: top;
  border-bottom: 1px solid var(--border);
}
.md-table th { font-weight: 600; font-size: 12px; }
.md-table tbody tr:last-child td { border-bottom: 0; }
.md-table tbody tr:hover td { background: var(--surface-hover); }
.md-table code { font-size: 12px; }
/* Mermaid 流程图 —— 导出时已经 prerender 成 SVG 烤进 HTML，离线可看。
 * 主题以导出时刻的 app 主题为准（mermaid SVG 颜色烤死），切换 HTML 主题时
 * 其它元素跟着变，mermaid 图保持不变。 */
.md-mermaid {
  display: block; margin: 10px 0; padding: 12px;
  border: 1px solid var(--border); border-radius: 8px;
  background: var(--surface); overflow-x: auto; text-align: center;
}
.md-mermaid svg { max-width: 100%; height: auto; }
.md-mermaid-source {
  margin: 0; padding: 10px 12px; background: var(--code-bg);
  border-radius: 6px; font-size: 12px; white-space: pre; overflow-x: auto;
  text-align: left;
  font-family: 'SF Mono', 'JetBrains Mono', Menlo, Consolas, monospace;
}
.md-mermaid-error { border-color: hsl(0 70% 60% / 0.5); }
.md-mermaid-errmsg {
  font-size: 12px; color: hsl(0 70% 50%);
  margin-bottom: 8px; text-align: left;
  font-family: 'SF Mono', 'JetBrains Mono', Menlo, Consolas, monospace;
}
.cmd-tag { background: var(--surface-hover); }
/* JSON syntax highlight：tool_use args 与 JSON 形态的 tool_result。 */
.lang-json .json-key { color: hsl(214 65% 42%); }
.lang-json .json-string { color: hsl(140 50% 32%); }
.lang-json .json-num { color: hsl(280 55% 45%); }
.lang-json .json-bool { color: hsl(14 75% 45%); font-weight: 500; }
.lang-json .json-null { color: var(--text-mute); font-style: italic; }
:root[data-theme="dark"] .lang-json .json-key { color: hsl(214 80% 70%); }
:root[data-theme="dark"] .lang-json .json-string { color: hsl(140 50% 65%); }
:root[data-theme="dark"] .lang-json .json-num { color: hsl(280 70% 75%); }
:root[data-theme="dark"] .lang-json .json-bool { color: hsl(14 75% 65%); }
/* Unified diff syntax highlight：Bash 跑 git diff / 工具吐 patch 等文本形态 diff。
   颜色复用 DiffBlock 的 add/del 语义，不画底色（避免和外层 pre 背景打架）。 */
.lang-diff { display: block; }
.lang-diff .diff-file { color: var(--text); font-weight: 600; }
.lang-diff .diff-meta { color: var(--text-mute); }
.lang-diff .diff-hunk { color: hsl(214 50% 45%); font-weight: 500; }
.lang-diff .diff-add { color: hsl(140 55% 32%); background: color-mix(in srgb, hsl(140 55% 45%) 12%, transparent); display: block; }
.lang-diff .diff-del { color: hsl(0 65% 42%); background: color-mix(in srgb, hsl(0 65% 50%) 12%, transparent); display: block; }
.lang-diff .diff-ctx { color: var(--text); }
:root[data-theme="dark"] .lang-diff .diff-hunk { color: hsl(214 70% 70%); }
:root[data-theme="dark"] .lang-diff .diff-add { color: hsl(140 55% 70%); background: color-mix(in srgb, hsl(140 55% 50%) 18%, transparent); }
:root[data-theme="dark"] .lang-diff .diff-del { color: hsl(0 70% 72%); background: color-mix(in srgb, hsl(0 70% 55%) 18%, transparent); }
pre {
  background: var(--code-bg); padding: 12px 14px; border-radius: 8px;
  border: 1px solid var(--border);
  overflow-x: auto;
  font: 12.5px/1.55 'SF Mono', 'JetBrains Mono', Menlo, Consolas, monospace;
  white-space: pre-wrap; word-break: break-word;
  color: var(--text);
}
code {
  background: var(--code-bg); padding: 1px 6px; border-radius: 4px;
  font: 0.92em 'SF Mono', 'JetBrains Mono', Menlo, Consolas, monospace;
  border: 1px solid var(--border);
}
pre code { background: transparent; padding: 0; border: 0; }
details {
  margin: 10px 0; border: 1px solid var(--border); border-radius: 8px;
  padding: 8px 12px; background: var(--surface-2);
}
details > summary {
  cursor: pointer; font-size: 12px; color: var(--text-dim);
  list-style: none; user-select: none;
}
details > summary::-webkit-details-marker { display: none; }
details > summary::before {
  content: '›'; display: inline-block; margin-right: 6px;
  transition: transform .15s; color: var(--text-mute);
}
details[open] > summary::before { transform: rotate(90deg); }
details[open] > summary { margin-bottom: 10px; }
img { max-width: 100%; border-radius: 6px; border: 1px solid var(--border); }
/* 图片可点击放大 —— 见 blockToHtml case 'image' 的 onclick + lightbox runtime。 */
img.msg-image { cursor: zoom-in; }
img.msg-image:hover { border-color: var(--border-strong); }
/* Lightbox：fixed 覆盖层，不开就 display:none；img 居中且按视口尺寸限缩。 */
.csv-lightbox {
  position: fixed; inset: 0;
  display: none;
  align-items: center; justify-content: center;
  background: rgba(0, 0, 0, 0.78);
  z-index: 9999;
  cursor: zoom-out;
  padding: 32px;
}
.csv-lightbox.open { display: flex; }
.csv-lightbox img {
  max-width: 100%; max-height: 100%;
  border-radius: 6px; border: none;
  box-shadow: 0 16px 48px rgba(0, 0, 0, 0.45);
}
.diff {
  background: var(--surface-2); border: 1px solid var(--border);
  border-radius: 8px; padding: 10px 12px;
  font: 12px/1.55 'SF Mono', Menlo, Consolas, monospace; overflow-x: auto;
}
.diff .add { background: var(--diff-add); display: block; }
.diff .del { background: var(--diff-del); display: block; }
.diff .ctx { display: block; color: var(--text-mute); }

/* Show-more / Show-less. JS wraps existing children in .collapsible-inner
   on first scan, measures inner height, and only injects the toggle button
   when content exceeds --max. Matches CollapsibleBox.vue in the app. */
.collapsible-box { position: relative; --max: 320px; }
.collapsible-inner { overflow: hidden; }
.collapsible-box.collapsed .collapsible-inner {
  max-height: var(--max);
  -webkit-mask-image: linear-gradient(to bottom, #000 70%, transparent 100%);
          mask-image: linear-gradient(to bottom, #000 70%, transparent 100%);
}
.collapsible-toggle {
  display: flex; align-items: center; justify-content: center; gap: 4px;
  margin: 8px auto 0; padding: 4px 10px;
  background: transparent; border: 0; color: var(--text-mute);
  font: inherit; font-size: 12px; cursor: pointer;
  border-radius: 6px; transition: background .12s, color .12s;
}
.collapsible-toggle:hover { background: var(--surface-hover); color: var(--text); }
.collapsible-toggle .chev { display: inline-block; transition: transform .15s; }
.collapsible-toggle.open .chev { transform: rotate(180deg); }

/* Scroll-to-top / scroll-to-bottom floating buttons (mirrors ChatView FABs).
   Hidden when at the corresponding edge with an 8px tolerance. */
.fabs {
  position: fixed; right: 24px; bottom: 24px; z-index: 30;
  display: flex; flex-direction: column; gap: 10px;
  pointer-events: none;
}
.fab {
  pointer-events: auto;
  width: 36px; height: 36px; border-radius: 50%;
  background: var(--surface); color: var(--text-dim);
  border: 1px solid var(--border);
  display: inline-flex; align-items: center; justify-content: center;
  cursor: pointer; padding: 0;
  box-shadow: 0 1px 3px rgba(0,0,0,0.08);
  transition: opacity .18s, transform .18s, background .12s, color .12s, border-color .12s;
}
.fab:hover { background: var(--surface-hover); color: var(--text); border-color: var(--border-strong); }
.fab[data-hidden="1"] { opacity: 0; pointer-events: none; transform: translateY(8px); }
`

function buildRuntimeScript(labels: {
  more: string
  less: string
  themeLight: string
  themeDark: string
}): string {
  const L_LIGHT = JSON.stringify(`☀ ${labels.themeLight}`)
  const L_DARK = JSON.stringify(`☾ ${labels.themeDark}`)
  return `
(function () {
  var KEY = 'csv-export-theme';
  var root = document.documentElement;
  var stored = null;
  try { stored = localStorage.getItem(KEY); } catch (_) {}
  if (stored === 'light' || stored === 'dark') {
    root.setAttribute('data-theme', stored);
  }
  var THEME_LIGHT = ${L_LIGHT};
  var THEME_DARK = ${L_DARK};
  function paintTheme() {
    var btn = document.getElementById('theme-toggle');
    if (!btn) return;
    var dark = root.getAttribute('data-theme') === 'dark';
    // Button shows the *destination* theme — clicking it switches you there.
    btn.textContent = dark ? THEME_LIGHT : THEME_DARK;
  }
  var L_MORE = ${JSON.stringify(labels.more)};
  var L_LESS = ${JSON.stringify(labels.less)};
  var MAX_PX = 320;
  function setupCollapsible(box) {
    if (box.dataset.csvCollapsible) return;
    box.dataset.csvCollapsible = '1';
    // Wrap whatever the box had in a single .collapsible-inner so we can
    // size/mask it without touching the toggle button we add as a sibling.
    var inner = document.createElement('div');
    inner.className = 'collapsible-inner';
    while (box.firstChild) inner.appendChild(box.firstChild);
    box.appendChild(inner);
    if (inner.scrollHeight <= MAX_PX + 1) return;
    box.classList.add('collapsed');
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'collapsible-toggle';
    btn.innerHTML = '<span class="chev">▾</span><span class="label">' + L_MORE + '</span>';
    box.appendChild(btn);
    btn.addEventListener('click', function () {
      var collapsedNow = box.classList.contains('collapsed');
      box.classList.toggle('collapsed', !collapsedNow);
      btn.classList.toggle('open', collapsedNow);
      btn.querySelector('.label').textContent = collapsedNow ? L_LESS : L_MORE;
    });
  }
  function scan() {
    var boxes = document.querySelectorAll('[data-collapsible]');
    for (var i = 0; i < boxes.length; i++) setupCollapsible(boxes[i]);
  }
  document.addEventListener('DOMContentLoaded', function () {
    paintTheme();
    var btn = document.getElementById('theme-toggle');
    if (btn) btn.addEventListener('click', function () {
      var dark = root.getAttribute('data-theme') === 'dark';
      var next = dark ? 'light' : 'dark';
      root.setAttribute('data-theme', next);
      try { localStorage.setItem(KEY, next); } catch (_) {}
      paintTheme();
    });
    scan();
    // Re-measure when a <details> opens for the first time — file change is
    // already open at load, but Read/Bash results expand on click.
    document.addEventListener('toggle', function (e) {
      if (e.target && e.target.tagName === 'DETAILS' && e.target.open) scan();
    }, true);

    // ----- smooth scroll FABs (mirrors ChatView.scrollToTop / ToBottom) -----
    var fabTop = document.getElementById('fab-top');
    var fabBottom = document.getElementById('fab-bottom');
    var rafScroll = 0;
    function cancelScroll() {
      if (rafScroll) { cancelAnimationFrame(rafScroll); rafScroll = 0; }
    }
    function smoothScrollTo(target) {
      cancelScroll();
      var start = window.scrollY;
      var max = Math.max(0, document.documentElement.scrollHeight - window.innerHeight);
      var dest = Math.max(0, Math.min(target, max));
      var dist = dest - start;
      if (Math.abs(dist) < 2) { window.scrollTo(0, dest); return; }
      var duration = Math.min(360, 180 + Math.abs(dist) * 0.05);
      var t0 = performance.now();
      function ease(p) { return 1 - Math.pow(1 - p, 3); }
      function step(now) {
        var p = Math.min(1, (now - t0) / duration);
        window.scrollTo(0, start + dist * ease(p));
        if (p < 1) rafScroll = requestAnimationFrame(step); else rafScroll = 0;
      }
      function onUserScroll() {
        cancelScroll();
        window.removeEventListener('wheel', onUserScroll);
        window.removeEventListener('touchmove', onUserScroll);
      }
      window.addEventListener('wheel', onUserScroll, { passive: true, once: true });
      window.addEventListener('touchmove', onUserScroll, { passive: true, once: true });
      rafScroll = requestAnimationFrame(step);
    }
    if (fabTop) fabTop.addEventListener('click', function () { smoothScrollTo(0); });
    if (fabBottom) fabBottom.addEventListener('click', function () {
      smoothScrollTo(document.documentElement.scrollHeight);
    });
    function updateEdges() {
      var y = window.scrollY;
      var max = document.documentElement.scrollHeight - window.innerHeight;
      var atTop = y <= 8;
      var atBottom = y >= max - 8;
      if (fabTop) fabTop.setAttribute('data-hidden', atTop ? '1' : '0');
      if (fabBottom) fabBottom.setAttribute('data-hidden', atBottom ? '1' : '0');
    }
    var rafEdge = 0;
    window.addEventListener('scroll', function () {
      if (rafEdge) return;
      rafEdge = requestAnimationFrame(function () { rafEdge = 0; updateEdges(); });
    }, { passive: true });
    window.addEventListener('resize', updateEdges);
    updateEdges();

    // ----- image lightbox -----
    // 同页放大查看。data: URL 无法走 window.open（Chrome 阻断顶层导航到 data:），
    // 改成 fixed 覆盖层。点遮罩 / 按 Esc 关闭。
    var lb = document.createElement('div');
    lb.id = 'csv-lightbox';
    lb.className = 'csv-lightbox';
    var lbImg = document.createElement('img');
    lb.appendChild(lbImg);
    document.body.appendChild(lb);
    function closeLb() { lb.classList.remove('open'); lbImg.removeAttribute('src'); }
    function openLb(src) {
      if (!src) return;
      lbImg.src = src;
      lb.classList.add('open');
    }
    lb.addEventListener('click', closeLb);
    document.addEventListener('keydown', function (e) {
      if (e.key === 'Escape' && lb.classList.contains('open')) closeLb();
    });
    window.__csvLightbox = openLb;
  });
})();
`
}

function diffToHtml(hunks: DiffHunk[]): string {
  const rows: string[] = []
  for (const h of hunks) {
    rows.push(
      `<span class="ctx">@@ -${h.oldStart}, +${h.newStart} @@</span>`,
    )
    for (const l of h.lines) {
      const cls = l.kind === 'add' ? 'add' : l.kind === 'del' ? 'del' : 'ctx'
      const sign = l.kind === 'add' ? '+' : l.kind === 'del' ? '-' : ' '
      rows.push(`<span class="${cls}">${escapeHtml(sign + l.text)}</span>`)
    }
  }
  return `<div class="diff">${rows.join('\n')}</div>`
}

function toolResultBodyHtml(b: Block): string {
  if (b.diff && b.diff.length) {
    return `<div class="collapsible-box" data-collapsible>${diffToHtml(b.diff)}</div>`
  }
  const txt = b.text ?? ''
  if (!txt) return ''
  // 渲染优先级：unified diff（`git diff` / patch 文本）→ JSON → 原样。
  // diff 必须先判，因为 JSON 文件的 diff 既像 diff 又像 JSON，应该按 diff 渲染。
  let pre: string
  if (looksLikeDiff(txt)) {
    pre = `<pre class="lang-diff">${highlightDiff(txt)}</pre>`
  } else if (looksLikeJson(txt)) {
    pre = `<pre class="lang-json">${highlightJsonInPlace(txt)}</pre>`
  } else {
    pre = `<pre>${escapeHtml(txt)}</pre>`
  }
  return `<div class="collapsible-box" data-collapsible>${pre}</div>`
}

// tool.resultDiff = "File change · {file}" / "文件改动 · {file}". Split out the
// {file} slot so the path can render as a <code> chip in HTML.
function splitDiffLabel(filePath: string): string {
  const SENTINEL = '__CSV_FILE__'
  const tmpl = t('tool.resultDiff', { file: SENTINEL })
  const idx = tmpl.indexOf(SENTINEL)
  if (idx < 0) return `${escapeHtml(tmpl)} <code>${escapeHtml(filePath)}</code>`
  const pre = escapeHtml(tmpl.slice(0, idx))
  const post = escapeHtml(tmpl.slice(idx + SENTINEL.length))
  return `${pre}<code>${escapeHtml(filePath)}</code>${post}`
}

function toolResultLabel(b: Block): string {
  if (b.filePath) return `📄 ${splitDiffLabel(b.filePath)}`
  if (b.isError) return `⚠️ ${escapeHtml(t('tool.resultError'))}`
  return `📤 ${escapeHtml(t('tool.result'))}`
}

function blockToHtml(
  b: Block,
  ctx: { resultByToolId: Map<string, Block>; inlinedIds: Set<string> },
): string {
  switch (b.kind) {
    case 'text':
      // 跟聊天界面一致：renderText() 给出表格 / fenced code / 行内强调 + 一个 mermaid
      // 占位符（<div class="md-mermaid" data-source="..."/>）。占位符在 messagesToHtml
      // 收尾阶段统一被 prerenderMermaidInHtml 替换成 SVG（一次性烤进 HTML，不依赖运行时 JS）。
      return renderText(b.text ?? '')
    case 'thinking':
      return `<details><summary>🧠 ${escapeHtml(t('tool.thinking'))}</summary><pre>${escapeHtml(b.text ?? '')}</pre></details>`
    case 'tool_use': {
      const label = escapeHtml(t('tool.call', { name: b.toolName ?? '' }))
      // Tool args 永远当 JSON 试 —— prettify + 上色；parse 失败也只是上 token 色，
      // 总比裸 escapeHtml 强。
      const args = prettifyAndHighlightJson(b.toolInput ?? '')
      let inner = `<pre class="lang-json">${args}</pre>`
      if (b.toolId && ctx.inlinedIds.has(b.toolId)) {
        const r = ctx.resultByToolId.get(b.toolId)
        if (r) {
          const body = toolResultBodyHtml(r)
          if (body) inner += `<div class="tool-result-inline">${body}</div>`
        }
      }
      return `<details><summary>🔧 ${label}</summary>${inner}</details>`
    }
    case 'tool_result': {
      // 已被 tool_use 吸收的不再单独出现
      if (b.toolId && ctx.inlinedIds.has(b.toolId)) return ''
      const label = toolResultLabel(b)
      const body = toolResultBodyHtml(b)
      // File change（有 diff 或 filePath）默认展开，跟会话详情一致
      const open = b.filePath || (b.diff && b.diff.length) ? ' open' : ''
      return body
        ? `<details${open}><summary>${label}</summary>${body}</details>`
        : `<details${open}><summary>${label}</summary></details>`
    }
    case 'image':
      // 导出的 HTML 里图片默认按文本宽度缩放，看不清细节；点击 → 同页 lightbox
      // 放大查看。原本用 window.open(this.src) 但 Chrome 拒绝从 window.open
      // 顶层导航到 data: URL（数据 URL 是 base64 图片，被 Block 成 about:blank）。
      // lightbox 在同页 fixed 覆盖，没有跨源 / 顶层导航问题。
      return b.imageSrc
        ? `<img src="${escapeHtml(b.imageSrc)}" alt="" class="msg-image" onclick="window.__csvLightbox&amp;&amp;window.__csvLightbox(this.src)">`
        : ''
    default:
      return ''
  }
}

function msgToHtml(
  m: Msg,
  agent: Agent,
  ctx: { resultByToolId: Map<string, Block>; inlinedIds: Set<string> },
): string {
  // System event row — centered, no avatar, no bubble.
  const sysText = systemEventText(m)
  if (sysText) {
    const ts = m.timestamp ? ` · ${escapeHtml(formatTime(m.timestamp))}` : ''
    return `<div class="msg system"><div class="system-event">${escapeHtml(sysText)}${ts}</div></div>`
  }
  const displayRole = isToolOnly(m) ? 'tool' : m.role
  const tag = [
    roleLabel(displayRole, agent),
    m.model ? escapeHtml(m.model) : '',
    m.timestamp ? escapeHtml(formatTime(m.timestamp)) : '',
  ]
    .filter(Boolean)
    .join(' · ')
  const body = m.blocks.map((b) => blockToHtml(b, ctx)).filter(Boolean).join('\n')
  if (!body) return ''
  // 跟 ChatView 一致：只有用户消息整体走 CollapsibleBox，超过 320px 才折叠+显示更多
  const wrappedBody =
    displayRole === 'user'
      ? `<div class="collapsible-box" data-collapsible>${body}</div>`
      : body
  return `<div class="msg ${displayRole}">
  <div class="avatar">${avatarSvg(displayRole, agent)}</div>
  <div class="bubble">
    <div class="role-tag">${tag}</div>
    ${wrappedBody}
  </div>
</div>`
}

function currentTheme(): 'light' | 'dark' {
  return document.documentElement.classList.contains('theme-dark') ? 'dark' : 'light'
}

/** 扫一遍 HTML 把 renderText 留下的 .md-mermaid 占位符替换成真 SVG。
 *  让导出 HTML 完全离线可看（不依赖运行时 mermaid.js）。
 *  - 一次性 dynamic-import mermaid；同一 source 二次出现复用上次的 SVG 不重画。
 *  - 渲染失败：保留占位符 + 一行错误提示 + 源码，跟聊天里的兜底一致。
 *  - 主题：用当前 app 的 theme（light/dark），SVG 颜色烤死；HTML 的 theme toggle 切
 *    其它元素的色，mermaid SVG 保持不变（mermaid 不支持运行时切主题）。 */
async function prerenderMermaidInHtml(html: string): Promise<string> {
  // 没占位符就别动 mermaid，避免给纯文本会话加 600KB 的解析开销。
  if (!html.includes('class="md-mermaid"')) return html
  let mermaid: typeof import('mermaid').default
  try {
    mermaid = (await import('mermaid')).default
  } catch (e) {
    // 拉不到 mermaid（离线 / 安装损坏）—— 直接交回带占位符的 HTML，源码 fallback 还在。
    console.warn('[export] mermaid load failed:', e)
    return html
  }
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: 'strict',
    theme: currentTheme() === 'dark' ? 'dark' : 'default',
    fontFamily:
      '-apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif',
  })
  const cache = new Map<string, { ok: true; svg: string } | { ok: false; err: string }>()
  // \s\S 跨行匹配占位符里的 fallback <pre>；同一占位符 div 不嵌套，懒匹配安全。
  const RE = /<div class="md-mermaid" data-source="([^"]*)">[\s\S]*?<\/div>/g
  const sources = new Set<string>()
  for (const m of html.matchAll(RE)) sources.add(m[1])
  let counter = 0
  for (const enc of sources) {
    counter += 1
    const src = decodeURIComponent(enc)
    try {
      const { svg } = await mermaid.render(`md-mermaid-export-${counter}`, src)
      cache.set(enc, { ok: true, svg })
    } catch (e) {
      cache.set(enc, { ok: false, err: (e as Error)?.message ?? String(e) })
    }
  }
  return html.replace(RE, (_, enc) => {
    const hit = cache.get(enc)
    const src = decodeURIComponent(enc)
    if (!hit) return _
    if (hit.ok) {
      return `<div class="md-mermaid" data-rendered>${hit.svg}</div>`
    }
    return (
      `<div class="md-mermaid md-mermaid-error" data-rendered>` +
      `<div class="md-mermaid-errmsg">mermaid: ${escapeHtml(hit.err)}</div>` +
      `<pre class="md-mermaid-source">${escapeHtml(src)}</pre>` +
      `</div>`
    )
  })
}

export async function messagesToHtml(
  session: SessionMeta,
  messages: Msg[],
  agent: Agent,
): Promise<string> {
  const title = escapeHtml(session.title)
  const { u, a } = computeStats(messages)
  const statsLine = escapeHtml(
    t('chat.stats', {
      u,
      a,
      time: session.created ? formatTime(session.created) : '—',
    }),
  )
  const meta = [
    statsLine,
    `${escapeHtml(t('export.meta.agent'))}: <code>${agent}</code>`,
    session.cwd ? `${escapeHtml(t('export.meta.cwd'))}: <code>${escapeHtml(session.cwd)}</code>` : '',
    session.id ? `${escapeHtml(t('export.meta.id'))}: <code>${escapeHtml(session.id)}</code>` : '',
  ]
    .filter(Boolean)
    .join(' &middot; ')
  const ctx = buildInlinedResults(messages)
  // 先生成 raw body（含 .md-mermaid 占位符），再一次性烤 SVG 进去。
  // 收尾才烤可以让多个 mermaid 块共用同一个 mermaid runtime 初始化。
  const rawBody = messages
    .filter((m) => !isCaveatOnlyMsg(m))
    .map((m) => msgToHtml(m, agent, ctx))
    .filter(Boolean)
    .join('\n')
  const body = await prerenderMermaidInHtml(rawBody)
  const theme = currentTheme()
  const themeLight = t('export.theme.light')
  const themeDark = t('export.theme.dark')
  const runtimeScript = buildRuntimeScript({
    more: t('chat.collapse.more'),
    less: t('chat.collapse.less'),
    themeLight,
    themeDark,
  })
  const initialBtnLabel = theme === 'dark' ? `☀ ${escapeHtml(themeLight)}` : `☾ ${escapeHtml(themeDark)}`
  const topLabel = escapeHtml(t('chat.action.top'))
  const bottomLabel = escapeHtml(t('chat.action.bottom'))
  return `<!doctype html>
<html lang="en" data-theme="${theme}">
<head>
<meta charset="utf-8">
<title>${title}</title>
<style>${HTML_STYLE}</style>
</head>
<body>
<div class="sticky-head">
  <div class="header">
    <h1>${title}</h1>
    <button id="theme-toggle" class="theme-toggle" type="button" aria-label="Toggle theme">${initialBtnLabel}</button>
  </div>
  <div class="meta">${meta}</div>
</div>
${body}
<div class="fabs">
  <button id="fab-top" class="fab" type="button" aria-label="${topLabel}" title="${topLabel}" data-hidden="1">${AVATAR_SVG.arrowUp}</button>
  <button id="fab-bottom" class="fab" type="button" aria-label="${bottomLabel}" title="${bottomLabel}">${AVATAR_SVG.arrowDown}</button>
</div>
<script>${runtimeScript}</script>
</body>
</html>
`
}

// ============================ 落盘 ============================
// 弹原生 Save As 让用户选位置，再写盘。返回最终路径以便提示/打开访达。
// 用户取消对话框时返回 null（调用方据此跳过 toast 与 reveal）。

export type ExportKind = 'md' | 'html' | 'json'

const EXPORT_FILTERS: Record<ExportKind, { name: string; extensions: string[] }> = {
  md: { name: 'Markdown', extensions: ['md'] },
  html: { name: 'HTML', extensions: ['html'] },
  json: { name: 'JSON', extensions: ['json'] },
}

async function pickAndWrite(
  content: string,
  defaultName: string,
  kind: ExportKind,
): Promise<string | null> {
  const chosen = await saveDialog({
    defaultPath: defaultName,
    filters: [EXPORT_FILTERS[kind]],
  })
  if (!chosen) return null
  return writeFile(chosen, content)
}

/** 无损 JSON 导出的信封：自包含（带 messages），可在任意机器上重新导入还原。
 *  `__type` 是导入端识别本格式的标记；`version` 留给以后格式演进。 */
export function buildExportEnvelope(
  session: SessionMeta,
  messages: Msg[],
  agent: Agent,
): string {
  return JSON.stringify(
    { __type: 'cc-session-viewer-export', version: 1, agent, session, messages },
    null,
    2,
  )
}

export function exportMarkdown(
  session: SessionMeta,
  messages: Msg[],
  agent: Agent,
): Promise<string | null> {
  const md = messagesToMarkdown(session, messages, agent)
  return pickAndWrite(md, `${sanitizeFilename(session.title)}.md`, 'md')
}

export async function exportHtml(
  session: SessionMeta,
  messages: Msg[],
  agent: Agent,
): Promise<string | null> {
  const html = await messagesToHtml(session, messages, agent)
  return pickAndWrite(html, `${sanitizeFilename(session.title)}.html`, 'html')
}

export function exportJson(
  session: SessionMeta,
  messages: Msg[],
  agent: Agent,
): Promise<string | null> {
  const json = buildExportEnvelope(session, messages, agent)
  return pickAndWrite(json, `${sanitizeFilename(session.title)}.json`, 'json')
}

// ============================ 批量导出 ============================
// 批量场景：让用户挑一个目标目录，所有会话以 `<title>-<id8>.<ext>` 落进去。
// 用 `/` 拼接：Rust 端走 `PathBuf::from`，Windows 也能接受正斜杠。

/** 弹原生 Open 目录选择器；取消返回 null。 */
export async function pickExportDir(): Promise<string | null> {
  const r = await openDialog({ directory: true, multiple: false })
  // open() 在「单选 + directory」下返回字符串或 null（与平台/插件版本相关）。
  return typeof r === 'string' ? r : null
}

/** 批量导出的子目录名：`export-YYYYMMDD-HHMMSS-<md|html>`。
 *  本地时间，便于人在 Finder 里直观分辨；多次导出不会撞名。
 *  `now` 形参只用于测试；生产路径走默认值 `new Date()`。 */
export function batchExportFolderName(kind: ExportKind, now: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  const dt = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`
  return `export-${dt}-${kind}`
}

/** 文件名：`<sanitized-title>-<id8>.<ext>`；标题相同的两条会话不会互相覆盖。 */
function batchFileName(session: SessionMeta, ext: ExportKind): string {
  const title = sanitizeFilename(session.title)
  const tag = (session.id || '').slice(0, 8) || 'session'
  return `${title}-${tag}.${ext}`
}

/** 把一条会话以 Markdown 写到目录里，返回最终绝对路径。 */
export async function exportMarkdownToDir(
  session: SessionMeta,
  messages: Msg[],
  agent: Agent,
  dir: string,
): Promise<string> {
  const md = messagesToMarkdown(session, messages, agent)
  return writeFile(`${dir}/${batchFileName(session, 'md')}`, md)
}

/** 把一条会话以 HTML 写到目录里，返回最终绝对路径。 */
export async function exportHtmlToDir(
  session: SessionMeta,
  messages: Msg[],
  agent: Agent,
  dir: string,
): Promise<string> {
  const html = await messagesToHtml(session, messages, agent)
  return writeFile(`${dir}/${batchFileName(session, 'html')}`, html)
}

/** 把一条会话以无损 JSON 写到目录里，返回最终绝对路径。 */
export async function exportJsonToDir(
  session: SessionMeta,
  messages: Msg[],
  agent: Agent,
  dir: string,
): Promise<string> {
  const json = buildExportEnvelope(session, messages, agent)
  return writeFile(`${dir}/${batchFileName(session, 'json')}`, json)
}
