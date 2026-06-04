<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { t } from '../i18n'
import {
  codexShowArchivedSessions,
  codexShowInternalSessions,
  lang,
  setTerminalApp,
  setCodexShowArchivedSessions,
  setCodexShowInternalSessions,
  setLang,
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
  IconTerminal,
  IconLanguages,
  IconPalette,
  IconDatabase,
  IconInfo,
  IconRefresh,
  IconExternalLink,
  IconArchive,
  IconShieldCheck,
} from './icons'
import * as api from '../api'
import {
  latestVersion,
  openReleasePage,
  syncFromManualCheck,
  updateAvailable,
} from '../updateCheck'

const props = defineProps<{ cacheBytes: number }>()
const emit = defineEmits<{ close: []; clearCache: [] }>()

const cacheLabel = computed(() =>
  props.cacheBytes > 0 ? formatSize(props.cacheBytes) : '0 B',
)

const version = ref('—')
const updateMsg = ref('')
const checking = ref(false)

onMounted(async () => {
  try {
    version.value = await api.appVersion()
  } catch {
    /* ignore */
  }
  // 如果后台检查在启动时就已经发现有新版本，开 Settings 时直接把同样的提示
  // 展示出来 —— 用户不必再点一次「检查更新」就能看到信息和「打开 release」链接。
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

async function doCheck() {
  if (checking.value) return
  checking.value = true
  updateMsg.value = t('settings.checking')
  try {
    const r = await api.checkUpdate()
    updateMsg.value = r.hasUpdate
      ? t('settings.updateAvailable', { v: r.latest, cur: r.current })
      : t('settings.upToDate', { v: r.current })
    // 同步给后台检查模块：手动结果是最新的真值，顺便刷新 24h TTL，
    // 让侧边栏小红点立即跟手动检查的结论一致。
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

      <div class="set-body">
        <!-- 语言 -->
        <section class="set-section">
          <header class="set-section-head">
            <IconLanguages />
            <span class="set-section-title">{{ t('settings.section.lang') }}</span>
          </header>
          <div class="segmented seg-wide">
            <button
              v-for="o in langOptions"
              :key="o.v"
              :class="{ active: lang === o.v }"
              @click="setLang(o.v)"
            >
              {{ t(o.key) }}
            </button>
          </div>
        </section>

        <!-- 主题 -->
        <section class="set-section">
          <header class="set-section-head">
            <IconPalette />
            <span class="set-section-title">{{ t('settings.section.theme') }}</span>
          </header>
          <div class="theme-select-wrap">
            <span class="theme-swatch" :class="`theme-swatch-${theme}`">Aa</span>
            <select
              class="theme-select"
              :value="theme"
              @change="setTheme(($event.target as HTMLSelectElement).value as Theme)"
            >
              <option v-for="o in themeOptions" :key="o.v" :value="o.v">
                {{ t(o.key) }}
              </option>
            </select>
          </div>
        </section>

        <!-- 终端 -->
        <section class="set-section">
          <header class="set-section-head">
            <IconTerminal />
            <span class="set-section-title">{{ t('settings.section.terminal') }}</span>
          </header>
          <div class="theme-grid">
            <button
              v-for="o in terminalOptions"
              :key="o.v"
              class="terminal-card"
              :class="{ active: terminalApp === o.v }"
              @click="setTerminalApp(o.v)"
            >
              <IconTerminal class="theme-card-ic" />
              <span class="theme-card-label">{{ t(o.key) }}</span>
            </button>
          </div>
        </section>

        <!-- Codex -->
        <section class="set-section">
          <header class="set-section-head">
            <IconShieldCheck />
            <span class="set-section-title">{{ t('settings.section.codex') }}</span>
          </header>
          <p class="set-section-desc">{{ t('settings.codexVisibilityDesc') }}</p>
          <div class="codex-toggle-grid">
            <button
              class="codex-toggle-card"
              :class="{ active: codexShowInternalSessions }"
              @click="setCodexShowInternalSessions(!codexShowInternalSessions)"
            >
              <IconShieldCheck class="theme-card-ic" />
              <span class="theme-card-label">{{ t('settings.codex.showInternal') }}</span>
            </button>
            <button
              class="codex-toggle-card"
              :class="{ active: codexShowArchivedSessions }"
              @click="setCodexShowArchivedSessions(!codexShowArchivedSessions)"
            >
              <IconArchive class="theme-card-ic" />
              <span class="theme-card-label">{{ t('settings.codex.showArchived') }}</span>
            </button>
          </div>
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
            <!-- 有新版本时多挂一个「打开 release」按钮（优先用 GitHub 返回的
                 html_url，没拿到就退回 /releases/latest）。primary 视觉权重更高，
                 让用户清楚下一步该点哪。 -->
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
      </div>
    </div>
  </div>
</template>
