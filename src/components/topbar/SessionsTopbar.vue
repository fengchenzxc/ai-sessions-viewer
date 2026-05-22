<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { t } from '../../i18n'
import {
  sessionSearch,
  sessionSort,
  sessionWithIdOnly,
  type SessionSort,
} from '../../sessionsToolbar'
import { IconSearch, IconClose, IconHash, IconChevronDown, IconCheck } from '../icons'

const hasQuery = computed(() => sessionSearch.value.length > 0)

function clearSearch() {
  sessionSearch.value = ''
}

// 排序下拉 —— 复用 .ct-scope-* 样式（同 ChatTopbar 的 scope / TrashTopbar 的项目筛选）。
const SORTS: { value: SessionSort; key: string }[] = [
  { value: 'recent', key: 'list.tb.sortRecent' },
  { value: 'oldest', key: 'list.tb.sortOldest' },
  { value: 'size', key: 'list.tb.sortSize' },
  { value: 'messages', key: 'list.tb.sortMessages' },
]
const sortMenuOpen = ref(false)
const sortMenuEl = ref<HTMLElement>()
const sortLabel = computed(() => {
  const found = SORTS.find((s) => s.value === sessionSort.value)
  return t(found?.key ?? 'list.tb.sortRecent')
})
function toggleSortMenu(e: Event) {
  e.stopPropagation()
  sortMenuOpen.value = !sortMenuOpen.value
}
function pickSort(s: SessionSort) {
  sessionSort.value = s
  sortMenuOpen.value = false
}
function onDocClick(e: MouseEvent) {
  if (!sortMenuOpen.value) return
  if (sortMenuEl.value && sortMenuEl.value.contains(e.target as Node)) return
  sortMenuOpen.value = false
}

// ⌘F / Ctrl+F：会话列表打开时拦截系统 Find，聚焦搜索框并全选。
// 只检测当前平台对应的修饰键，避免 macOS 上 Ctrl+F（光标右移）被误抢。
const searchInput = ref<HTMLInputElement>()
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

onMounted(() => {
  document.addEventListener('click', onDocClick)
  window.addEventListener('keydown', onFindShortcut)
})
onUnmounted(() => {
  document.removeEventListener('click', onDocClick)
  window.removeEventListener('keydown', onFindShortcut)
})
</script>

<template>
  <div class="chat-topbar">
    <div class="ct-search" :class="{ active: hasQuery }">
      <div ref="sortMenuEl" class="ct-scope-wrap">
        <button
          type="button"
          class="ct-scope-btn"
          :class="{ active: sortMenuOpen }"
          v-tooltip:right="t('list.tb.sort')"
          @click="toggleSortMenu"
        >
          <span class="ct-scope-label">{{ sortLabel }}</span>
          <IconChevronDown class="ct-scope-chev" />
        </button>
        <div v-if="sortMenuOpen" class="ct-scope-menu" role="menu">
          <button
            v-for="s in SORTS"
            :key="s.value"
            type="button"
            class="ct-scope-item"
            :class="{ active: sessionSort === s.value }"
            role="menuitemradio"
            :aria-checked="sessionSort === s.value"
            @click="pickSort(s.value)"
          >
            <span class="ct-scope-check">
              <IconCheck v-if="sessionSort === s.value" />
            </span>
            <span>{{ t(s.key) }}</span>
          </button>
        </div>
      </div>
      <span class="ct-search-ic"><IconSearch /></span>
      <input
        ref="searchInput"
        v-model="sessionSearch"
        type="text"
        class="ct-search-input"
        :placeholder="t('list.tb.searchPlaceholder')"
        spellcheck="false"
        autocomplete="off"
      />
      <button
        v-if="hasQuery"
        class="ct-btn"
        v-tooltip="t('chat.tb.search.clear')"
        @click="clearSearch"
      >
        <IconClose />
      </button>
    </div>

    <div class="ct-actions">
      <button
        class="ct-btn"
        :class="{ active: sessionWithIdOnly }"
        v-tooltip="t('list.tb.withId')"
        @click="sessionWithIdOnly = !sessionWithIdOnly"
      >
        <IconHash />
      </button>
    </div>
  </div>
</template>
