import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

// SettingsModal reads the app version and update info through the Tauri-backed
// api module — stub it so the component can mount outside a Tauri shell.
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
import { lang, setLang, setTheme, theme } from '../../src/settings'

beforeEach(() => {
  setLang('en')
  setTheme('system')
  appVersionMock.mockReset().mockResolvedValue('9.9.9')
  checkUpdateMock.mockReset()
})
afterEach(() => {
  setLang('en')
  setTheme('system')
})

type Props = InstanceType<typeof SettingsModal>['$props']
const factory = (props: Partial<Props> = {}) =>
  mount(SettingsModal, {
    props: { cacheBytes: 0, ...props } as Props,
    global: { directives: { tooltip: vTooltip } },
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

  it('renders the four languages and switches on click', async () => {
    const wrapper = factory()
    const langBtns = wrapper.findAll('.seg-wide button')
    expect(langBtns).toHaveLength(4)
    expect(langBtns[0].classes()).toContain('active') // English is current

    await langBtns[1].trigger('click') // 简体中文
    expect(lang.value).toBe('zh')
  })

  it('renders the theme dropdown and switches on change', async () => {
    const wrapper = factory()
    const select = wrapper.find('.theme-select')
    expect(select.exists()).toBe(true)
    expect(select.findAll('option')).toHaveLength(5)

    await select.setValue('dracula')
    expect(theme.value).toBe('dracula')
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

    await wrapper.findAll('.btn')[1].trigger('click')
    await flushPromises()

    expect(checkUpdateMock).toHaveBeenCalled()
    expect(wrapper.text()).toContain('2.0.0')
  })

  it('reports when the app is up to date', async () => {
    checkUpdateMock.mockResolvedValue({ hasUpdate: false, latest: '1.0.0', current: '1.0.0' })
    const wrapper = factory()
    await flushPromises()

    await wrapper.findAll('.btn')[1].trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('latest version')
  })

  it('surfaces a failed update check', async () => {
    checkUpdateMock.mockRejectedValue(new Error('offline'))
    const wrapper = factory()
    await flushPromises()

    await wrapper.findAll('.btn')[1].trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Update check failed')
  })
})
