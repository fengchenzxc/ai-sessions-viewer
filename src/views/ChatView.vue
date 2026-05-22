<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import type { Agent, Msg, SessionMeta, Block } from '../types'
import { renderText, formatTime, isCaveatOnlyMsg, parseSystemEvent } from '../format'
import { t } from '../i18n'
import ToolResult from '../components/ToolResult.vue'
import CollapsibleBox from '../components/CollapsibleBox.vue'
import VueEasyLightbox from 'vue-easy-lightbox'
import {
  search,
  searchCount,
  searchIndex,
  searchScope,
  setSearchNavigator,
  toolsCollapsed,
} from '../chatToolbar'
import {
  IconArrowLeft,
  IconRefresh,
  IconTrash,
  IconRestore,
  IconPlay,
  IconFolder,
  IconArrowUp,
  IconArrowDown,
  IconChevronRight,
  IconPencil,
  IconCopy,
  IconDownload,
  IconMarkdown,
  IconHtml,
  agentIcons,
} from '../components/icons'

const props = defineProps<{
  agent: Agent
  session: SessionMeta
  messages: Msg[]
  /** 会话来自回收站 —— 只读查看，隐藏 重命名/恢复终端/删除/导出 等操作。 */
  trashed?: boolean
}>()

defineEmits<{
  back: []
  refresh: []
  delete: []
  resume: []
  rename: []
  reveal: []
  copyId: []
  exportMd: []
  exportHtml: []
  restore: []
}>()

function shortId(id: string): string {
  if (!id) return ''
  return id.length > 8 ? id.slice(0, 8) : id
}

function isToolOnly(m: Msg): boolean {
  return m.role === 'user' && m.blocks.every((b) => b.kind === 'tool_result')
}

function toolLabel(b: Block): string {
  if (b.kind === 'tool_use') return t('tool.call', { name: b.toolName ?? '' })
  if (b.kind === 'thinking') return t('tool.thinking')
  return ''
}

// 这几个工具会让 tool_result 携带 structuredPatch / 文件 diff，需要单独以
// 一个块呈现，便于一眼看到改动；其它工具（Read / Bash / TaskUpdate / Grep …）
// 的结果只是文本输出，嵌入到 Tool call 内部更紧凑。
const FILE_MUTATING_TOOLS = new Set([
  'Write',
  'Edit',
  'MultiEdit',
  'NotebookEdit',
  'apply_patch',
])

// 搜索范围分类 —— 给 .msg-row / tool_use <details> 打 data-search-scope，
// applySearch 沿祖先链找最近的 scope 决定是否收录该文本节点。
//   'user' / 'assistant'：用户消息 / 助手文本（含 thinking）
//   'tools-edit'：文件改动型工具（与 'agent' 选项合并）
//   'tools-other'：其它工具调用（与 'tools' 选项匹配）
function rowScope(m: Msg): string {
  // tool-only 行只在 FILE_MUTATING_TOOLS 的 tool_result 拆出来时出现，所以一定是 edit 类
  if (isToolOnly(m)) return 'tools-edit'
  return m.role
}
function toolUseScope(b: Block): string {
  return FILE_MUTATING_TOOLS.has(b.toolName ?? '') ? 'tools-edit' : 'tools-other'
}

const resultByToolId = computed(() => {
  const map = new Map<string, Block>()
  for (const m of props.messages) {
    for (const b of m.blocks) {
      if (b.kind === 'tool_result' && b.toolId) map.set(b.toolId, b)
    }
  }
  return map
})

const inlinedResultIds = computed(() => {
  const set = new Set<string>()
  for (const m of props.messages) {
    for (const b of m.blocks) {
      if (
        b.kind === 'tool_use' &&
        b.toolId &&
        !FILE_MUTATING_TOOLS.has(b.toolName ?? '') &&
        resultByToolId.value.has(b.toolId)
      ) {
        set.add(b.toolId)
      }
    }
  }
  return set
})

function inlinedResultFor(b: Block): Block | undefined {
  if (b.kind !== 'tool_use' || !b.toolId) return undefined
  if (!inlinedResultIds.value.has(b.toolId)) return undefined
  return resultByToolId.value.get(b.toolId)
}

function isInlinedResult(b: Block): boolean {
  return b.kind === 'tool_result' && !!b.toolId && inlinedResultIds.value.has(b.toolId)
}

function rowHasContent(m: Msg): boolean {
  // Local-command caveat user messages are pure plumbing — hide the row entirely.
  if (isCaveatOnlyMsg(m)) return false
  if (!isToolOnly(m)) return true
  return m.blocks.some((b) => !isInlinedResult(b))
}

const assistantName = computed(() =>
  props.agent === 'codex' ? 'Codex' : 'Claude',
)

function systemEventLabel(m: Msg): string | null {
  const ev = parseSystemEvent(m)
  if (!ev) return null
  if (ev.kind === 'rename') return t('chat.systemEvent.rename', { name: ev.name })
  return null
}

const stats = computed(() => {
  const u = props.messages.filter(
    (m) =>
      m.role === 'user' &&
      !isToolOnly(m) &&
      !isCaveatOnlyMsg(m) &&
      !systemEventLabel(m),
  ).length
  const a = props.messages.filter((m) => m.role === 'assistant').length
  return { u, a }
})

const lightboxVisible = ref(false)
const lightboxSrc = ref('')
function openLightbox(src: string) {
  lightboxSrc.value = src
  lightboxVisible.value = true
}

const scrollEl = ref<HTMLElement>()

// 自定义 rAF 平滑滚动：原生 behavior:'smooth' 在长会话里会随距离把动画拉长，
// 每帧又触发大段 reflow，所以 420 条消息时就会卡。这里用固定时长 + ease-out，
// 并在用户滚动/再次点击时打断。
let scrollRAF = 0
function cancelScroll() {
  if (scrollRAF) {
    cancelAnimationFrame(scrollRAF)
    scrollRAF = 0
  }
}
function smoothScrollTo(target: number) {
  const el = scrollEl.value
  if (!el) return
  cancelScroll()
  const start = el.scrollTop
  const dest = Math.max(0, Math.min(target, el.scrollHeight - el.clientHeight))
  const dist = dest - start
  if (Math.abs(dist) < 2) {
    el.scrollTop = dest
    return
  }
  // 距离越长动画稍微拉长一点，但封顶 360ms，避免长会话拖沓
  const duration = Math.min(360, 180 + Math.abs(dist) * 0.05)
  const t0 = performance.now()
  // easeOutCubic
  const ease = (p: number) => 1 - Math.pow(1 - p, 3)
  const step = (now: number) => {
    const p = Math.min(1, (now - t0) / duration)
    el.scrollTop = start + dist * ease(p)
    if (p < 1) {
      scrollRAF = requestAnimationFrame(step)
    } else {
      scrollRAF = 0
    }
  }
  // 用户主动滚动则中断
  const onUserScroll = () => {
    cancelScroll()
    el.removeEventListener('wheel', onUserScroll)
    el.removeEventListener('touchmove', onUserScroll)
  }
  el.addEventListener('wheel', onUserScroll, { passive: true, once: true })
  el.addEventListener('touchmove', onUserScroll, { passive: true, once: true })
  scrollRAF = requestAnimationFrame(step)
}
function scrollToTop() {
  smoothScrollTo(0)
}
function scrollToBottom() {
  const el = scrollEl.value
  if (el) smoothScrollTo(el.scrollHeight)
}

// 到顶 / 到底时分别隐藏对应方向的 FAB，留一点 8px 阈值避免抖动
const atTop = ref(true)
const atBottom = ref(true)
function updateEdges() {
  const el = scrollEl.value
  if (!el) return
  atTop.value = el.scrollTop <= 8
  atBottom.value = el.scrollTop + el.clientHeight >= el.scrollHeight - 8
}
let rafEdge = 0
function onScroll() {
  if (rafEdge) return
  rafEdge = requestAnimationFrame(() => {
    rafEdge = 0
    updateEdges()
  })
}
onMounted(() => {
  scrollEl.value?.addEventListener('scroll', onScroll, { passive: true })
  // 内容渲染完再算一次（长消息列表挂载后 scrollHeight 才稳定）
  requestAnimationFrame(updateEdges)
})
onUnmounted(() => {
  scrollEl.value?.removeEventListener('scroll', onScroll)
  if (rafEdge) cancelAnimationFrame(rafEdge)
  cancelScroll()
})

// ============================ 顶栏功能：折叠工具 / 搜索 ============================

const innerEl = ref<HTMLElement>()

// ---- 一键折叠/展开所有 <details> （工具调用 + thinking 块）
//
// 实现方式：当 toolsCollapsed 切换时，扫一遍 chat-inner 下所有 <details>，
// 把它们的 `open` 属性同步过去。之后用户单独点哪个 <summary> 仍然能再次展开 /
// 收起——直到下次点击 topbar 的折叠按钮全局再 sweep 一次。
function sweepDetails(open: boolean) {
  const root = innerEl.value
  if (!root) return
  for (const el of root.querySelectorAll<HTMLDetailsElement>('details')) {
    el.open = open
  }
}
watch(toolsCollapsed, (v) => sweepDetails(!v))

// ---- 消息内搜索：DOM walker 把匹配文本包成 <mark class="search-hit">
//
// 不修改渲染管线（renderText 走 v-html），而是渲染完之后再扫一遍 DOM，
// 把所有匹配的纯文本节点替换成带 <mark> 的片段。然后维护一组 mark 元素
// 让 ↑/↓ 按钮 / Enter 键能在它们之间跳转。messages / search 变化时整体重做。

let marks: HTMLElement[] = []
let searchDebounce = 0

function unmarkAll() {
  const root = innerEl.value
  if (!root) return
  const list = root.querySelectorAll<HTMLElement>('mark.search-hit')
  list.forEach((m) => {
    const parent = m.parentNode
    if (!parent) return
    parent.replaceChild(document.createTextNode(m.textContent ?? ''), m)
    parent.normalize()
  })
  marks = []
}

function applySearch() {
  unmarkAll()
  const root = innerEl.value
  const q = search.value.trim()
  if (!q || !root) {
    searchCount.value = 0
    searchIndex.value = 0
    return
  }
  const lower = q.toLowerCase()
  const filter = searchScope.value
  // 沿祖先链找到最近的 data-search-scope 标签，再决定是否计入当前筛选项。
  function scopeOk(parent: HTMLElement): boolean {
    if (filter === 'all') return true
    const node = parent.closest<HTMLElement>('[data-search-scope]')
    const scope = node?.dataset.searchScope ?? null
    if (filter === 'user') return scope === 'user'
    if (filter === 'agent') return scope === 'assistant' || scope === 'tools-edit'
    if (filter === 'tools') return scope === 'tools-other'
    return true
  }
  // 收集所有候选文本节点（跳过 <script>/<style>/已经是 mark 内部的）
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const txt = node.textContent
      if (!txt || !txt.toLowerCase().includes(lower)) return NodeFilter.FILTER_REJECT
      const parent = (node as Text).parentElement
      if (!parent) return NodeFilter.FILTER_REJECT
      // 不在脚本/样式里搜
      const tag = parent.tagName
      if (tag === 'SCRIPT' || tag === 'STYLE') return NodeFilter.FILTER_REJECT
      if (!scopeOk(parent)) return NodeFilter.FILTER_REJECT
      return NodeFilter.FILTER_ACCEPT
    },
  })
  const targets: Text[] = []
  let n: Node | null
  while ((n = walker.nextNode())) targets.push(n as Text)

  const collected: HTMLElement[] = []
  for (const text of targets) {
    const s = text.data
    const lowerS = s.toLowerCase()
    const frag = document.createDocumentFragment()
    let cur = 0
    let idx = lowerS.indexOf(lower, cur)
    while (idx >= 0) {
      if (idx > cur) frag.appendChild(document.createTextNode(s.slice(cur, idx)))
      const mark = document.createElement('mark')
      mark.className = 'search-hit'
      mark.textContent = s.slice(idx, idx + lower.length)
      frag.appendChild(mark)
      collected.push(mark)
      cur = idx + lower.length
      idx = lowerS.indexOf(lower, cur)
    }
    if (cur < s.length) frag.appendChild(document.createTextNode(s.slice(cur)))
    text.parentNode?.replaceChild(frag, text)
  }
  marks = collected
  searchCount.value = marks.length
  searchIndex.value = marks.length > 0 ? 1 : 0
  setCurrentMark()
}

function setCurrentMark() {
  marks.forEach((m) => m.classList.remove('current'))
  if (searchIndex.value < 1 || searchIndex.value > marks.length) return
  const target = marks[searchIndex.value - 1]
  target.classList.add('current')
  // 匹配可能藏在 collapsed 的 <details> 里 —— 沿着祖先链全部打开，确保可见
  let p: HTMLElement | null = target.parentElement
  while (p) {
    if (p.tagName === 'DETAILS' && !(p as HTMLDetailsElement).open) {
      ;(p as HTMLDetailsElement).open = true
    }
    p = p.parentElement
  }
  // 不用 smooth scroll：长会话里成百上千次跳转会卡，且我们已有自定义滚动 RAF。
  // block: 'center' 让 mark 出现在视区中部，符合搜索体验直觉。
  target.scrollIntoView({ block: 'center' })
}

function navigateMatches(dir: 1 | -1) {
  if (marks.length === 0) return
  const next = ((searchIndex.value - 1 + dir + marks.length) % marks.length) + 1
  searchIndex.value = next
  setCurrentMark()
}

watch(search, () => {
  // 短文本输入会快速变更，debounce 避免每按一键都重写一遍 DOM
  window.clearTimeout(searchDebounce)
  searchDebounce = window.setTimeout(applySearch, 120)
})

// 切换搜索范围时立即重做（不 debounce —— 是离散操作）
watch(searchScope, () => {
  if (search.value) applySearch()
})

// 消息变化（切换会话 / 刷新）后重新建立标记 + 重新 sweep 折叠态
watch(
  () => props.messages,
  () => {
    nextTick(() => {
      if (toolsCollapsed.value) sweepDetails(false)
      if (search.value) applySearch()
    })
  },
  { flush: 'post' },
)

onMounted(() => {
  setSearchNavigator(navigateMatches)
  document.addEventListener('click', onDocClick)
})
onUnmounted(() => {
  setSearchNavigator(null)
  window.clearTimeout(searchDebounce)
  unmarkAll()
  document.removeEventListener('click', onDocClick)
})

// 导出下拉菜单：点空白处关闭。锚定到导出按钮容器，点容器内的项不触发关闭。
const exportMenuOpen = ref(false)
const exportMenuEl = ref<HTMLElement>()
function toggleExportMenu(e: Event) {
  e.stopPropagation()
  exportMenuOpen.value = !exportMenuOpen.value
}
function onDocClick(e: MouseEvent) {
  if (!exportMenuOpen.value) return
  if (exportMenuEl.value && exportMenuEl.value.contains(e.target as Node)) return
  exportMenuOpen.value = false
}
</script>

<template>
  <div class="chat-head">
    <button class="icon-btn" v-tooltip="t('chat.back')" @click="$emit('back')">
      <IconArrowLeft />
    </button>
    <div class="chat-head-info">
      <div class="t">
        <span class="t-text">{{ session.title }}</span>
        <button
          v-if="!trashed"
          class="title-rename-ic"
          v-tooltip="t('chat.action.rename')"
          @click="$emit('rename')"
        >
          <IconPencil />
        </button>
      </div>
      <div class="s">
        <span>{{
          t('chat.stats', {
            u: stats.u,
            a: stats.a,
            time: formatTime(session.created),
          })
        }}</span>
        <span v-if="session.id" class="session-id" v-tooltip="session.id">
          <span class="session-id-label">{{ t('session.id') }}</span>
          <span class="session-id-text">{{ shortId(session.id) }}</span>
          <button
            class="session-id-copy"
            v-tooltip="t('chat.action.copyId')"
            @click="$emit('copyId')"
          >
            <IconCopy />
          </button>
        </span>
      </div>
    </div>
    <button
      v-if="!trashed"
      class="icon-btn"
      v-tooltip="t('chat.action.resume')"
      @click="$emit('resume')"
    >
      <IconPlay />
    </button>
    <button
      v-if="!trashed"
      class="icon-btn"
      v-tooltip="t('chat.action.reveal')"
      @click="$emit('reveal')"
    >
      <IconFolder />
    </button>
    <button
      v-if="!trashed"
      class="icon-btn"
      v-tooltip="t('chat.action.refresh')"
      @click="$emit('refresh')"
    >
      <IconRefresh />
    </button>
    <div v-if="!trashed" ref="exportMenuEl" class="export-menu-wrap">
      <button
        class="icon-btn"
        :class="{ active: exportMenuOpen }"
        v-tooltip:top="t('chat.tb.export.md') + ' / ' + t('chat.tb.export.html')"
        @click="toggleExportMenu"
      >
        <IconDownload />
      </button>
      <div v-if="exportMenuOpen" class="export-menu" role="menu">
        <button
          class="export-menu-item"
          role="menuitem"
          @click="exportMenuOpen = false; $emit('exportMd')"
        >
          <IconMarkdown />
          <span>{{ t('chat.tb.export.md') }}</span>
        </button>
        <button
          class="export-menu-item"
          role="menuitem"
          @click="exportMenuOpen = false; $emit('exportHtml')"
        >
          <IconHtml />
          <span>{{ t('chat.tb.export.html') }}</span>
        </button>
      </div>
    </div>
    <button
      v-if="!trashed"
      class="icon-btn danger"
      v-tooltip="t('chat.action.delete')"
      @click="$emit('delete')"
    >
      <IconTrash />
    </button>
    <button
      v-if="trashed"
      class="icon-btn chat-restore-btn"
      v-tooltip="t('trash.restore')"
      @click="$emit('restore')"
    >
      <IconRestore />
    </button>
  </div>

  <div ref="scrollEl" class="chat-scroll">
    <div ref="innerEl" class="chat-inner">
      <div
        v-for="(m, i) in messages"
        :key="m.uuid ?? i"
        v-show="rowHasContent(m)"
        class="msg-row"
        :class="systemEventLabel(m) ? 'system' : isToolOnly(m) ? 'tool-only' : m.role"
        :data-search-scope="rowScope(m)"
      >
        <!-- System events (e.g. /rename) render as a small centered line,
             not a "Me" bubble — they're meta facts, not user prose. -->
        <div v-if="systemEventLabel(m)" class="system-event">
          {{ systemEventLabel(m) }}
        </div>

        <div v-else-if="isToolOnly(m)" style="max-width: 86%; min-width: 0">
          <template v-for="(b, bi) in m.blocks" :key="bi">
            <ToolResult v-if="!isInlinedResult(b)" :block="b" />
          </template>
        </div>

        <div v-else class="bubble" :class="m.role">
          <div class="role-tag">
            <span class="name">
              <component
                v-if="m.role === 'assistant'"
                :is="agentIcons[agent]"
                class="agent-icon"
                :class="agent"
              />
              {{ m.role === 'user' ? t('chat.role.me') : assistantName }}
            </span>
            <span v-if="m.model" class="tool-chip">{{ m.model }}</span>
            <span v-if="m.sidechain" class="sidechain-badge">
              {{ t('chat.badge.subtask') }}
            </span>
            <span>{{ formatTime(m.timestamp) }}</span>
          </div>

          <CollapsibleBox :enabled="m.role === 'user'" :max-height="320">
            <template v-for="(b, bi) in m.blocks" :key="bi">
              <div v-if="b.kind === 'text'" class="text-run" v-html="renderText(b.text ?? '')" />

              <div
                v-else-if="b.kind === 'image' && b.imageSrc"
                class="inline-image-wrap"
                @click="openLightbox(b.imageSrc)"
              >
                <img
                  :src="b.imageSrc"
                  class="inline-image"
                  loading="lazy"
                  alt=""
                />
              </div>

              <details
                v-else-if="b.kind === 'thinking'"
                class="block-card"
                :class="{ 'in-user': m.role === 'user' }"
              >
                <summary class="block-summary">
                  <span class="chev"><IconChevronRight /></span>
                  <span class="label">{{ toolLabel(b) }}</span>
                </summary>
                <div class="block-body"><pre>{{ b.text }}</pre></div>
              </details>

              <details
                v-else-if="b.kind === 'tool_use'"
                class="block-card"
                :class="{ 'in-user': m.role === 'user' }"
                :data-search-scope="toolUseScope(b)"
              >
                <summary class="block-summary">
                  <span class="chev"><IconChevronRight /></span>
                  <span class="label">{{ toolLabel(b) }}</span>
                </summary>
                <div class="block-body">
                  <pre>{{ b.toolInput }}</pre>
                  <ToolResult
                    v-if="inlinedResultFor(b)"
                    :block="inlinedResultFor(b)!"
                  />
                </div>
              </details>

              <ToolResult
                v-else-if="b.kind === 'tool_result' && !isInlinedResult(b)"
                :block="b"
                :in-user="m.role === 'user'"
              />
            </template>
          </CollapsibleBox>
        </div>
      </div>

      <div v-if="!messages.length" class="empty" style="height: 200px">
        <div>{{ t('chat.empty') }}</div>
      </div>
    </div>
  </div>

  <div v-if="messages.length" class="scroll-fab">
    <button
      v-if="!atTop"
      class="fab"
      v-tooltip="t('chat.action.top')"
      @click="scrollToTop"
    >
      <IconArrowUp />
    </button>
    <button
      v-if="!atBottom"
      class="fab"
      v-tooltip="t('chat.action.bottom')"
      @click="scrollToBottom"
    >
      <IconArrowDown />
    </button>
  </div>

  <VueEasyLightbox
    :visible="lightboxVisible"
    :imgs="lightboxSrc"
    @hide="lightboxVisible = false"
  />
</template>
