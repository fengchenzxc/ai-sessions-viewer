import { afterEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { applyTheme, clearAppCache, lang, setLang, setTheme, theme } from '../src/settings'

const DARK = 'theme-dark'

// Replace window.matchMedia so `theme: 'system'` resolves deterministically.
function stubMatchMedia(matches: boolean) {
  vi.stubGlobal(
    'matchMedia',
    vi.fn().mockImplementation((query: string) => ({
      matches,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
  document.documentElement.classList.remove(DARK)
  setLang('en')
  setTheme('system')
})

describe('setLang', () => {
  it('updates the ref and persists to localStorage', () => {
    setLang('ja')
    expect(lang.value).toBe('ja')
    expect(localStorage.getItem('lang')).toBe('ja')
  })
})

describe('setTheme', () => {
  it('updates the ref and persists to localStorage', () => {
    setTheme('dark')
    expect(theme.value).toBe('dark')
    expect(localStorage.getItem('theme')).toBe('dark')
  })
})

describe('applyTheme', () => {
  it('adds the dark class when the theme is dark', () => {
    setTheme('dark')
    applyTheme()
    expect(document.documentElement.classList.contains(DARK)).toBe(true)
  })

  it('removes the dark class when the theme is light', () => {
    document.documentElement.classList.add(DARK)
    setTheme('light')
    applyTheme()
    expect(document.documentElement.classList.contains(DARK)).toBe(false)
  })

  it('follows the system preference when the theme is system', () => {
    stubMatchMedia(true)
    setTheme('system')
    applyTheme()
    expect(document.documentElement.classList.contains(DARK)).toBe(true)

    stubMatchMedia(false)
    applyTheme()
    expect(document.documentElement.classList.contains(DARK)).toBe(false)
  })

  it('re-applies automatically (via watchEffect) when the theme ref changes', async () => {
    setTheme('dark')
    await nextTick()
    expect(document.documentElement.classList.contains(DARK)).toBe(true)

    setTheme('light')
    await nextTick()
    expect(document.documentElement.classList.contains(DARK)).toBe(false)
  })
})

describe('clearAppCache', () => {
  it('removes the project-prefs cache key', () => {
    localStorage.setItem('projPrefs:v1', '{"pinned":[]}')
    clearAppCache()
    expect(localStorage.getItem('projPrefs:v1')).toBeNull()
  })
})

// detectSystemLang is module-private and only runs at import time, so we
// re-import a fresh copy of settings.ts under controlled navigator state.
describe('language detection on first load', () => {
  async function freshLoad(opts: {
    languages?: unknown
    storedLang?: string
    storedTheme?: string
  }) {
    localStorage.clear()
    if (opts.storedLang) localStorage.setItem('lang', opts.storedLang)
    if (opts.storedTheme) localStorage.setItem('theme', opts.storedTheme)
    Object.defineProperty(window.navigator, 'languages', {
      value: opts.languages,
      configurable: true,
    })
    vi.resetModules()
    return import('../src/settings')
  }

  it.each([
    ['zh-Hant-TW', 'zh-TW'],
    ['zh-TW', 'zh-TW'],
    ['zh-HK', 'zh-TW'],
    ['zh-MO', 'zh-TW'],
    ['zh-CN', 'zh'],
    ['zh', 'zh'],
    ['ja-JP', 'ja'],
    ['ja', 'ja'],
    ['en-GB', 'en'],
  ])('maps %s to %s', async (tag, expected) => {
    const mod = await freshLoad({ languages: [tag] })
    expect(mod.lang.value).toBe(expected)
  })

  it('falls back to English for an unsupported language', async () => {
    const mod = await freshLoad({ languages: ['fr-FR'] })
    expect(mod.lang.value).toBe('en')
  })

  it('skips empty entries and uses the first usable tag', async () => {
    const mod = await freshLoad({ languages: ['', 'ja-JP'] })
    expect(mod.lang.value).toBe('ja')
  })

  it('falls back to navigator.language when languages is unavailable', async () => {
    const mod = await freshLoad({ languages: undefined })
    expect(mod.lang.value).toBe('en')
  })

  it('prefers an explicit localStorage language over detection', async () => {
    const mod = await freshLoad({ languages: ['ja-JP'], storedLang: 'zh' })
    expect(mod.lang.value).toBe('zh')
  })

  it('restores a persisted theme, defaulting to system', async () => {
    const stored = await freshLoad({ languages: ['en-US'], storedTheme: 'dark' })
    expect(stored.theme.value).toBe('dark')
    const fallback = await freshLoad({ languages: ['en-US'] })
    expect(fallback.theme.value).toBe('system')
  })
})
