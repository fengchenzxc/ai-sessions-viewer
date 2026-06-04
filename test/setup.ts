// Global test setup — runs once before each test file (Vitest `setupFiles`).
//
// jsdom ships neither `matchMedia` nor the Web Animations API, but
// settings.ts touches `matchMedia` at *import time* and flyToTrash.ts calls
// `Element.prototype.animate`. Polyfill both here so importing those modules
// doesn't throw.
import { afterEach, vi } from 'vitest'

// --- localStorage ---------------------------------------------------------
// Vitest 4 on newer Node can leave unqualified `localStorage` undefined at
// module import time unless Node is launched with --localstorage-file. The app
// reads it while importing settings.ts, so provide a small Storage-compatible
// shim for tests.
if (!globalThis.localStorage) {
  const store = new Map<string, string>()
  globalThis.localStorage = {
    get length() {
      return store.size
    },
    clear() {
      store.clear()
    },
    getItem(key: string) {
      return store.has(key) ? store.get(key)! : null
    },
    key(index: number) {
      return Array.from(store.keys())[index] ?? null
    },
    removeItem(key: string) {
      store.delete(key)
    },
    setItem(key: string, value: string) {
      store.set(key, String(value))
    },
  } as Storage
}

// --- window.matchMedia ----------------------------------------------------
// Default to light mode (matches: false). Individual tests override
// `window.matchMedia` with vi.stubGlobal when they need dark mode.
if (!window.matchMedia) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(), // deprecated, kept for completeness
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }))
}

// --- ResizeObserver -------------------------------------------------------
// jsdom omits ResizeObserver; CollapsibleBox feature-detects it, so provide a
// no-op class to exercise that branch.
if (!globalThis.ResizeObserver) {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
}

// --- IntersectionObserver -------------------------------------------------
// jsdom omits this too; SessionsView uses it to lazy-load per-card token
// usage. Tests don't exercise visibility, so a no-op is enough.
if (!globalThis.IntersectionObserver) {
  globalThis.IntersectionObserver = class {
    constructor() {}
    observe() {}
    unobserve() {}
    disconnect() {}
    takeRecords() {
      return []
    }
    root = null
    rootMargin = ''
    thresholds: number[] = []
  } as unknown as typeof IntersectionObserver
}

// --- Element.prototype.animate -------------------------------------------
// Minimal Web Animations API stub: every test that exercises animation only
// needs `.finished` (a resolved promise) and `.cancel()`.
if (!Element.prototype.animate) {
  Element.prototype.animate = vi.fn().mockImplementation(() => ({
    finished: Promise.resolve(),
    cancel: vi.fn(),
    play: vi.fn(),
    pause: vi.fn(),
    onfinish: null,
  })) as unknown as typeof Element.prototype.animate
}

// Keep localStorage clean between tests so persisted lang/theme/prefs from
// one test never leak into the next.
afterEach(() => {
  localStorage.clear()
})
