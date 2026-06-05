<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { t } from '../i18n'
import {
  codexShowArchivedSessions,
  codexShowInternalSessions,
  lang,
  setCodexShowArchivedSessions,
  setCodexShowInternalSessions,
  setLang,
  setTerminalApp,
  setTheme,
  terminalApp,
  theme,
  type Lang,
  type TerminalApp,
  type Theme,
} from '../settings'
import { formatSize } from '../format'
import {
  IconClose,
  IconLanguages,
  IconPalette,
  IconTerminal,
  IconDatabase,
  IconInfo,
  IconRefresh,
  IconExternalLink,
  IconCheck,
  IconChevronDown,
} from './icons'
import * as api from '../api'
import {
  latestVersion,
  openReleasePage,
  syncFromManualCheck,
  updateAvailable,
} from '../updateCheck'

type SettingsTab = 'general' | 'advanced'

const props = defineProps<{ cacheBytes: number }>()
const emit = defineEmits<{ close: []; clearCache: [] }>()

const activeTab = ref<SettingsTab>('general')

const cacheLabel = computed(() =>
  props.cacheBytes > 0 ? formatSize(props.cacheBytes) : '0 B',
)

const version = ref('—')
const updateMsg = ref('')
const checking = ref(false)

// custom dropdown state
const langMenuOpen = ref(false)
const themeMenuOpen = ref(false)
const terminalMenuOpen = ref(false)
const langWrapEl = ref<HTMLElement>()
const themeWrapEl = ref<HTMLElement>()
const terminalWrapEl = ref<HTMLElement>()

function pickLang(v: Lang) {
  setLang(v)
  langMenuOpen.value = false
}
function pickTheme(v: Theme) {
  setTheme(v)
  themeMenuOpen.value = false
}
function pickTerminal(v: TerminalApp) {
  setTerminalApp(v)
  terminalMenuOpen.value = false
}
function onDocClick(e: MouseEvent) {
  if (langMenuOpen.value && langWrapEl.value && !langWrapEl.value.contains(e.target as Node))
    langMenuOpen.value = false
  if (themeMenuOpen.value && themeWrapEl.value && !themeWrapEl.value.contains(e.target as Node))
    themeMenuOpen.value = false
  if (
    terminalMenuOpen.value &&
    terminalWrapEl.value &&
    !terminalWrapEl.value.contains(e.target as Node)
  )
    terminalMenuOpen.value = false
}
onMounted(() => document.addEventListener('click', onDocClick, true))
onUnmounted(() => document.removeEventListener('click', onDocClick, true))

onMounted(async () => {
  try {
    version.value = await api.appVersion()
  } catch {
    /* ignore */
  }
  if (updateAvailable.value && latestVersion.value) {
    updateMsg.value = t('settings.updateAvailable', {
      v: latestVersion.value,
      cur: version.value,
    })
  }
})

const langOptions: { v: Lang; key: string }[] = [
  { v: 'en', key: 'settings.lang.en' },
  { v: 'zh', key: 'settings.lang.zh' },
  { v: 'zh-TW', key: 'settings.lang.zhTw' },
  { v: 'ja', key: 'settings.lang.ja' },
]
type ThemeOpt = { v: Theme; key: string }
const themeOptions: ThemeOpt[] = [
  { v: 'light', key: 'settings.theme.light' },
  { v: 'dark', key: 'settings.theme.dark' },
  { v: 'system', key: 'settings.theme.system' },
  { v: 'codex', key: 'settings.theme.codex' },
  { v: 'dracula', key: 'settings.theme.dracula' },
]
type TerminalOpt = { v: TerminalApp; key: string }
const terminalOptions: TerminalOpt[] = [
  { v: 'warp', key: 'settings.terminal.warp' },
  { v: 'terminal', key: 'settings.terminal.terminal' },
  { v: 'iterm2', key: 'settings.terminal.iterm2' },
]

const currentLangLabel = computed(() => {
  const o = langOptions.find(o => o.v === lang.value)
  return o ? t(o.key) : lang.value
})
const currentThemeLabel = computed(() => {
  const o = themeOptions.find(o => o.v === theme.value)
  return o ? t(o.key) : theme.value
})
const currentTerminalLabel = computed(() => {
  const o = terminalOptions.find(o => o.v === terminalApp.value)
  return o ? t(o.key) : terminalApp.value
})

async function doCheck() {
  if (checking.value) return
  checking.value = true
  updateMsg.value = t('settings.checking')
  try {
    const r = await api.checkUpdate()
    updateMsg.value = r.hasUpdate
      ? t('settings.updateAvailable', { v: r.latest, cur: r.current })
      : t('settings.upToDate', { v: r.current })
    syncFromManualCheck(r)
  } catch (e) {
    updateMsg.value = t('settings.updateFail', { e: String(e) })
  } finally {
    checking.value = false
  }
}
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div class="modal settings-modal">
      <div class="modal-head">
        <h3>{{ t('settings.title') }}</h3>
        <button
          class="modal-close"
          v-tooltip="t('common.close')"
          @click="emit('close')"
        >
          <IconClose />
        </button>
      </div>

      <div class="set-tabs segmented">
        <button
          :class="{ active: activeTab === 'general' }"
          @click="activeTab = 'general'"
        >
          {{ t('settings.tab.general') }}
        </button>
        <button
          :class="{ active: activeTab === 'advanced' }"
          @click="activeTab = 'advanced'"
        >
          {{ t('settings.tab.advanced') }}
        </button>
      </div>

      <div class="set-body">
        <template v-if="activeTab === 'general'">
          <!-- 语言 -->
          <section class="set-section">
            <header class="set-section-head">
              <IconLanguages />
              <span class="set-section-title">{{ t('settings.section.lang') }}</span>
              <div ref="langWrapEl" class="set-dropdown-wrap">
                <button
                  class="set-dropdown-btn"
                  :class="{ active: langMenuOpen }"
                  @click.stop="langMenuOpen = !langMenuOpen; themeMenuOpen = false; terminalMenuOpen = false"
                >
                  <span>{{ currentLangLabel }}</span>
                  <IconChevronDown class="set-dropdown-chev" />
                </button>
                <div v-if="langMenuOpen" class="set-dropdown-menu" role="menu">
                  <button
                    v-for="o in langOptions"
                    :key="o.v"
                    class="set-dropdown-item"
                    :class="{ active: lang === o.v }"
                    role="menuitem"
                    @click.stop="pickLang(o.v)"
                  >
                    <span class="set-dropdown-check"><IconCheck v-if="lang === o.v" /></span>
                    <span>{{ t(o.key) }}</span>
                  </button>
                </div>
              </div>
            </header>
          </section>

          <!-- 主题 -->
          <section class="set-section">
            <header class="set-section-head">
              <IconPalette />
              <span class="set-section-title">{{ t('settings.section.theme') }}</span>
              <div ref="themeWrapEl" class="set-dropdown-wrap">
                <button
                  class="set-dropdown-btn"
                  :class="{ active: themeMenuOpen }"
                  @click.stop="themeMenuOpen = !themeMenuOpen; langMenuOpen = false; terminalMenuOpen = false"
                >
                  <span class="theme-swatch theme-swatch-sm" :class="`theme-swatch-${theme}`">Aa</span>
                  <span>{{ currentThemeLabel }}</span>
                  <IconChevronDown class="set-dropdown-chev" />
                </button>
                <div v-if="themeMenuOpen" class="set-dropdown-menu" role="menu">
                  <button
                    v-for="o in themeOptions"
                    :key="o.v"
                    class="set-dropdown-item"
                    :class="{ active: theme === o.v }"
                    role="menuitem"
                    @click.stop="pickTheme(o.v)"
                  >
                    <span class="set-dropdown-check"><IconCheck v-if="theme === o.v" /></span>
                    <span class="theme-swatch theme-swatch-sm" :class="`theme-swatch-${o.v}`">Aa</span>
                    <span>{{ t(o.key) }}</span>
                  </button>
                </div>
              </div>
            </header>
          </section>

          <!-- 数据 -->
          <section class="set-section">
            <header class="set-section-head">
              <IconDatabase />
              <span class="set-section-title">{{ t('settings.section.data') }}</span>
              <span class="set-section-tail">{{ cacheLabel }}</span>
            </header>
            <p class="set-section-desc">{{ t('settings.clearCacheDesc') }}</p>
            <button
              class="btn danger"
              :disabled="cacheBytes === 0"
              @click="emit('clearCache')"
            >
              {{ t('settings.clearCache') }}
            </button>
          </section>

          <!-- 关于 -->
          <section class="set-section">
            <header class="set-section-head">
              <IconInfo />
              <span class="set-section-title">{{ t('settings.section.about') }}</span>
              <span class="set-section-tail mono">v{{ version }}</span>
            </header>
            <p v-if="updateMsg" class="set-section-desc">{{ updateMsg }}</p>
            <div class="set-update-actions">
              <button class="btn" :disabled="checking" @click="doCheck">
                <IconRefresh v-if="!checking" />
                {{ checking ? t('settings.checking') : t('settings.checkUpdate') }}
              </button>
              <button
                v-if="updateAvailable"
                class="btn primary"
                @click="openReleasePage()"
              >
                <IconExternalLink />
                {{ t('settings.viewRelease', { v: latestVersion ?? '' }) }}
              </button>
            </div>
          </section>
        </template>

        <template v-else>
          <!-- 终端 -->
          <section class="set-section">
            <header class="set-section-head">
              <IconTerminal />
              <span class="set-section-title">{{ t('settings.section.terminal') }}</span>
              <div ref="terminalWrapEl" class="set-dropdown-wrap terminal-dropdown">
                <button
                  class="set-dropdown-btn terminal-dropdown-btn"
                  :class="{ active: terminalMenuOpen }"
                  @click.stop="terminalMenuOpen = !terminalMenuOpen; langMenuOpen = false; themeMenuOpen = false"
                >
                  <IconTerminal class="terminal-dropdown-ic" />
                  <span>{{ currentTerminalLabel }}</span>
                  <IconChevronDown class="set-dropdown-chev" />
                </button>
                <div v-if="terminalMenuOpen" class="set-dropdown-menu" role="menu">
                  <button
                    v-for="o in terminalOptions"
                    :key="o.v"
                    class="set-dropdown-item"
                    :class="{ active: terminalApp === o.v }"
                    role="menuitem"
                    @click.stop="pickTerminal(o.v)"
                  >
                    <span class="set-dropdown-check"><IconCheck v-if="terminalApp === o.v" /></span>
                    <IconTerminal class="terminal-dropdown-item-ic" />
                    <span>{{ t(o.key) }}</span>
                  </button>
                </div>
              </div>
            </header>
            <p class="set-section-desc set-toggle-hint">{{ t('settings.terminalDesc') }}</p>
          </section>

          <!-- Codex -->
          <section class="set-section">
            <header class="set-section-head">
              <span class="set-section-title">Codex</span>
            </header>
            <label class="set-toggle-row" @click.prevent="setCodexShowInternalSessions(!codexShowInternalSessions)">
              <span class="set-toggle-label">{{ t('settings.codex.showInternal') }}</span>
              <span class="set-toggle-track" :class="{ on: codexShowInternalSessions }">
                <span class="set-toggle-thumb" />
              </span>
            </label>
            <label class="set-toggle-row" @click.prevent="setCodexShowArchivedSessions(!codexShowArchivedSessions)">
              <span class="set-toggle-label">{{ t('settings.codex.showArchived') }}</span>
              <span class="set-toggle-track" :class="{ on: codexShowArchivedSessions }">
                <span class="set-toggle-thumb" />
              </span>
            </label>
            <p class="set-section-desc set-toggle-hint">{{ t('settings.codexVisibilityDesc') }}</p>
          </section>
        </template>
      </div>
    </div>
  </div>
</template>
