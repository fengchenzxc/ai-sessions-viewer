import { ref, watch, watchEffect } from 'vue'
import type { StatsRange, StatsScope, TerminalApp } from './types'

export type Lang = 'en' | 'zh' | 'zh-TW' | 'ja'
export type Theme = 'light' | 'dark' | 'system' | 'codex' | 'dracula'
export type { TerminalApp }

const LANG_KEY = 'lang'
const THEME_KEY = 'theme'
const PREFS_KEY = 'projPrefs:v1'
const STATS_SCOPE_KEY = 'statsScope:v1'
const STATS_RANGE_KEY = 'statsRange:v1'
const CODEX_SHOW_INTERNAL_KEY = 'codexShowInternalSessions:v1'
const CODEX_SHOW_ARCHIVED_KEY = 'codexShowArchivedSessions:v1'
const TERMINAL_APP_KEY = 'terminalApp:v1'

/**
 * 根据浏览器/系统语言探测默认语言。
 * 匹配优先级：zh-Hant / zh-TW / zh-HK → zh-TW；其他 zh-* → zh；ja* → ja；其余 → en。
 * 仅在用户未显式设置（localStorage 无值）时生效。
 */
function detectSystemLang(): Lang {
  const candidates = (navigator.languages && navigator.languages.length
    ? navigator.languages
    : [navigator.language]) as string[]
  for (const raw of candidates) {
    if (!raw) continue
    const tag = raw.toLowerCase()
    if (tag.startsWith('zh')) {
      if (tag.includes('hant') || tag.includes('-tw') || tag.includes('-hk') || tag.includes('-mo')) {
        return 'zh-TW'
      }
      return 'zh'
    }
    if (tag.startsWith('ja')) return 'ja'
    if (tag.startsWith('en')) return 'en'
  }
  return 'en'
}

export const lang = ref<Lang>(
  (localStorage.getItem(LANG_KEY) as Lang | null) ?? detectSystemLang(),
)
function readTheme(): Theme {
  const v = localStorage.getItem(THEME_KEY)
  return v === 'light' || v === 'dark' || v === 'system' || v === 'codex' || v === 'dracula'
    ? v
    : 'system'
}
export const theme = ref<Theme>(readTheme())
export const codexShowInternalSessions = ref(localStorage.getItem(CODEX_SHOW_INTERNAL_KEY) === '1')
export const codexShowArchivedSessions = ref(localStorage.getItem(CODEX_SHOW_ARCHIVED_KEY) === '1')
function readTerminalApp(): TerminalApp {
  const v = localStorage.getItem(TERMINAL_APP_KEY)
  return v === 'warp' || v === 'terminal' || v === 'iterm2' ? v : 'terminal'
}
export const terminalApp = ref<TerminalApp>(readTerminalApp())

export function setLang(l: Lang) {
  lang.value = l
  localStorage.setItem(LANG_KEY, l)
}

export function setTheme(t: Theme) {
  theme.value = t
  localStorage.setItem(THEME_KEY, t)
}

export function setCodexShowInternalSessions(v: boolean) {
  codexShowInternalSessions.value = v
  localStorage.setItem(CODEX_SHOW_INTERNAL_KEY, v ? '1' : '0')
}

export function setCodexShowArchivedSessions(v: boolean) {
  codexShowArchivedSessions.value = v
  localStorage.setItem(CODEX_SHOW_ARCHIVED_KEY, v ? '1' : '0')
}

export function setTerminalApp(v: TerminalApp) {
  terminalApp.value = v
  localStorage.setItem(TERMINAL_APP_KEY, v)
}

function systemDark(): boolean {
  return window.matchMedia('(prefers-color-scheme: dark)').matches
}

export function applyTheme() {
  const dark = theme.value === 'dark' || theme.value === 'dracula' || (theme.value === 'system' && systemDark())
  document.documentElement.classList.toggle('theme-dark', dark)
  document.documentElement.classList.toggle('theme-codex', theme.value === 'codex')
  document.documentElement.classList.toggle('theme-dracula', theme.value === 'dracula')
}

// 主题变化或系统外观变化时自动应用
watchEffect(applyTheme)
window
  .matchMedia('(prefers-color-scheme: dark)')
  .addEventListener('change', () => {
    if (theme.value === 'system') applyTheme()
  })

/** 清除应用级缓存（目前只有项目置顶/沉底偏好；会话 rename 直接写 JSONL，不走 cache） */
export function clearAppCache() {
  localStorage.removeItem(PREFS_KEY)
}

// ---------- Statistics 页的 scope / range 持久化 ----------
// 默认 all agents + all time；用户改完写回 localStorage，下次进入沿用上次选择。

function readStatsScope(): StatsScope {
  const v = localStorage.getItem(STATS_SCOPE_KEY)
  return v === 'claude' || v === 'codex' || v === 'gemini' || v === 'all' ? v : 'all'
}
function readStatsRange(): StatsRange {
  const v = localStorage.getItem(STATS_RANGE_KEY)
  return v === 'today' || v === 'days7' || v === 'days30' || v === 'all' ? v : 'all'
}

export const statsScope = ref<StatsScope>(readStatsScope())
export const statsRange = ref<StatsRange>(readStatsRange())

watch(statsScope, (v) => localStorage.setItem(STATS_SCOPE_KEY, v))
watch(statsRange, (v) => localStorage.setItem(STATS_RANGE_KEY, v))
