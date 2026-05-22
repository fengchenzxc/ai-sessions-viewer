import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  formatSize,
  formatTime,
  highlightSegments,
  isCaveatOnlyMsg,
  parseSystemEvent,
  renderText,
  shortName,
} from '../src/format'
import { setLang } from '../src/settings'

// format.ts pulls localized strings via t(); pin the language so assertions
// don't depend on the host machine's locale.
beforeEach(() => setLang('en'))

// Convenience builders for the structural-message shapes the helpers accept.
const block = (kind: string, text?: string) => ({ kind, text })
const userMsg = (...blocks: Array<{ kind: string; text?: string }>) => ({
  role: 'user',
  blocks,
})

describe('renderText', () => {
  it('escapes HTML special characters', () => {
    expect(renderText('<b> & </b>')).toContain('&lt;b&gt; &amp; &lt;/b&gt;')
  })

  it('renders inline code, bold and headings', () => {
    expect(renderText('`code`')).toContain('<code>code</code>')
    expect(renderText('**bold**')).toContain('<strong>bold</strong>')
    expect(renderText('# Title')).toContain('<h3>Title</h3>')
    expect(renderText('## Sub')).toContain('<h3>Sub</h3>')
    expect(renderText('### Deep')).toContain('<h4>Deep</h4>')
  })

  it('converts newlines inside a text run to <br>', () => {
    expect(renderText('line1\nline2')).toContain('line1<br>line2')
  })

  it('renders a fenced code block with a language line', () => {
    const html = renderText('```js\nconst x = 1\n```')
    expect(html).toContain('<pre class="code-block"><code>const x = 1</code></pre>')
  })

  it('renders a fenced code block with no language line', () => {
    expect(renderText('```\nplain\n```')).toContain('<code>plain</code>')
  })

  it('escapes HTML inside fenced code blocks', () => {
    expect(renderText('```\n<a> & b\n```')).toContain('&lt;a&gt; &amp; b')
  })

  it('wraps plain prose in a text-run div', () => {
    expect(renderText('hello')).toBe('<div class="text-run">hello</div>')
  })

  it('drops <command-message> and emits <command-name> as a code chip', () => {
    const html = renderText(
      '<command-message>init</command-message><command-name>/init</command-name>',
    )
    expect(html).not.toContain('command-message')
    expect(html).toContain('<code class="cmd-tag">/init</code>')
  })

  it('emits <command-args> as a code chip and escapes its content', () => {
    const html = renderText('<command-args><x></command-args>')
    expect(html).toContain('<code class="cmd-tag">&lt;x&gt;</code>')
  })

  it('returns an empty string for empty input', () => {
    expect(renderText('')).toBe('')
  })
})

describe('isCaveatOnlyMsg', () => {
  it('is true when every block is a local-command-caveat', () => {
    expect(
      isCaveatOnlyMsg(
        userMsg(block('text', '<local-command-caveat>x</local-command-caveat>')),
      ),
    ).toBe(true)
  })

  it('tolerates surrounding whitespace', () => {
    expect(
      isCaveatOnlyMsg(
        userMsg(block('text', '  \n<local-command-caveat>x</local-command-caveat>\n ')),
      ),
    ).toBe(true)
  })

  it('is false for non-user roles', () => {
    expect(
      isCaveatOnlyMsg({
        role: 'assistant',
        blocks: [block('text', '<local-command-caveat>x</local-command-caveat>')],
      }),
    ).toBe(false)
  })

  it('is false when the message has no blocks', () => {
    expect(isCaveatOnlyMsg(userMsg())).toBe(false)
  })

  it('is false when prose accompanies the caveat', () => {
    expect(
      isCaveatOnlyMsg(
        userMsg(block('text', 'hi <local-command-caveat>x</local-command-caveat>')),
      ),
    ).toBe(false)
  })

  it('is false when a non-text block is present', () => {
    expect(
      isCaveatOnlyMsg(
        userMsg(
          block('text', '<local-command-caveat>x</local-command-caveat>'),
          block('image'),
        ),
      ),
    ).toBe(false)
  })
})

describe('parseSystemEvent', () => {
  it('parses a /rename system reminder', () => {
    const ev = parseSystemEvent(
      userMsg(
        block(
          'text',
          '<system-reminder>\nThe user named this session "批量导入". More.\n</system-reminder>',
        ),
      ),
    )
    expect(ev).toEqual({ kind: 'rename', name: '批量导入' })
  })

  it('returns null for non-user roles', () => {
    expect(
      parseSystemEvent({
        role: 'assistant',
        blocks: [block('text', '<system-reminder>The user named this session "x"</system-reminder>')],
      }),
    ).toBeNull()
  })

  it('returns null when there is more than one block', () => {
    expect(
      parseSystemEvent(
        userMsg(
          block('text', '<system-reminder>The user named this session "x"</system-reminder>'),
          block('text', 'extra'),
        ),
      ),
    ).toBeNull()
  })

  it('returns null when prose surrounds the reminder', () => {
    expect(
      parseSystemEvent(
        userMsg(block('text', 'hello <system-reminder>The user named this session "x"</system-reminder>')),
      ),
    ).toBeNull()
  })

  it('returns null for an unrecognized reminder', () => {
    expect(
      parseSystemEvent(userMsg(block('text', '<system-reminder>some other note</system-reminder>'))),
    ).toBeNull()
  })

  it('returns null when there is no reminder at all', () => {
    expect(parseSystemEvent(userMsg(block('text', 'plain message')))).toBeNull()
  })
})

describe('formatSize', () => {
  it('formats bytes below 1 KiB', () => {
    expect(formatSize(0)).toBe('0 B')
    expect(formatSize(1023)).toBe('1023 B')
  })

  it('formats kibibytes with one decimal', () => {
    expect(formatSize(1024)).toBe('1.0 KB')
    expect(formatSize(1536)).toBe('1.5 KB')
  })

  it('formats mebibytes with one decimal', () => {
    expect(formatSize(1024 * 1024)).toBe('1.0 MB')
    expect(formatSize(2.5 * 1024 * 1024)).toBe('2.5 MB')
  })
})

describe('formatTime', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 4, 22, 15, 0, 0))
  })
  afterEach(() => vi.useRealTimers())

  it('returns an em dash for missing or empty input', () => {
    expect(formatTime(undefined)).toBe('—')
    expect(formatTime('')).toBe('—')
  })

  it('returns an em dash for an unparseable value', () => {
    expect(formatTime('not-a-date')).toBe('—')
    expect(formatTime(NaN)).toBe('—')
  })

  it('labels a same-day timestamp as Today', () => {
    expect(formatTime(new Date(2026, 4, 22, 9, 5).getTime())).toBe('Today 09:05')
  })

  it('labels the previous calendar day as Yesterday', () => {
    expect(formatTime(new Date(2026, 4, 21, 23, 59).getTime())).toBe('Yesterday 23:59')
  })

  it('formats older timestamps as YYYY-MM-DD HH:MM', () => {
    expect(formatTime(new Date(2026, 0, 3, 8, 7).getTime())).toBe('2026-01-03 08:07')
  })
})

describe('shortName', () => {
  it('returns the last path segment', () => {
    expect(shortName('/Users/me/apps/viewer')).toBe('viewer')
  })

  it('ignores a trailing slash', () => {
    expect(shortName('/Users/me/apps/viewer/')).toBe('viewer')
  })

  it('returns the input unchanged when there is no separator', () => {
    expect(shortName('viewer')).toBe('viewer')
  })

  it('returns the input for an empty string', () => {
    expect(shortName('')).toBe('')
  })
})

describe('highlightSegments', () => {
  it('returns a single non-hit segment when the query is empty', () => {
    expect(highlightSegments('workflow with obsidian', '')).toEqual([
      { text: 'workflow with obsidian', hit: false },
    ])
  })

  it('splits a single match into before / hit / after', () => {
    expect(highlightSegments('workflow with obsidian', 'obsidian')).toEqual([
      { text: 'workflow with ', hit: false },
      { text: 'obsidian', hit: true },
    ])
  })

  it('matches case-insensitively but keeps the original casing in the hit', () => {
    expect(highlightSegments('Obsidian Notes', 'obsidian')).toEqual([
      { text: 'Obsidian', hit: true },
      { text: ' Notes', hit: false },
    ])
  })

  it('highlights every occurrence', () => {
    expect(highlightSegments('aXaXa', 'a').filter((s) => s.hit)).toHaveLength(3)
  })

  it('treats regex-special characters literally', () => {
    expect(highlightSegments('a.b.c', '.')).toEqual([
      { text: 'a', hit: false },
      { text: '.', hit: true },
      { text: 'b', hit: false },
      { text: '.', hit: true },
      { text: 'c', hit: false },
    ])
  })

  it('returns one non-hit segment when there is no match', () => {
    expect(highlightSegments('hello', 'zzz')).toEqual([{ text: 'hello', hit: false }])
  })

  it('reproduces the original text when the segments are joined', () => {
    const text = 'fix the obsidian sync bug in obsidian'
    const joined = highlightSegments(text, 'obsidian')
      .map((s) => s.text)
      .join('')
    expect(joined).toBe(text)
  })

  it('ignores a whitespace-only query', () => {
    expect(highlightSegments('hello', '   ')).toEqual([{ text: 'hello', hit: false }])
  })
})
