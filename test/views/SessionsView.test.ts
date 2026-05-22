import { beforeEach, describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import SessionsView from '../../src/views/SessionsView.vue'
import { vTooltip } from '../../src/tooltip'
import { setLang } from '../../src/settings'
import {
  resetSessionsToolbar,
  sessionSearch,
  sessionWithIdOnly,
} from '../../src/sessionsToolbar'
import type { ProjectInfo, SessionMeta } from '../../src/types'

beforeEach(() => {
  setLang('en')
  resetSessionsToolbar()
})

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
  ...over,
})

type Props = InstanceType<typeof SessionsView>['$props']
const factory = (sessions: SessionMeta[] = [session()]) =>
  mount(SessionsView, {
    props: {
      project,
      sessions,
      sessionTotal: sessions.length,
      loading: false,
      loadingMore: false,
    } as Props,
    global: { directives: { tooltip: vTooltip } },
  })

describe('SessionsView', () => {
  it('emits "open" when a session card is clicked', async () => {
    const wrapper = factory()
    await wrapper.find('.session-card').trigger('click')
    expect(wrapper.emitted('open')).toHaveLength(1)
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
    it('renders only the sessions matching the search term', () => {
      sessionSearch.value = 'parser'
      const wrapper = factory([
        session({ path: 'a', title: 'Refactor parser' }),
        session({ path: 'b', title: 'Fix login bug' }),
      ])
      expect(wrapper.findAll('.session-card')).toHaveLength(1)
      expect(wrapper.text()).toContain('Refactor parser')
    })

    it('shows the no-match state when filters exclude every session', () => {
      sessionWithIdOnly.value = true
      const wrapper = factory([session({ path: 'a', id: '' })])
      expect(wrapper.findAll('.session-card')).toHaveLength(0)
      expect(wrapper.text()).toContain('No sessions match')
    })

    it('keeps the project-empty state separate from the no-match state', () => {
      expect(factory([]).text()).toContain('No sessions in this project')
    })
  })

  describe('keyword highlight', () => {
    it('wraps the matched keyword in the title in a .kw-hit', () => {
      sessionSearch.value = 'obsidian'
      const wrapper = factory([
        session({ path: 'a', title: 'workflow with obsidian' }),
      ])
      const hits = wrapper.findAll('.session-title-text .kw-hit')
      expect(hits).toHaveLength(1)
      expect(hits[0].text()).toBe('obsidian')
    })

    it('highlights a match in the session ID', () => {
      sessionSearch.value = 'abcd'
      const wrapper = factory([
        session({ path: 'a', title: 'no match here', id: 'abcdef12' }),
      ])
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
})
