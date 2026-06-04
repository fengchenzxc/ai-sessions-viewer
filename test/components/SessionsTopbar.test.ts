import { beforeEach, describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import SessionsTopbar from '../../src/components/topbar/SessionsTopbar.vue'
import { vTooltip } from '../../src/tooltip'
import { setLang } from '../../src/settings'
import {
  resetSessionsToolbar,
  selectedSessions,
  sessionSearch,
  sessionSelectMode,
  sessionSort,
  sessionWithIdOnly,
} from '../../src/sessionsToolbar'
import type { SessionMeta } from '../../src/types'

beforeEach(() => {
  setLang('en')
  resetSessionsToolbar()
})

const session = (over: Partial<SessionMeta> = {}): SessionMeta => ({
  path: '/p/a.jsonl',
  fileName: 'a.jsonl',
  id: 'aaaa1111-bbbb-2222-cccc-333344445555',
  title: 'A session',
  cwd: '/p',
  size: 100,
  messageCount: 1,
  modified: 0,
  codexAppListRank: null,
  codexAppListScanned: 0,
  codexAppFirstPageSize: 50,
  codexAppFirstPagePosition: 0,
  codexInternal: false,
  codexArchived: false,
  ...over,
})

const factory = (sessions: SessionMeta[] = [session(), session({ path: '/p/b.jsonl' })]) =>
  mount(SessionsTopbar, {
    props: { sessions },
    global: { directives: { tooltip: vTooltip } },
  })

describe('SessionsTopbar', () => {
  it('binds the search box to the shared search ref (debounced)', async () => {
    const wrapper = factory()
    await wrapper.find('.ct-search-input').setValue('parser')
    // 防抖：打字立即落到本地 draft，~220ms 后才同步到共享 ref
    expect(sessionSearch.value).toBe('')
    await new Promise((r) => setTimeout(r, 280))
    expect(sessionSearch.value).toBe('parser')
  })

  it('clears the search from the clear button', async () => {
    sessionSearch.value = 'parser'
    const wrapper = factory()
    await wrapper.find('.ct-search .ct-btn').trigger('click')
    expect(sessionSearch.value).toBe('')
  })

  it('lists the four sort options and applies a pick', async () => {
    const wrapper = factory()
    await wrapper.find('.ct-scope-btn').trigger('click')
    const items = wrapper.findAll('.ct-scope-item')
    expect(items).toHaveLength(4)

    await items[2].trigger('click') // 'Largest first'
    expect(sessionSort.value).toBe('size')
  })

  it('toggles the with-id filter from the hash button', async () => {
    const wrapper = factory()
    expect(sessionWithIdOnly.value).toBe(false)
    await wrapper.find('.ct-actions .ct-btn').trigger('click')
    expect(sessionWithIdOnly.value).toBe(true)
    await wrapper.find('.ct-actions .ct-btn').trigger('click')
    expect(sessionWithIdOnly.value).toBe(false)
  })

  it('marks the with-id button active while the filter is on', async () => {
    const wrapper = factory()
    expect(wrapper.find('.ct-actions .ct-btn').classes()).not.toContain('active')
    sessionWithIdOnly.value = true
    await wrapper.vm.$nextTick()
    expect(wrapper.find('.ct-actions .ct-btn').classes()).toContain('active')
  })

  it('focuses the search box on the ⌘F / Ctrl+F shortcut', () => {
    const wrapper = mount(SessionsTopbar, {
      props: { sessions: [session(), session({ path: '/p/b.jsonl' })] },
      global: { directives: { tooltip: vTooltip } },
      attachTo: document.body,
    })
    const isMac = /Mac/i.test(navigator.platform)
    window.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'f', metaKey: isMac, ctrlKey: !isMac }),
    )
    expect(document.activeElement).toBe(wrapper.find('.ct-search-input').element)
    wrapper.unmount()
  })

  describe('select mode', () => {
    it('shows the "select multiple" entry only when there are 2+ sessions', () => {
      const w1 = factory([session()])
      // Only the with-id hash button is rendered.
      expect(w1.findAll('.ct-actions .ct-btn')).toHaveLength(1)
      const w2 = factory()
      expect(w2.findAll('.ct-actions .ct-btn')).toHaveLength(2)
    })

    it('flips into select mode from the entry button', async () => {
      const wrapper = factory()
      // The 2nd action button is the "select multiple" entry.
      await wrapper.findAll('.ct-actions .ct-btn')[1].trigger('click')
      expect(sessionSelectMode.value).toBe(true)
    })

    it('renders the count, select-all, export, delete and cancel controls', () => {
      sessionSelectMode.value = true
      selectedSessions.value = new Set(['/p/a.jsonl'])
      const wrapper = factory()
      expect(wrapper.find('.ct-search-count').text()).toBe('1 selected')
      // Select-all + export + delete + cancel = 4 buttons.
      expect(wrapper.findAll('.ct-actions > .ct-btn')).toHaveLength(3)
      expect(wrapper.find('.export-menu-wrap .ct-btn').exists()).toBe(true)
    })

    it('emits batch-delete from the danger button', async () => {
      sessionSelectMode.value = true
      selectedSessions.value = new Set(['/p/a.jsonl'])
      const wrapper = factory()
      await wrapper.find('.ct-actions .ct-btn.danger').trigger('click')
      expect(wrapper.emitted('batch-delete')).toHaveLength(1)
    })

    it('emits batch-export with the picked format', async () => {
      sessionSelectMode.value = true
      selectedSessions.value = new Set(['/p/a.jsonl'])
      const wrapper = factory()
      // Click the export menu trigger (3rd action button: select-all, export, delete).
      await wrapper.find('.export-menu-wrap .ct-btn').trigger('click')
      const items = wrapper.findAll('.export-menu-item')
      expect(items).toHaveLength(2)
      await items[1].trigger('click') // HTML
      expect(wrapper.emitted('batch-export')).toEqual([['html']])
    })
  })
})
