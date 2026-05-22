// Global test setup — runs once before each test file (Vitest `setupFiles`).
//
// jsdom ships neither `matchMedia` nor the Web Animations API, but
// settings.ts touches `matchMedia` at *import time* and flyToTrash.ts calls
// `Element.prototype.animate`. Polyfill both here so importing those modules
// doesn't throw.
import { afterEach, vi } from 'vitest'

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
