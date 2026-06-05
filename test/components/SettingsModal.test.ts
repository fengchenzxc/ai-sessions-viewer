import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const { appVersionMock, checkUpdateMock } = vi.hoisted(() => ({
  appVersionMock: vi.fn(),
  checkUpdateMock: vi.fn(),
}))
vi.mock('../../src/api', () => ({
  appVersion: appVersionMock,
  checkUpdate: checkUpdateMock,
}))

import SettingsModal from '../../src/components/SettingsModal.vue'
import { vTooltip } from '../../src/tooltip'
import { lang, setLang, setTerminalApp, setTheme, terminalApp, theme } from '../../src/settings'

beforeEach(() => {
  setLang('en')
  setTheme('system')
  setTerminalApp('terminal')
  appVersionMock.mockReset().mockResolvedValue('9.9.9')
  checkUpdateMock.mockReset()
})
afterEach(() => {
  setLang('en')
  setTheme('system')
  setTerminalApp('terminal')
})

type Props = InstanceType<typeof SettingsModal>['$props']
const factory = (props: Partial<Props> = {}) =>
  mount(SettingsModal, {
    props: { cacheBytes: 0, ...props } as Props,
    global: { directives: { tooltip: vTooltip } },
    attachTo: document.body,
  })

describe('SettingsModal', () => {
  it('shows a human-readable cache size', () => {
    expect(factory({ cacheBytes: 2048 }).find('.set-section-tail').text()).toBe('2.0 KB')
  })

  it('shows "0 B" and disables the clear button when the cache is empty', () => {
    const wrapper = factory({ cacheBytes: 0 })
    expect(wrapper.find('.set-section-tail').text()).toBe('0 B')
    expect(wrapper.find('.btn.danger').attributes('disabled')).toBeDefined()
  })

  it('enables the clear button and emits clearCache when there is cached data', async () => {
    const wrapper = factory({ cacheBytes: 4096 })
    const clearBtn = wrapper.find('.btn.danger')
    expect(clearBtn.attributes('disabled')).toBeUndefined()
    await clearBtn.trigger('click')
    expect(wrapper.emitted('clearCache')).toHaveLength(1)
  })

  it('emits close from the X button and the overlay backdrop', async () => {
    const wrapper = factory()
    await wrapper.find('.modal-close').trigger('click')
    await wrapper.find('.overlay').trigger('click')
    expect(wrapper.emitted('close')).toHaveLength(2)
  })

  it('switches language via the custom dropdown', async () => {
    const wrapper = factory()
    const dropdowns = wrapper.findAll('.set-dropdown-btn')
    await dropdowns[0].trigger('click')
    const items = wrapper.findAll('.set-dropdown-item')
    expect(items.length).toBeGreaterThanOrEqual(4)
    await items[1].trigger('click') // 简体中文
    expect(lang.value).toBe('zh')
  })

  it('switches theme via the custom dropdown', async () => {
    const wrapper = factory()
    const dropdowns = wrapper.findAll('.set-dropdown-btn')
    await dropdowns[1].trigger('click')
    const items = wrapper.findAll('.set-dropdown-item')
    // find the Dracula option (last one)
    await items[items.length - 1].trigger('click')
    expect(theme.value).toBe('dracula')
  })

  it('switches terminal app from the advanced tab dropdown', async () => {
    const wrapper = factory()
    await wrapper.findAll('.set-tabs button')[1].trigger('click')
    expect(wrapper.find('.terminal-choice-group').exists()).toBe(false)

    const terminalDropdown = wrapper.find('.terminal-dropdown .set-dropdown-btn')
    expect(terminalDropdown.text()).toContain('Terminal.app')

    await terminalDropdown.trigger('click')
    const terminalItems = wrapper.findAll('.terminal-dropdown .set-dropdown-item')
    expect(terminalItems).toHaveLength(3)

    await terminalItems[0].trigger('click')
    expect(terminalApp.value).toBe('warp')

    await terminalDropdown.trigger('click')
    await wrapper.findAll('.terminal-dropdown .set-dropdown-item')[2].trigger('click')
    expect(terminalApp.value).toBe('iterm2')
  })

  it('localizes settings tabs and terminal description', async () => {
    setLang('zh')
    const wrapper = factory()
    const tabs = wrapper.findAll('.set-tabs button')
    expect(tabs[0].text()).toBe('通用')
    expect(tabs[1].text()).toBe('高级')

    await tabs[1].trigger('click')
    expect(wrapper.text()).not.toContain('settings.tab.')
    expect(wrapper.text()).not.toContain('settings.terminalDesc')
    expect(wrapper.find('.terminal-dropdown').exists()).toBe(true)
    expect(wrapper.text()).toContain('选择恢复或新建会话时打开的外部终端。')
  })

  it('loads the app version on mount', async () => {
    const wrapper = factory()
    await flushPromises()
    expect(appVersionMock).toHaveBeenCalled()
    expect(wrapper.text()).toContain('v9.9.9')
  })

  it('reports when an update is available', async () => {
    checkUpdateMock.mockResolvedValue({ hasUpdate: true, latest: '2.0.0', current: '1.0.0' })
    const wrapper = factory()
    await flushPromises()

    const checkBtn = wrapper.find('.set-update-actions .btn')
    await checkBtn.trigger('click')
    await flushPromises()

    expect(checkUpdateMock).toHaveBeenCalled()
    expect(wrapper.text()).toContain('2.0.0')
  })

  it('reports when the app is up to date', async () => {
    checkUpdateMock.mockResolvedValue({ hasUpdate: false, latest: '1.0.0', current: '1.0.0' })
    const wrapper = factory()
    await flushPromises()

    const checkBtn = wrapper.find('.set-update-actions .btn')
    await checkBtn.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('latest version')
  })

  it('surfaces a failed update check', async () => {
    checkUpdateMock.mockRejectedValue(new Error('offline'))
    const wrapper = factory()
    await flushPromises()

    const checkBtn = wrapper.find('.set-update-actions .btn')
    await checkBtn.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Update check failed')
  })
})
