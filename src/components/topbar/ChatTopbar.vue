<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { t } from '../../i18n'
import {
  search,
  searchCount,
  searchIndex,
  searchScope,
  toolsCollapsed,
  navigate,
} from '../../chatToolbar'
import {
  IconSearch,
  IconChevronUp,
  IconChevronDown,
  IconClose,
  IconFold,
  IconUnfold,
  IconCheck,
} from '../icons'
import type { SearchScope } from '../../chatToolbar'

const searchInput = ref<HTMLInputElement>()

// ⌘F / Ctrl+F：聊天页打开时（即本组件挂载时）拦截系统 Find，聚焦搜索框并全选。
// 只检测当前平台对应的修饰键，避免 macOS 上 Ctrl+F（光标右移）被误抢。
const isMac = /Mac/i.test(navigator.platform)
function onFindShortcut(e: KeyboardEvent) {
  if (e.key !== 'f' && e.key !== 'F') return
  const want = isMac ? e.metaKey : e.ctrlKey
  const other = isMac ? e.ctrlKey : e.metaKey
  if (!want || other || e.shiftKey || e.altKey) return
  e.preventDefault()
  searchInput.value?.focus()
  searchInput.value?.select()
}
onMounted(() => window.addEventListener('keydown', onFindShortcut))
onUnmounted(() => window.removeEventListener('keydown', onFindShortcut))

function onKeydown(e: KeyboardEvent) {
  // Enter / Shift+Enter 在搜索框里跳下一个 / 上一个
  if (e.key === 'Enter') {
    e.preventDefault()
    if (searchCount.value === 0) return
    navigate(e.shiftKey ? -1 : 1)
  } else if (e.key === 'Escape') {
    e.preventDefault()
    clearSearch()
  }
}

function clearSearch() {
  search.value = ''
  searchInput.value?.blur()
}

function toggleTools() {
  toolsCollapsed.value = !toolsCollapsed.value
}

const hasQuery = computed(() => search.value.length > 0)

// 自定义 scope 下拉（替代原生 <select>），跟导出菜单使用同一套样式
const scopeMenuOpen = ref(false)
const scopeMenuEl = ref<HTMLElement>()
const SCOPES: { value: SearchScope; key: string }[] = [
  { value: 'all', key: 'chat.tb.scope.all' },
  { value: 'user', key: 'chat.tb.scope.user' },
  { value: 'agent', key: 'chat.tb.scope.agent' },
  { value: 'tools', key: 'chat.tb.scope.tools' },
]
const scopeLabel = computed(() => {
  const found = SCOPES.find((s) => s.value === searchScope.value)
  return t(found?.key ?? 'chat.tb.scope.all')
})
function toggleScopeMenu(e: Event) {
  e.stopPropagation()
  scopeMenuOpen.value = !scopeMenuOpen.value
}
function pickScope(s: SearchScope) {
  searchScope.value = s
  scopeMenuOpen.value = false
}
function onDocClick(e: MouseEvent) {
  if (!scopeMenuOpen.value) return
  if (scopeMenuEl.value && scopeMenuEl.value.contains(e.target as Node)) return
  scopeMenuOpen.value = false
}
onMounted(() => document.addEventListener('click', onDocClick))
onUnmounted(() => document.removeEventListener('click', onDocClick))
</script>

<template>
  <div class="chat-topbar">
    <div class="ct-search" :class="{ active: hasQuery }">
      <div ref="scopeMenuEl" class="ct-scope-wrap">
        <button
          type="button"
          class="ct-scope-btn"
          :class="{ active: scopeMenuOpen }"
          v-tooltip:right="t('chat.tb.scope.tooltip')"
          @click="toggleScopeMenu"
        >
          <span class="ct-scope-label">{{ scopeLabel }}</span>
          <IconChevronDown class="ct-scope-chev" />
        </button>
        <div v-if="scopeMenuOpen" class="ct-scope-menu" role="menu">
          <button
            v-for="s in SCOPES"
            :key="s.value"
            type="button"
            class="ct-scope-item"
            :class="{ active: searchScope === s.value }"
            role="menuitemradio"
            :aria-checked="searchScope === s.value"
            @click="pickScope(s.value)"
          >
            <span class="ct-scope-check">
              <IconCheck v-if="searchScope === s.value" />
            </span>
            <span>{{ t(s.key) }}</span>
          </button>
        </div>
      </div>
      <span class="ct-search-ic"><IconSearch /></span>
      <input
        ref="searchInput"
        v-model="search"
        type="text"
        class="ct-search-input"
        :placeholder="t('chat.tb.search.placeholder')"
        spellcheck="false"
        autocomplete="off"
        @keydown="onKeydown"
      />
      <template v-if="hasQuery">
        <span class="ct-search-count" :class="{ none: searchCount === 0 }">
          {{
            searchCount === 0
              ? t('chat.tb.search.none')
              : t('chat.tb.search.count', { cur: searchIndex, total: searchCount })
          }}
        </span>
        <button
          class="ct-btn"
          :disabled="searchCount === 0"
          v-tooltip="t('chat.tb.search.prev')"
          @click="navigate(-1)"
        >
          <IconChevronUp />
        </button>
        <button
          class="ct-btn"
          :disabled="searchCount === 0"
          v-tooltip="t('chat.tb.search.next')"
          @click="navigate(1)"
        >
          <IconChevronDown />
        </button>
        <button
          class="ct-btn"
          v-tooltip="t('chat.tb.search.clear')"
          @click="clearSearch"
        >
          <IconClose />
        </button>
      </template>
    </div>

    <div class="ct-actions">
      <button
        class="ct-btn"
        v-tooltip="
          toolsCollapsed
            ? t('chat.tb.tools.expand')
            : t('chat.tb.tools.collapse')
        "
        @click="toggleTools"
      >
        <component :is="toolsCollapsed ? IconUnfold : IconFold" />
      </button>
    </div>
  </div>
</template>
