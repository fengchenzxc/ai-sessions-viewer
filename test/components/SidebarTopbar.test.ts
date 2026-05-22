import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import SidebarTopbar from '../../src/components/SidebarTopbar.vue'
import { vTooltip } from '../../src/tooltip'

const factory = (props: Partial<InstanceType<typeof SidebarTopbar>['$props']> = {}) =>
  mount(SidebarTopbar, {
    props: { refreshing: false, showTrash: false, hasTrash: false, ...props },
    global: { directives: { tooltip: vTooltip } },
  })

describe('SidebarTopbar', () => {
  it('renders the toggle, refresh and trash buttons', () => {
    expect(factory().findAll('.top-btn')).toHaveLength(3)
  })

  it('emits toggle-sidebar / refresh / open-trash on the matching click', async () => {
    const wrapper = factory()
    const [toggle, refresh] = wrapper.findAll('.topbar-icons .top-btn')
    await toggle.trigger('click')
    await refresh.trigger('click')
    await wrapper.find('.topbar-trash-btn').trigger('click')

    expect(wrapper.emitted('toggle-sidebar')).toHaveLength(1)
    expect(wrapper.emitted('refresh')).toHaveLength(1)
    expect(wrapper.emitted('open-trash')).toHaveLength(1)
  })

  it('marks the refresh button spinning and disabled while refreshing', () => {
    const refresh = factory({ refreshing: true }).findAll('.topbar-icons .top-btn')[1]
    expect(refresh.classes()).toContain('spinning')
    expect(refresh.attributes('disabled')).toBeDefined()
  })

  it('highlights the trash button when the trash view is open', () => {
    expect(factory({ showTrash: true }).find('.topbar-trash-btn').classes()).toContain('active')
    expect(factory({ showTrash: false }).find('.topbar-trash-btn').classes()).not.toContain('active')
  })

  it('shows the trash dot only when there is trashed content', () => {
    expect(factory({ hasTrash: true }).find('.trash-dot').exists()).toBe(true)
    expect(factory({ hasTrash: false }).find('.trash-dot').exists()).toBe(false)
  })
})
