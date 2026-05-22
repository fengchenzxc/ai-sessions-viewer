import { beforeEach, describe, expect, it } from 'vitest'
import { getRecents, recordRecent, recents } from '../src/recents'

beforeEach(() => {
  localStorage.clear()
  recents.value = {}
})

describe('recents', () => {
  it('records a project and reads it back', () => {
    recordRecent('claude', 'proj-a')
    expect(getRecents('claude')).toEqual(['proj-a'])
  })

  it('returns an empty list for an agent with no history', () => {
    expect(getRecents('codex')).toEqual([])
  })

  it('puts the most recently opened project first', () => {
    recordRecent('claude', 'a')
    recordRecent('claude', 'b')
    recordRecent('claude', 'c')
    expect(getRecents('claude')).toEqual(['c', 'b', 'a'])
  })

  it('deduplicates — reopening a project moves it to the front', () => {
    recordRecent('claude', 'a')
    recordRecent('claude', 'b')
    recordRecent('claude', 'a')
    expect(getRecents('claude')).toEqual(['a', 'b'])
  })

  it('caps the list at 6 entries', () => {
    for (const d of ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h']) {
      recordRecent('claude', d)
    }
    expect(getRecents('claude')).toEqual(['h', 'g', 'f', 'e', 'd', 'c'])
  })

  it('keeps each agent in its own bucket', () => {
    recordRecent('claude', 'c-proj')
    recordRecent('codex', 'x-proj')
    expect(getRecents('claude')).toEqual(['c-proj'])
    expect(getRecents('codex')).toEqual(['x-proj'])
  })

  it('persists to localStorage', () => {
    recordRecent('claude', 'persisted')
    const raw = localStorage.getItem('recents:v1')
    expect(raw).toBeTruthy()
    expect(JSON.parse(raw!)).toEqual({ claude: ['persisted'] })
  })
})
