import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { vTooltip } from '../../src/tooltip'
import { setLang } from '../../src/settings'

// 关键词搜索走后端：mock 掉，让规格用例驱动返回值。
// cancelSearch 在每次新输入时调一次，桩成 no-op 即可。
const { searchMock, cancelMock, usageMock } = vi.hoisted(() => ({
  searchMock: vi.fn(),
  cancelMock: vi.fn().mockResolvedValue(undefined),
  // SessionsView wires sessionUsage to an IntersectionObserver — our jsdom stub
  // never reports visibility, so the mock is mostly unused but must exist.
  usageMock: vi.fn().mockResolvedValue({
    inputTokens: 0,
    outputTokens: 0,
    cacheCreationInputTokens: 0,
    cacheReadInputTokens: 0,
    reasoningOutputTokens: 0,
    total: 0,
  }),
}))
let _id = 0
vi.mock('../../src/api', () => ({
  searchSessions: searchMock,
  cancelSearch: cancelMock,
  nextSearchRequestId: () => ++_id,
  sessionUsage: usageMock,
}))

import SessionsView from '../../src/views/SessionsView.vue'
import {
  resetSessionsToolbar,
  selectedSessions,
  sessionSearch,
  sessionSelectMode,
  sessionWithIdOnly,
} from '../../src/sessionsToolbar'
import type { ProjectInfo, SearchHit, SessionMeta } from '../../src/types'

// 每个 case 后卸载它挂载的 wrapper。否则旧实例的 watch 仍订阅 sessionSearch，
// 一旦设值，所有历史实例都会一起调 searchSessions，把 mockResolvedValueOnce
// 提前消费掉，当前 case 拿到 undefined → 渲染空列表。
enableAutoUnmount(afterEach)

beforeEach(() => {
  setLang('en')
  resetSessionsToolbar()
  searchMock.mockReset()
  cancelMock.mockClear()
  cancelMock.mockResolvedValue(undefined)
  _id = 0
})

// 防抖 ≥ 280ms + 后端 promise；给个 320ms 余量再 flush，等 visibleSessions 切到 searchHits。
async function waitForSearchSettle() {
  await new Promise((r) => setTimeout(r, 320))
  await flushPromises()
}

// 把元数据数组包成后端会返回的 SearchHit 形状。
const toHits = (sessions: SessionMeta[]): SearchHit[] =>
  sessions.map((s) => ({
    projectKey: 'proj',
    projectDisplay: '/work/proj',
    matchedField: 'title' as const,
    snippet: s.title,
    session: s,
  }))

const project: ProjectInfo = {
  dirName: 'proj',
  displayPath: '/work/proj',
  sessionCount: 1,
  lastModified: 0,
  exists: true,
}

const session = (over: Partial<SessionMeta> = {}): SessionMeta => ({
  id: 'sess-abcdef12',
  fileName: 's.jsonl',
  path: '/work/proj/s.jsonl',
  title: 'A session',
  modified: 0,
  size: 1024,
  messageCount: 3,
  codexAppListRank: null,
  codexAppListScanned: 0,
  codexAppFirstPageSize: 50,
  codexAppFirstPagePosition: 0,
  codexInternal: false,
  codexArchived: false,
  ...over,
})

type Props = InstanceType<typeof SessionsView>['$props']
const factory = (
  sessions: SessionMeta[] = [session()],
  overrides: Partial<Props> = {},
) =>
  mount(SessionsView, {
    props: {
      agent: 'claude',
      project,
      sessions,
      sessionTotal: sessions.length,
      loading: false,
      loadingMore: false,
      ...overrides,
    } as Props,
    global: { directives: { tooltip: vTooltip } },
  })

describe('SessionsView', () => {
  it('emits "open" when a session card is clicked', async () => {
    const wrapper = factory()
    await wrapper.find('.session-card').trigger('click')
    expect(wrapper.emitted('open')).toHaveLength(1)
  })

  it('shows Codex app-server first-screen position and rank in the metadata row', () => {
    setLang('zh')
    const wrapper = factory(
      [
        session({
          codexAppListRank: 64,
          codexAppListScanned: 1000,
          codexAppFirstPageSize: 50,
          codexAppFirstPagePosition: 0,
        } as Partial<SessionMeta>),
      ],
      { agent: 'codex' },
    )
    expect(wrapper.find('.session-meta').text()).toContain('首屏 0/50 · rank 64')
  })

  it('does not show Codex app-server rank metadata for non-Codex agents', () => {
    setLang('zh')
    const wrapper = factory([
      session({
        codexAppListRank: 64,
        codexAppListScanned: 1000,
        codexAppFirstPageSize: 50,
        codexAppFirstPagePosition: 0,
      } as Partial<SessionMeta>),
    ])
    expect(wrapper.find('.session-meta').text()).not.toContain('首屏 0/50')
    expect(wrapper.find('.session-meta').text()).not.toContain('rank 64')
  })

  it('shows rank placeholder for Codex sessions absent from the scanned app-server window', () => {
    setLang('zh')
    const wrapper = factory(
      [
        session({
          codexAppListRank: null,
          codexAppListScanned: 1000,
          codexAppFirstPageSize: 50,
          codexAppFirstPagePosition: 0,
        }),
      ],
      { agent: 'codex' },
    )
    expect(wrapper.find('.session-meta').text()).toContain('首屏 0/50 · rank -')
  })

  it('marks Codex internal guardian sessions instead of showing app rank', () => {
    setLang('zh')
    const wrapper = factory(
      [
        session({
          codexInternal: true,
          codexAppListRank: 1,
          codexAppListScanned: 50,
          codexAppFirstPagePosition: 1,
        }),
      ],
      { agent: 'codex' },
    )
    const tag = wrapper.find('.codex-special-tag')
    expect(tag.text()).toBe('审核会话')
    expect(wrapper.find('.session-meta').text()).not.toContain('首屏 1/50')
  })

  it('marks Codex archived sessions instead of showing app rank', () => {
    setLang('zh')
    const wrapper = factory(
      [
        session({
          codexArchived: true,
          codexAppListRank: 1,
          codexAppListScanned: 50,
          codexAppFirstPagePosition: 1,
        }),
      ],
      { agent: 'codex' },
    )
    const tag = wrapper.find('.codex-special-tag')
    expect(tag.text()).toBe('已归档会话')
    expect(wrapper.find('.session-meta').text()).not.toContain('首屏 1/50')
  })

  it('shows archived label before internal label when a Codex session has both flags', () => {
    setLang('zh')
    const wrapper = factory(
      [
        session({
          codexInternal: true,
          codexArchived: true,
          codexAppListRank: 1,
          codexAppListScanned: 50,
          codexAppFirstPagePosition: 1,
        }),
      ],
      { agent: 'codex' },
    )
    expect(wrapper.find('.codex-special-tag').text()).toBe('已归档会话')
  })

  it('opens the export menu without navigating into the session', async () => {
    const wrapper = factory()
    await wrapper.find('.export-menu-wrap .icon-btn').trigger('click')
    expect(wrapper.find('.export-menu').exists()).toBe(true)
    expect(wrapper.emitted('open')).toBeUndefined()
  })

  // Regression: clicking the menu's padding/gap (the container, not an item)
  // used to bubble to the .session-card and open the session.
  it('does not navigate when the export menu padding is clicked', async () => {
    const wrapper = factory()
    await wrapper.find('.export-menu-wrap .icon-btn').trigger('click')
    await wrapper.find('.export-menu').trigger('click')
    expect(wrapper.emitted('open')).toBeUndefined()
  })

  it('emits "export" — and not "open" — when a menu item is clicked', async () => {
    const wrapper = factory()
    await wrapper.find('.export-menu-wrap .icon-btn').trigger('click')
    await wrapper.findAll('.export-menu-item')[0].trigger('click')

    const exported = wrapper.emitted('export')
    expect(exported).toHaveLength(1)
    expect(exported![0][1]).toBe('md')
    expect(wrapper.emitted('open')).toBeUndefined()
  })

  describe('toolbar filters', () => {
    it('renders only the sessions returned by the backend search', async () => {
      const a = session({ path: 'a', title: 'Refactor parser' })
      const b = session({ path: 'b', title: 'Fix login bug' })
      searchMock.mockResolvedValueOnce(toHits([a]))
      const wrapper = factory([a, b])
      sessionSearch.value = 'parser'
      await waitForSearchSettle()
      expect(wrapper.findAll('.session-card')).toHaveLength(1)
      expect(wrapper.text()).toContain('Refactor parser')
      expect(searchMock).toHaveBeenCalledWith(
        'claude',
        'parser',
        expect.any(Number),
        'proj',
      )
    })

    it('shows the no-match state when filters exclude every session', () => {
      sessionWithIdOnly.value = true
      const wrapper = factory([session({ path: 'a', id: '' })])
      expect(wrapper.findAll('.session-card')).toHaveLength(0)
      expect(wrapper.text()).toContain('No sessions match')
    })

    it('shows the no-match state when the backend search returns nothing', async () => {
      searchMock.mockResolvedValueOnce([])
      const wrapper = factory([session({ path: 'a', title: 'Refactor parser' })])
      sessionSearch.value = 'zzznoop'
      await waitForSearchSettle()
      expect(wrapper.findAll('.session-card')).toHaveLength(0)
      expect(wrapper.text()).toContain('No sessions match')
    })

    it('keeps the project-empty state separate from the no-match state', () => {
      expect(factory([]).text()).toContain('No sessions in this project')
    })
  })

  describe('keyword highlight', () => {
    it('wraps the matched keyword in the title in a .kw-hit', async () => {
      const s = session({ path: 'a', title: 'workflow with obsidian' })
      searchMock.mockResolvedValueOnce(toHits([s]))
      const wrapper = factory([s])
      sessionSearch.value = 'obsidian'
      await waitForSearchSettle()
      const hits = wrapper.findAll('.session-title-text .kw-hit')
      expect(hits).toHaveLength(1)
      expect(hits[0].text()).toBe('obsidian')
    })

    it('highlights a match in the session ID', async () => {
      const s = session({ path: 'a', title: 'no match here', id: 'abcdef12' })
      searchMock.mockResolvedValueOnce(toHits([s]))
      const wrapper = factory([s])
      sessionSearch.value = 'abcd'
      await waitForSearchSettle()
      const hits = wrapper.findAll('.session-id-text .kw-hit')
      expect(hits).toHaveLength(1)
      expect(hits[0].text()).toBe('abcd')
    })

    it('renders no highlight when there is no active search', () => {
      const wrapper = factory([
        session({ path: 'a', title: 'workflow with obsidian' }),
      ])
      expect(wrapper.find('.kw-hit').exists()).toBe(false)
      // 标题文本仍完整无缺
      expect(wrapper.find('.session-title-text').text()).toBe('workflow with obsidian')
    })
  })

  describe('header actions', () => {
    it('emits "new-session" when the new-session button is clicked', async () => {
      const wrapper = factory()
      await wrapper.find('.list-head-actions .icon-btn').trigger('click')
      expect(wrapper.emitted('new-session')).toHaveLength(1)
    })

    it('hides new-session and refresh when the project directory is missing', () => {
      const wrapper = mount(SessionsView, {
        props: {
          project: { ...project, exists: false },
          sessions: [],
          sessionTotal: 0,
          loading: false,
          loadingMore: false,
        } as Props,
        global: { directives: { tooltip: vTooltip } },
      })
      // 目录已不存在 → 新建会话 / 刷新都没意义，只剩删除项目
      expect(wrapper.findAll('.list-head-actions .icon-btn')).toHaveLength(1)
    })

    it('emits "refresh" when the header refresh button is clicked', async () => {
      const wrapper = factory()
      const buttons = wrapper.findAll('.list-head-actions .icon-btn')
      await buttons[1].trigger('click')
      expect(wrapper.emitted('refresh')).toHaveLength(1)
    })

    it('emits "delete-project" when the header delete button is clicked', async () => {
      const wrapper = factory()
      const buttons = wrapper.findAll('.list-head-actions .icon-btn')
      await buttons[2].trigger('click')
      expect(wrapper.emitted('delete-project')).toHaveLength(1)
    })
  })

  describe('missing-directory tag', () => {
    it('shows the tag when the project directory no longer exists', () => {
      const wrapper = mount(SessionsView, {
        props: {
          project: { ...project, exists: false },
          sessions: [],
          sessionTotal: 0,
          loading: false,
          loadingMore: false,
        } as Props,
        global: { directives: { tooltip: vTooltip } },
      })
      expect(wrapper.find('.dir-missing-tag').exists()).toBe(true)
    })

    it('hides the tag when the directory exists', () => {
      expect(factory().find('.dir-missing-tag').exists()).toBe(false)
    })

    // 目录已不存在 → 恢复 / 刷新 这些依赖项目目录的卡片操作没有意义，隐藏。
    // 重命名只动 ~/.claude/projects 下的 JSONL，与项目目录无关 —— 保留。
    it('hides resume and refresh on session cards when the directory is missing', () => {
      const wrapper = mount(SessionsView, {
        props: {
          project: { ...project, exists: false },
          sessions: [session()],
          sessionTotal: 1,
          loading: false,
          loadingMore: false,
        } as Props,
        global: { directives: { tooltip: vTooltip } },
      })
      expect(wrapper.find('.title-rename-ic').exists()).toBe(true)
      // 只剩 在文件管理器中显示 / 导出 / 删除
      expect(wrapper.findAll('.session-actions .icon-btn')).toHaveLength(3)
    })

    it('keeps every card action when the directory exists', () => {
      const wrapper = factory()
      expect(wrapper.find('.title-rename-ic').exists()).toBe(true)
      expect(wrapper.findAll('.session-actions .icon-btn')).toHaveLength(5)
    })
  })

  describe('select mode', () => {
    it('shows a checkbox on each card and hides the row actions', () => {
      sessionSelectMode.value = true
      const wrapper = factory([session({ path: 'a' })])
      expect(wrapper.find('.list-check').exists()).toBe(true)
      expect(wrapper.find('.session-actions').exists()).toBe(false)
      expect(wrapper.find('.title-rename-ic').exists()).toBe(false)
      expect(wrapper.find('.session-id-copy').exists()).toBe(false)
    })

    it('toggles selection — and does not open — when a card is clicked', async () => {
      sessionSelectMode.value = true
      const wrapper = factory([session({ path: 'a' })])

      await wrapper.find('.session-card').trigger('click')
      expect(selectedSessions.value.has('a')).toBe(true)
      expect(wrapper.emitted('open')).toBeUndefined()

      await wrapper.find('.session-card').trigger('click')
      expect(selectedSessions.value.has('a')).toBe(false)
    })

    it('marks the row as selected via the list-selected class', async () => {
      sessionSelectMode.value = true
      selectedSessions.value = new Set(['a'])
      const wrapper = factory([session({ path: 'a' })])
      expect(wrapper.find('.session-card').classes()).toContain('list-selected')
    })
  })
})
