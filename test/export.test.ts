import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Block, Msg, SessionMeta } from '../src/types'

// Tauri's save dialog and the filesystem command are unavailable in jsdom —
// stub them so the落盘 path (exportMarkdown / exportHtml) is testable.
const { saveMock, writeFileMock } = vi.hoisted(() => ({
  saveMock: vi.fn(),
  writeFileMock: vi.fn(),
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: saveMock }))
vi.mock('../src/api', () => ({ writeFile: writeFileMock }))

import {
  exportHtml,
  exportMarkdown,
  messagesToHtml,
  messagesToMarkdown,
} from '../src/export'
import { setLang } from '../src/settings'

beforeEach(() => {
  setLang('en')
  saveMock.mockReset()
  writeFileMock.mockReset()
})
afterEach(() => {
  document.documentElement.classList.remove('theme-dark')
})

// ---- factories -----------------------------------------------------------
function blk(over: Partial<Block> & { kind: Block['kind'] }): Block {
  return { isError: false, ...over }
}
function msg(
  role: Msg['role'],
  blocks: Block[],
  over: Partial<Msg> = {},
): Msg {
  return { role, sidechain: false, blocks, ...over }
}
function session(over: Partial<SessionMeta> = {}): SessionMeta {
  return {
    id: 'sess-1',
    fileName: 's.jsonl',
    path: '/p/s.jsonl',
    title: 'My Session',
    modified: 0,
    size: 100,
    messageCount: 5,
    ...over,
  }
}

const text = (t: string) => blk({ kind: 'text', text: t })

describe('messagesToMarkdown', () => {
  it('emits a title heading and the meta block', () => {
    const md = messagesToMarkdown(session({ cwd: '/work', id: 'abc' }), [], 'claude')
    expect(md).toContain('# My Session')
    expect(md).toContain('- Agent: `claude`')
    expect(md).toContain('- cwd: `/work`')
    expect(md).toContain('- ID: `abc`')
    expect(md).toContain('\n---\n')
  })

  it('omits the cwd and id lines when they are absent', () => {
    const md = messagesToMarkdown(session({ cwd: undefined, id: '' }), [], 'claude')
    expect(md).not.toContain('cwd:')
    expect(md).not.toContain('ID:')
  })

  it('renders a user text block under a "Me" heading', () => {
    const md = messagesToMarkdown(session(), [msg('user', [text('Hello world')])], 'claude')
    expect(md).toContain('## Me')
    expect(md).toContain('Hello world')
  })

  it('renders a thinking block inside a <details> element', () => {
    const md = messagesToMarkdown(
      session(),
      [msg('assistant', [blk({ kind: 'thinking', text: 'pondering' })])],
      'claude',
    )
    expect(md).toContain('<summary>🧠 Thinking</summary>')
    expect(md).toContain('pondering')
  })

  it('renders a tool_use with its JSON arguments', () => {
    const md = messagesToMarkdown(
      session(),
      [msg('assistant', [blk({ kind: 'tool_use', toolName: 'Read', toolInput: '{"file":"x"}' })])],
      'claude',
    )
    expect(md).toContain('Tool call · Read')
    expect(md).toContain('```json')
    expect(md).toContain('{"file":"x"}')
  })

  it('inlines a non-file-mutating tool_result under its tool_use', () => {
    const messages = [
      msg('assistant', [blk({ kind: 'tool_use', toolName: 'Read', toolId: 't1', toolInput: '{}' })]),
      msg('user', [blk({ kind: 'tool_result', toolId: 't1', text: 'file body' })]),
    ]
    const md = messagesToMarkdown(session(), messages, 'claude')
    expect(md).toContain('file body')
    // the tool-result message is fully absorbed — no standalone "## Tool"
    expect(md).not.toContain('## Tool')
  })

  it('renders a file-mutating tool_result as its own diff block', () => {
    const messages = [
      msg('assistant', [blk({ kind: 'tool_use', toolName: 'Write', toolId: 't2', toolInput: '{}' })]),
      msg('user', [
        blk({
          kind: 'tool_result',
          toolId: 't2',
          filePath: '/x/y.ts',
          diff: [
            {
              oldStart: 1,
              newStart: 1,
              lines: [
                { kind: 'ctx', oldNo: 1, newNo: 1, text: 'keep' },
                { kind: 'add', oldNo: null, newNo: 2, text: 'added' },
                { kind: 'del', oldNo: 2, newNo: null, text: 'removed' },
              ],
            },
          ],
        }),
      ]),
    ]
    const md = messagesToMarkdown(session(), messages, 'claude')
    expect(md).toContain('File change · /x/y.ts')
    expect(md).toContain('```diff')
    expect(md).toContain('+added')
    expect(md).toContain('-removed')
  })

  it('marks an error tool_result', () => {
    const md = messagesToMarkdown(
      session(),
      [msg('assistant', [blk({ kind: 'tool_result', text: 'boom', isError: true })])],
      'claude',
    )
    expect(md).toContain('Tool result · error')
  })

  it('renders an image block', () => {
    const md = messagesToMarkdown(
      session(),
      [msg('user', [blk({ kind: 'image', imageSrc: 'data:image/png;base64,AAA' })])],
      'claude',
    )
    expect(md).toContain('![image](data:image/png;base64,AAA)')
  })

  it('drops local-command-caveat messages', () => {
    const messages = [
      msg('user', [text('<local-command-caveat>noise</local-command-caveat>')]),
      msg('user', [text('real prompt')]),
    ]
    const md = messagesToMarkdown(session(), messages, 'claude')
    expect(md).not.toContain('noise')
    expect(md).toContain('real prompt')
  })

  it('renders a /rename system event as an italic line', () => {
    const messages = [
      msg('user', [
        text('<system-reminder>The user named this session "新名字". x</system-reminder>'),
      ]),
    ]
    const md = messagesToMarkdown(session(), messages, 'claude')
    expect(md).toContain('_User renamed this session to "新名字"')
  })

  it('labels a tool-only user message as "Tool"', () => {
    const messages = [
      msg('assistant', [blk({ kind: 'tool_use', toolName: 'Write', toolId: 't9', toolInput: '{}' })]),
      msg('user', [blk({ kind: 'tool_result', toolId: 't9', filePath: '/a.ts', text: 'done' })]),
    ]
    const md = messagesToMarkdown(session(), messages, 'claude')
    expect(md).toContain('## Tool')
  })

  it('counts prompts and replies in the stats line', () => {
    const messages = [
      msg('user', [text('q1')]),
      msg('assistant', [text('a1')]),
      msg('assistant', [text('a2')]),
      msg('user', [text('<local-command-caveat>x</local-command-caveat>')]),
    ]
    const md = messagesToMarkdown(session(), messages, 'claude')
    expect(md).toContain('1 prompts · 2 replies')
  })

  it('uses the agent-specific assistant label', () => {
    const md = messagesToMarkdown(session(), [msg('assistant', [text('hi')])], 'codex')
    expect(md).toContain('## Codex')
    expect(md).toContain('- Agent: `codex`')
  })
})

describe('messagesToHtml', () => {
  it('produces a full HTML document', () => {
    const html = messagesToHtml(session(), [], 'claude')
    expect(html.startsWith('<!doctype html>')).toBe(true)
    expect(html).toContain('<title>My Session</title>')
    expect(html).toContain('</html>')
  })

  it('escapes HTML-significant characters in the title', () => {
    const html = messagesToHtml(session({ title: '<script>' }), [], 'claude')
    expect(html).toContain('<title>&lt;script&gt;</title>')
  })

  it('wraps a user message body in a collapsible box', () => {
    const html = messagesToHtml(session(), [msg('user', [text('hi')])], 'claude')
    expect(html).toContain('<div class="msg user">')
    expect(html).toContain('collapsible-box')
  })

  it('converts newlines in assistant text to <br>', () => {
    const html = messagesToHtml(session(), [msg('assistant', [text('a\nb')])], 'claude')
    expect(html).toContain('a<br>b')
  })

  it('renders a file-change result as an open <details>', () => {
    const messages = [
      msg('assistant', [
        blk({ kind: 'tool_result', filePath: '/f.ts', text: 'patched' }),
      ]),
    ]
    const html = messagesToHtml(session(), messages, 'claude')
    expect(html).toContain('<details open>')
    expect(html).toContain('<code>/f.ts</code>')
  })

  it('renders a thinking block as a <details> element', () => {
    const html = messagesToHtml(
      session(),
      [msg('assistant', [blk({ kind: 'thinking', text: 'reasoning' })])],
      'claude',
    )
    expect(html).toContain('🧠')
    expect(html).toContain('<details><summary>')
    expect(html).toContain('reasoning')
  })

  it('renders a tool_use with its arguments', () => {
    const html = messagesToHtml(
      session(),
      [msg('assistant', [blk({ kind: 'tool_use', toolName: 'Bash', toolInput: 'ls -la' })])],
      'claude',
    )
    expect(html).toContain('🔧')
    expect(html).toContain('Tool call · Bash')
    expect(html).toContain('ls -la')
  })

  it('inlines a non-file-mutating tool_result inside its tool_use', () => {
    const messages = [
      msg('assistant', [blk({ kind: 'tool_use', toolName: 'Read', toolId: 'r1', toolInput: '{}' })]),
      msg('user', [blk({ kind: 'tool_result', toolId: 'r1', text: 'file contents' })]),
    ]
    const html = messagesToHtml(session(), messages, 'claude')
    expect(html).toContain('tool-result-inline')
    expect(html).toContain('file contents')
  })

  it('renders a structured diff result with add/del rows', () => {
    const messages = [
      msg('assistant', [
        blk({
          kind: 'tool_result',
          filePath: '/d.ts',
          diff: [
            {
              oldStart: 2,
              newStart: 2,
              lines: [
                { kind: 'ctx', oldNo: 2, newNo: 2, text: 'unchanged' },
                { kind: 'add', oldNo: null, newNo: 3, text: 'new line' },
                { kind: 'del', oldNo: 3, newNo: null, text: 'old line' },
              ],
            },
          ],
        }),
      ]),
    ]
    const html = messagesToHtml(session(), messages, 'claude')
    expect(html).toContain('<div class="diff">')
    expect(html).toContain('<span class="add">+new line</span>')
    expect(html).toContain('<span class="del">-old line</span>')
  })

  it('marks a standalone error tool_result', () => {
    const html = messagesToHtml(
      session(),
      [msg('assistant', [blk({ kind: 'tool_result', text: 'failure', isError: true })])],
      'claude',
    )
    expect(html).toContain('⚠️')
    expect(html).toContain('Tool result · error')
  })

  it('labels a tool-only user message with the tool avatar', () => {
    const messages = [
      msg('assistant', [blk({ kind: 'tool_use', toolName: 'Write', toolId: 'w1', toolInput: '{}' })]),
      msg('user', [blk({ kind: 'tool_result', toolId: 'w1', filePath: '/a.ts', text: 'done' })]),
    ]
    const html = messagesToHtml(session(), messages, 'claude')
    expect(html).toContain('<div class="msg tool">')
  })

  it('renders an image as an <img> tag', () => {
    const html = messagesToHtml(
      session(),
      [msg('user', [blk({ kind: 'image', imageSrc: 'data:x' })])],
      'claude',
    )
    expect(html).toContain('<img src="data:x"')
  })

  it('drops local-command-caveat messages', () => {
    const html = messagesToHtml(
      session(),
      [msg('user', [text('<local-command-caveat>noise</local-command-caveat>')])],
      'claude',
    )
    expect(html).not.toContain('noise')
  })

  it('renders a system event as a centered row', () => {
    const html = messagesToHtml(
      session(),
      [msg('user', [text('<system-reminder>The user named this session "X". y</system-reminder>')])],
      'claude',
    )
    expect(html).toContain('<div class="msg system">')
  })

  it('reflects the active theme in the data-theme attribute', () => {
    expect(messagesToHtml(session(), [], 'claude')).toContain('data-theme="light"')
    document.documentElement.classList.add('theme-dark')
    expect(messagesToHtml(session(), [], 'claude')).toContain('data-theme="dark"')
  })
})

describe('exportMarkdown / exportHtml', () => {
  it('returns null when the save dialog is cancelled', async () => {
    saveMock.mockResolvedValue(null)
    const result = await exportMarkdown(session(), [], 'claude')
    expect(result).toBeNull()
    expect(writeFileMock).not.toHaveBeenCalled()
  })

  it('writes the markdown file and returns the final path', async () => {
    saveMock.mockResolvedValue('/Users/me/out.md')
    writeFileMock.mockResolvedValue('/Users/me/out.md')
    const result = await exportMarkdown(session(), [msg('user', [text('hi')])], 'claude')
    expect(result).toBe('/Users/me/out.md')
    expect(writeFileMock).toHaveBeenCalledWith('/Users/me/out.md', expect.stringContaining('# My Session'))
  })

  it('writes the html file and returns the final path', async () => {
    saveMock.mockResolvedValue('/Users/me/out.html')
    writeFileMock.mockResolvedValue('/Users/me/out.html')
    const result = await exportHtml(session(), [], 'claude')
    expect(result).toBe('/Users/me/out.html')
    expect(writeFileMock).toHaveBeenCalledWith('/Users/me/out.html', expect.stringContaining('<!doctype html>'))
  })

  it('sanitizes illegal characters out of the default filename', async () => {
    saveMock.mockResolvedValue(null)
    await exportMarkdown(session({ title: 'a/b:c*?' }), [], 'claude')
    expect(saveMock.mock.calls[0][0].defaultPath).toBe('a_b_c__.md')
  })

  it('falls back to "session" when the title sanitizes to empty', async () => {
    saveMock.mockResolvedValue(null)
    await exportMarkdown(session({ title: '   ' }), [], 'claude')
    expect(saveMock.mock.calls[0][0].defaultPath).toBe('session.md')
  })
})
