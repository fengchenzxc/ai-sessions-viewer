import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setLang } from '../../src/settings'
import { vTooltip } from '../../src/tooltip'

// Stub the Tauri invoke (searchSessions) — every spec drives this manually.
// cancelSearch is also called by the modal on every input event to interrupt
// in-flight searches; stub it to a no-op to keep tests from hitting Tauri.
const { searchMock, cancelMock } = vi.hoisted(() => ({
  searchMock: vi.fn(),
  cancelMock: vi.fn().mockResolvedValue(undefined),
}))
let _id = 0
vi.mock('../../src/api', () => ({
  searchSessions: searchMock,
  cancelSearch: cancelMock,
  nextSearchRequestId: () => ++_id,
}))

import GlobalSearchModal from '../../src/modals/GlobalSearchModal.vue'
import type { SearchHit } from '../../src/types'
import { clearRecents, recentSearches } from '../../src/globalSearch'

beforeEach(() => {
  setLang('en')
  searchMock.mockReset()
  cancelMock.mockClear()
  cancelMock.mockResolvedValue(undefined)
  _id = 0
  clearRecents()
  sessionStorage.clear()
})

function hit(over: Partial<SearchHit> = {}): SearchHit {
  return {
    projectKey: 'proj',
    projectDisplay: '/work/proj',
    matchedField: 'title',
    snippet: 'A session',
    session: {
      id: 'aaaa1111',
      fileName: 's.jsonl',
      path: '/work/proj/s.jsonl',
      title: 'A session',
      modified: 0,
      size: 1,
      messageCount: 1,
      codexAppListRank: null,
      codexAppListScanned: 0,
      codexAppFirstPageSize: 50,
      codexAppFirstPagePosition: 0,
      codexInternal: false,
      codexArchived: false,
    },
    ...over,
  }
}

const factory = (show = true) =>
  mount(GlobalSearchModal, {
    props: { show, agent: 'claude' },
    attachTo: document.body,
    global: { directives: { tooltip: vTooltip } },
  })

describe('GlobalSearchModal', () => {
  it('renders the placeholder and footer hints when open', () => {
    const wrapper = factory()
    expect(wrapper.find('.gs-input').exists()).toBe(true)
    expect(wrapper.find('.gs-foot').text()).toContain('to select')
    expect(wrapper.find('.gs-foot').text()).toContain('to navigate')
    expect(wrapper.find('.gs-foot').text()).toContain('to close')
    wrapper.unmount()
  })

  it('shows the empty state with no input', () => {
    const wrapper = factory()
    expect(wrapper.text()).toContain('No recent searches')
    wrapper.unmount()
  })

  it('debounces the search and renders grouped results', async () => {
    searchMock.mockResolvedValue([
      hit({ matchedField: 'text', snippet: 'hello there' }),
      hit({
        projectKey: 'b',
        projectDisplay: '/work/b',
        session: { ...hit().session, path: '/work/b/x.jsonl', title: 'Other' },
      }),
    ])
    const wrapper = factory()
    await wrapper.find('.gs-input').setValue('hello')
    // 450ms debounce —— 等到 500ms 再 flush 异步队列。
    await new Promise((r) => setTimeout(r, 500))
    await flushPromises()
    // 第 3 个参数是单调 request id，每次调用递增；这里只比对前两个。
    expect(searchMock).toHaveBeenCalledWith('claude', 'hello', expect.any(Number))
    expect(wrapper.findAll('.gs-group')).toHaveLength(2)
    expect(wrapper.findAll('.gs-row')).toHaveLength(2)
    wrapper.unmount()
  })

  it('skips searches shorter than the min query length', async () => {
    const wrapper = factory()
    await wrapper.find('.gs-input').setValue('a')
    await new Promise((r) => setTimeout(r, 500))
    await flushPromises()
    expect(searchMock).not.toHaveBeenCalled()
    wrapper.unmount()
  })

  it('opens the highlighted hit and records the query on Enter', async () => {
    searchMock.mockResolvedValue([hit()])
    const wrapper = factory()
    await wrapper.find('.gs-input').setValue('hello')
    await new Promise((r) => setTimeout(r, 500))
    await flushPromises()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }))
    await nextTick()
    expect(wrapper.emitted('open')).toHaveLength(1)
    expect(wrapper.emitted('update:show')?.[0]).toEqual([false])
    expect(recentSearches.value).toEqual(['hello'])
    wrapper.unmount()
  })

  it('Esc closes the modal', async () => {
    const wrapper = factory()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await nextTick()
    expect(wrapper.emitted('update:show')?.[0]).toEqual([false])
    wrapper.unmount()
  })

  it('cancels the in-flight search when the user keeps typing', async () => {
    // 装一个永远 pending 的 searchSessions —— 模拟「搜索在跑」的状态。
    let resolveFirst: (v: SearchHit[]) => void = () => {}
    searchMock.mockImplementationOnce(
      () => new Promise<SearchHit[]>((r) => { resolveFirst = r }),
    )
    searchMock.mockResolvedValueOnce([hit()])
    const wrapper = factory()
    const input = wrapper.find('.gs-input')

    await input.setValue('ab')
    await new Promise((r) => setTimeout(r, 500))
    // 第一次搜索已经发出去（promise 挂着），开始下一次输入应该立即调 cancelSearch
    expect(searchMock).toHaveBeenCalledTimes(1)
    expect(cancelMock).not.toHaveBeenCalled()

    await input.setValue('abc')
    // input 事件刚发出来就应该 cancel —— 不用等防抖。
    expect(cancelMock).toHaveBeenCalledTimes(1)

    // 释放卡住的第一次（这条结果会被前端的 reqSeq 守卫丢弃）
    resolveFirst([])
    await new Promise((r) => setTimeout(r, 500))
    expect(searchMock).toHaveBeenCalledTimes(2)
    wrapper.unmount()
  })

  it('ArrowDown moves the highlight through the result list', async () => {
    searchMock.mockResolvedValue([
      hit({ session: { ...hit().session, path: 'a' } }),
      hit({ session: { ...hit().session, path: 'b', title: 'Second' } }),
    ])
    const wrapper = factory()
    // 至少 2 字符才会触发搜索
    await wrapper.find('.gs-input').setValue('se')
    await new Promise((r) => setTimeout(r, 500))
    await flushPromises()
    let rows = wrapper.findAll('.gs-row')
    expect(rows[0].classes()).toContain('active')
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }))
    await nextTick()
    rows = wrapper.findAll('.gs-row')
    expect(rows[1].classes()).toContain('active')
    wrapper.unmount()
  })
})
