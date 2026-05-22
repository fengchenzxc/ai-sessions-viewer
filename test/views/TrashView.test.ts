import { beforeEach, describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import TrashView from '../../src/views/TrashView.vue'
import { vTooltip } from '../../src/tooltip'
import { setLang } from '../../src/settings'
import {
  resetTrashToolbar,
  selectMode,
  selectedTrash,
  trashSearch,
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

const factory = (trash: TrashItem[], loading = false) =>
  mount(TrashView, {
    props: { trash, loading },
    global: { directives: { tooltip: vTooltip } },
  })

describe('TrashView', () => {
  it('renders one card per trash item', () => {
    const wrapper = factory([item({ trashFile: 'a' }), item({ trashFile: 'b' })])
    expect(wrapper.findAll('.session-card')).toHaveLength(2)
  })

  it('shows the empty state when the trash is empty', () => {
    expect(factory([]).text()).toContain('Trash is empty')
  })

  it('shows the no-match state when filters exclude every item', () => {
    trashSearch.value = 'definitely-not-present'
    const wrapper = factory([item({ trashFile: 'a' })])
    expect(wrapper.findAll('.session-card')).toHaveLength(0)
    expect(wrapper.text()).toContain('No sessions match')
  })

  it('emits restore / permanent-delete from the row actions', async () => {
    const wrapper = factory([item({ trashFile: 'a' })])
    const [restore, del] = wrapper.findAll('.session-actions .icon-btn')
    await restore.trigger('click')
    await del.trigger('click')
    expect(wrapper.emitted('restore')).toHaveLength(1)
    expect(wrapper.emitted('permanent-delete')).toHaveLength(1)
  })

  describe('select mode', () => {
    it('shows a checkbox on each card and hides the row actions', () => {
      selectMode.value = true
      const wrapper = factory([item({ trashFile: 'a' })])
      expect(wrapper.find('.trash-check').exists()).toBe(true)
      expect(wrapper.find('.session-actions').exists()).toBe(false)
    })

    it('toggles selection — and does not open — when a card is clicked', async () => {
      selectMode.value = true
      const wrapper = factory([item({ trashFile: 'a' })])

      await wrapper.find('.session-card').trigger('click')
      expect(selectedTrash.value.has('a')).toBe(true)

      await wrapper.find('.session-card').trigger('click')
      expect(selectedTrash.value.has('a')).toBe(false)

      expect(wrapper.emitted('open')).toBeUndefined()
    })
  })

  describe('open detail', () => {
    it('emits "open" with the item — and selects nothing — on a card click', async () => {
      const it0 = item({ trashFile: 'a' })
      const wrapper = factory([it0])
      await wrapper.find('.session-card').trigger('click')
      expect(selectedTrash.value.size).toBe(0)
      expect(wrapper.emitted('open')).toHaveLength(1)
      expect(wrapper.emitted('open')![0][0]).toEqual(it0)
    })

    it('does not open when a row action button is clicked', async () => {
      const wrapper = factory([item({ trashFile: 'a' })])
      const [restore] = wrapper.findAll('.session-actions .icon-btn')
      await restore.trigger('click')
      expect(wrapper.emitted('open')).toBeUndefined()
      expect(wrapper.emitted('restore')).toHaveLength(1)
    })
  })

  describe('keyword highlight', () => {
    it('highlights the matched keyword in the trash title', () => {
      trashSearch.value = 'parser'
      const wrapper = factory([
        item({ trashFile: 'a', title: 'Refactor parser', projectLabel: 'web' }),
      ])
      const hits = wrapper.findAll('.session-title .kw-hit')
      expect(hits).toHaveLength(1)
      expect(hits[0].text()).toBe('parser')
    })

    it('highlights a match in the project label', () => {
      trashSearch.value = 'viewer'
      const wrapper = factory([
        item({ trashFile: 'a', title: 'no match', projectLabel: '/Users/me/viewer' }),
      ])
      const hits = wrapper.findAll('.session-meta .kw-hit')
      expect(hits).toHaveLength(1)
      expect(hits[0].text()).toBe('viewer')
    })

    it('renders no highlight when there is no active search', () => {
      const wrapper = factory([item({ trashFile: 'a', title: 'Refactor parser' })])
      expect(wrapper.find('.kw-hit').exists()).toBe(false)
    })
  })
})
