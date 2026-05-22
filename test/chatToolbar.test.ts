import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  navigate,
  resetChatToolbar,
  search,
  searchCount,
  searchIndex,
  searchScope,
  setSearchNavigator,
  toolsCollapsed,
} from '../src/chatToolbar'

afterEach(() => {
  setSearchNavigator(null)
  resetChatToolbar()
})

describe('chatToolbar refs', () => {
  it('start at their documented defaults', () => {
    expect(toolsCollapsed.value).toBe(false)
    expect(search.value).toBe('')
    expect(searchScope.value).toBe('all')
    expect(searchCount.value).toBe(0)
    expect(searchIndex.value).toBe(0)
  })
})

describe('resetChatToolbar', () => {
  it('zeroes every piece of search/collapse state', () => {
    toolsCollapsed.value = true
    search.value = 'needle'
    searchScope.value = 'agent'
    searchCount.value = 7
    searchIndex.value = 3

    resetChatToolbar()

    expect(toolsCollapsed.value).toBe(false)
    expect(search.value).toBe('')
    expect(searchScope.value).toBe('all')
    expect(searchCount.value).toBe(0)
    expect(searchIndex.value).toBe(0)
  })
})

describe('search navigator', () => {
  it('does nothing when no navigator is registered', () => {
    expect(() => navigate(1)).not.toThrow()
  })

  it('forwards the direction to a registered navigator', () => {
    const fn = vi.fn()
    setSearchNavigator(fn)

    navigate(1)
    navigate(-1)

    expect(fn).toHaveBeenNthCalledWith(1, 1)
    expect(fn).toHaveBeenNthCalledWith(2, -1)
  })

  it('stops forwarding once the navigator is unregistered', () => {
    const fn = vi.fn()
    setSearchNavigator(fn)
    setSearchNavigator(null)

    navigate(1)

    expect(fn).not.toHaveBeenCalled()
  })
})
