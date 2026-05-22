<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import type { TrashItem } from '../../types'
import { t } from '../../i18n'
import { shortName } from '../../format'
import {
  trashSearch,
  trashSort,
  trashProject,
  selectMode,
  selectedTrash,
  exitSelectMode,
  filterTrash,
  trashProjects,
} from '../../trashToolbar'
import {
  IconSearch,
  IconClose,
  IconSort,
  IconSelect,
  IconRestore,
  IconCheck,
  IconChevronDown,
} from '../icons'

const props = defineProps<{ items: TrashItem[] }>()
const emit = defineEmits<{ (e: 'batch-restore'): void }>()

const hasQuery = computed(() => trashSearch.value.length > 0)
const projects = computed(() => trashProjects(props.items))

// 当前筛选下可见的条目 —— 全选 / 计数都基于它。
const visible = computed(() => filterTrash(props.items))
// 选中数按「仍存在于回收站里的条目」算：单条恢复/删除后选择集合可能残留失效 key。
const selectedCount = computed(
  () => props.items.filter((it) => selectedTrash.value.has(it.trashFile)).length,
)
const allSelected = computed(
  () =>
    visible.value.length > 0 &&
    visible.value.every((it) => selectedTrash.value.has(it.trashFile)),
)

function clearSearch() {
  trashSearch.value = ''
}
function toggleSort() {
  trashSort.value = trashSort.value === 'recent' ? 'oldest' : 'recent'
}
function toggleSelectAll() {
  const next = new Set(selectedTrash.value)
  for (const it of visible.value) {
    if (allSelected.value) next.delete(it.trashFile)
    else next.add(it.trashFile)
  }
  selectedTrash.value = next
}

// ⌘F / Ctrl+F：回收站打开时拦截系统 Find，聚焦搜索框并全选。
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

// 项目筛选下拉 —— 与 ChatTopbar 的 scope 下拉共用 .ct-scope-* 样式。
// 下拉项与按钮只显示项目短名（projectLabel 通常是一长串绝对路径）。
const projMenuOpen = ref(false)
const projMenuEl = ref<HTMLElement>()
const projLabel = computed(() =>
  trashProject.value === 'all'
    ? t('trash.tb.allProjects')
    : shortName(trashProject.value),
)
function toggleProjMenu(e: Event) {
  e.stopPropagation()
  projMenuOpen.value = !projMenuOpen.value
}
function pickProject(p: string) {
  trashProject.value = p
  projMenuOpen.value = false
}
function onDocClick(e: MouseEvent) {
  if (!projMenuOpen.value) return
  if (projMenuEl.value && projMenuEl.value.contains(e.target as Node)) return
  projMenuOpen.value = false
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
      <div ref="projMenuEl" class="ct-scope-wrap">
        <button
          type="button"
          class="ct-scope-btn"
          :class="{ active: projMenuOpen }"
          v-tooltip:right="t('trash.tb.projectFilter')"
          @click="toggleProjMenu"
        >
          <span class="ct-scope-label">{{ projLabel }}</span>
          <IconChevronDown class="ct-scope-chev" />
        </button>
        <div v-if="projMenuOpen" class="ct-scope-menu" role="menu">
          <button
            type="button"
            class="ct-scope-item"
            :class="{ active: trashProject === 'all' }"
            role="menuitemradio"
            :aria-checked="trashProject === 'all'"
            @click="pickProject('all')"
          >
            <span class="ct-scope-check">
              <IconCheck v-if="trashProject === 'all'" />
            </span>
            <span>{{ t('trash.tb.allProjects') }}</span>
          </button>
          <button
            v-for="p in projects"
            :key="p"
            type="button"
            class="ct-scope-item"
            :class="{ active: trashProject === p }"
            role="menuitemradio"
            :aria-checked="trashProject === p"
            @click="pickProject(p)"
          >
            <span class="ct-scope-check">
              <IconCheck v-if="trashProject === p" />
            </span>
            <span>{{ shortName(p) }}</span>
          </button>
        </div>
      </div>
      <span class="ct-search-ic"><IconSearch /></span>
      <input
        ref="searchInput"
        v-model="trashSearch"
        type="text"
        class="ct-search-input"
        :placeholder="t('trash.tb.searchPlaceholder')"
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
      <template v-if="selectMode">
        <span class="ct-search-count">{{
          t('trash.tb.selectedCount', { n: selectedCount })
        }}</span>
        <button
          class="ct-btn"
          :class="{ active: allSelected }"
          v-tooltip="allSelected ? t('trash.tb.selectNone') : t('trash.tb.selectAll')"
          @click="toggleSelectAll"
        >
          <IconCheck />
        </button>
        <button
          class="ct-btn"
          :disabled="selectedCount === 0"
          v-tooltip="t('trash.tb.restoreSelected')"
          @click="emit('batch-restore')"
        >
          <IconRestore />
        </button>
        <button
          class="ct-btn"
          v-tooltip="t('trash.tb.selectCancel')"
          @click="exitSelectMode"
        >
          <IconClose />
        </button>
      </template>
      <!-- 排序与批量选择都只在 2 条以上才有意义，单条 / 空时不显示。 -->
      <template v-else-if="items.length > 1">
        <button
          class="ct-btn"
          v-tooltip="
            trashSort === 'recent'
              ? t('trash.tb.sortRecent')
              : t('trash.tb.sortOldest')
          "
          @click="toggleSort"
        >
          <IconSort />
        </button>
        <button
          class="ct-btn"
          v-tooltip="t('trash.tb.select')"
          @click="selectMode = true"
        >
          <IconSelect />
        </button>
      </template>
    </div>
  </div>
</template>
