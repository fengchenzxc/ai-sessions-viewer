import { beforeEach, describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import SessionsTopbar from '../../src/components/topbar/SessionsTopbar.vue'
import { vTooltip } from '../../src/tooltip'
import { setLang } from '../../src/settings'
import {
  resetSessionsToolbar,
  sessionSearch,
  sessionSort,
  sessionWithIdOnly,
} from '../../src/sessionsToolbar'

beforeEach(() => {
  setLang('en')
  resetSessionsToolbar()
})

const factory = () =>
  mount(SessionsTopbar, {
    global: { directives: { tooltip: vTooltip } },
  })

describe('SessionsTopbar', () => {
  it('binds the search box to the shared search ref', async () => {
    const wrapper = factory()
    await wrapper.find('.ct-search-input').setValue('parser')
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
})
