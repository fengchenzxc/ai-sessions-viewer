import { beforeEach, describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import TrashTopbar from '../../src/components/topbar/TrashTopbar.vue'
import { vTooltip } from '../../src/tooltip'
import { setLang } from '../../src/settings'
import {
  resetTrashToolbar,
  selectMode,
  selectedTrash,
  trashProject,
  trashSearch,
  trashSort,
} from '../../src/trashToolbar'
import type { TrashItem } from '../../src/types'

beforeEach(() => {
  setLang('en')
  resetTrashToolbar()
})

const item = (over: Partial<TrashItem> & { trashFile: string }): TrashItem => ({
  agent: 'claude',
  projectLabel: 'proj',
  originalPath: '/orig',
  trashPath: `/trash/${over.trashFile}`,
  deletedAt: 0,
  title: 'A session',
  size: 100,
  ...over,
})

// 默认两条 —— 排序 / 批量选择按钮只在 ≥2 条时才渲染。
const factory = (
  items: TrashItem[] = [item({ trashFile: 'a' }), item({ trashFile: 'b' })],
) =>
  mount(TrashTopbar, {
    props: { items },
    global: { directives: { tooltip: vTooltip } },
  })

describe('TrashTopbar', () => {
  it('binds the search box to the shared search ref', async () => {
    const wrapper = factory()
    await wrapper.find('.ct-search-input').setValue('hello')
    expect(trashSearch.value).toBe('hello')
  })

  it('toggles the time sort', async () => {
    const wrapper = factory()
    expect(trashSort.value).toBe('recent')
    await wrapper.findAll('.ct-actions .ct-btn')[0].trigger('click')
    expect(trashSort.value).toBe('oldest')
  })

  it('enters select mode from the select button', async () => {
    const wrapper = factory()
    await wrapper.findAll('.ct-actions .ct-btn')[1].trigger('click')
    expect(selectMode.value).toBe(true)
  })

  it('hides the sort and select buttons unless there are at least two items', () => {
    // 排序 / 批量选择在 0 或 1 条时没有意义，不渲染。
    expect(factory([]).findAll('.ct-actions .ct-btn')).toHaveLength(0)
    expect(
      factory([item({ trashFile: 'a' })]).findAll('.ct-actions .ct-btn'),
    ).toHaveLength(0)
    // 两条时显示 [排序, 批量选择]。
    expect(factory().findAll('.ct-actions .ct-btn')).toHaveLength(2)
  })

  it('lists distinct projects in the filter dropdown and applies a pick', async () => {
    const wrapper = factory([
      item({ trashFile: 'a', projectLabel: 'web' }),
      item({ trashFile: 'b', projectLabel: 'api' }),
    ])
    await wrapper.find('.ct-scope-btn').trigger('click')
    const items = wrapper.findAll('.ct-scope-item')
    // 'All projects' + 2 distinct labels
    expect(items).toHaveLength(3)

    await items[1].trigger('click') // 'api' (sorted first)
    expect(trashProject.value).toBe('api')
  })

  it('shows only the project basename in the dropdown, not the full path', async () => {
    const wrapper = factory([
      item({ trashFile: 'a', projectLabel: '/Users/me/apps/my-project' }),
    ])
    await wrapper.find('.ct-scope-btn').trigger('click')
    const labels = wrapper.findAll('.ct-scope-item').map((b) => b.text())
    expect(labels).toContain('my-project')
    expect(labels.some((l) => l.includes('/Users'))).toBe(false)
  })

  it('focuses the search box on the ⌘F / Ctrl+F shortcut', () => {
    const wrapper = mount(TrashTopbar, {
      props: { items: [item({ trashFile: 'a' })] },
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
    it('select-all toggles the whole visible set', async () => {
      const wrapper = factory([item({ trashFile: 'a' }), item({ trashFile: 'b' })])
      selectMode.value = true
      await wrapper.vm.$nextTick()

      // [select-all, restore, cancel]
      await wrapper.findAll('.ct-actions .ct-btn')[0].trigger('click')
      expect(selectedTrash.value.size).toBe(2)

      await wrapper.findAll('.ct-actions .ct-btn')[0].trigger('click')
      expect(selectedTrash.value.size).toBe(0)
    })

    it('keeps the restore button disabled until something is selected', async () => {
      const wrapper = factory([item({ trashFile: 'a' })])
      selectMode.value = true
      await wrapper.vm.$nextTick()

      const restoreBtn = () => wrapper.findAll('.ct-actions .ct-btn')[1]
      expect(restoreBtn().attributes('disabled')).toBeDefined()

      selectedTrash.value = new Set(['a'])
      await wrapper.vm.$nextTick()
      expect(restoreBtn().attributes('disabled')).toBeUndefined()
    })

    it('emits batch-restore when restore is clicked with a selection', async () => {
      const wrapper = factory([item({ trashFile: 'a' })])
      selectMode.value = true
      selectedTrash.value = new Set(['a'])
      await wrapper.vm.$nextTick()

      await wrapper.findAll('.ct-actions .ct-btn')[1].trigger('click')
      expect(wrapper.emitted('batch-restore')).toHaveLength(1)
    })

    it('exits select mode from the cancel button', async () => {
      const wrapper = factory()
      selectMode.value = true
      await wrapper.vm.$nextTick()

      await wrapper.findAll('.ct-actions .ct-btn')[2].trigger('click')
      expect(selectMode.value).toBe(false)
    })
  })
})
