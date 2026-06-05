<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import type { Agent, Msg, SessionMeta, Block } from '../types'
import { renderText, formatTime, isCaveatOnlyMsg, parseSystemEvent } from '../format'
import { prettifyAndHighlightJson } from '../jsonHighlight'
import { renderAllMermaid, resetMermaidForTheme } from '../mermaid'
import { theme } from '../settings'
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
  IconJson,
  IconChart,
  IconFold,
  IconUnfold,
  agentIcons,
} from '../components/icons'

const props = defineProps<{
  agent: Agent
  session: SessionMeta
  messages: Msg[]
  /** 会话来自回收站 —— 只读查看，隐藏 重命名/恢复终端/删除/导出 等操作。 */
  trashed?: boolean
  /** Live tail 状态：后端正在追这条 JSONL；为 true 时显示 "● Live" 徽章。 */
  live?: boolean
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
  exportJson: []
  restore: []
  /** 打开会话统计页 —— 原本住在 ChatTopbar 里，现挪进 chat-head 减少
   *  topbar + chat-head 两排 icon-only 按钮重叠的扫描负担。 */
  openSessionStats: []
}>()

function toggleTools() {
  toolsCollapsed.value = !toolsCollapsed.value
}

const canResume = computed(() => !props.trashed && !!props.session.id)

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
  props.agent === 'codex'
    ? 'Codex'
    : props.agent === 'gemini'
      ? 'Gemini'
      : 'Claude',
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

// 跳转到某条消息：滚到对应 .msg-row，触发一次 .msg-flash 闪烁动画。
// 全局搜索点击命中后被 App.vue 通过 defineExpose 调用。idx 与 uuid 双兜底
// —— uuid 在场用 uuid 找（更稳，能扛重排），否则按 data-msg-idx 找。
//
// 长会话的滚动「不准」问题：
//   1) chatMsgs 被赋值后，巨型 v-for 要一两帧才把 .msg-row 真正挂上 DOM；
//   2) 挂上之后，里头的代码高亮 / DiffBlock / 图片还会异步把内容塞进去，
//      命中行的 offsetTop 会继续往下推。
// 应对：先「等 row 出现」最多 ~500ms；找到之后启动一个 rAF 循环，每帧重读
// offsetTop 让滚动追上后涨的高度；动画窗口（~360ms）结束后再校准 ~1.2s。
// 任何 wheel / pointerdown / keydown 都立即让位，绝不和用户抢滚动条。
const flashIdx = ref<number | null>(null)
let flashTimer = 0
let flashStickCleanup: (() => void) | null = null
function cancelFlashStick() {
  flashStickCleanup?.()
}
function flashMessage(idx: number, uuid?: string) {
  const inner = innerEl.value
  const sa = scrollEl.value
  if (!inner || !sa) return
  const findRow = () =>
    (uuid
      ? inner.querySelector<HTMLElement>(`.msg-row[data-msg-uuid="${CSS.escape(uuid)}"]`)
      : null) ?? inner.querySelector<HTMLElement>(`.msg-row[data-msg-idx="${idx}"]`)

  // 先取消上一个跳转的尾巴 + 任何在跑的平滑滚动。
  cancelFlashStick()
  cancelScroll()

  // Step 1：等 row 挂上 DOM —— readSession 返回后大长 v-for 通常 1-2 帧搞定，
  // 但留 30 帧（≈500ms）兜底，超过仍找不到就放弃。
  let waitFrames = 0
  const start = () => {
    const first = findRow()
    if (!first) {
      if (++waitFrames > 30) return
      requestAnimationFrame(start)
      return
    }
    run(first)
  }

  // Step 2：自带 rAF 循环 —— 不复用 smoothScrollTo，因为它把目标缓存为常量，
  // 长会话里目标会随子组件渲染往下挪，必须每帧重新读 offsetTop。
  const run = (first: HTMLElement) => {
    const startScroll = sa.scrollTop
    const firstTarget = Math.max(0, first.offsetTop - 80)
    const initDist = firstTarget - startScroll
    const duration = Math.min(360, 180 + Math.abs(initDist) * 0.05)
    const ease = (p: number) => 1 - Math.pow(1 - p, 3)
    const t0 = performance.now()
    // 总「贴靠」时长：动画 ~360ms + 校准 ~1.2s。校准期是为了等图片 / 代码块
    // 异步渲染完后还能把命中行拉回正确位置。
    const STICK_MS = 1600

    let userBailed = false
    const onUserInput = () => {
      userBailed = true
    }
    sa.addEventListener('wheel', onUserInput, { passive: true })
    sa.addEventListener('pointerdown', onUserInput, { passive: true })
    sa.addEventListener('keydown', onUserInput)

    let raf = 0
    const tick = () => {
      if (userBailed) return cleanup()
      const now = performance.now()
      const elapsed = now - t0
      if (elapsed > STICK_MS) return cleanup()

      // 每帧重新拿引用：keep-alive 之类的边界场景下 row 节点可能被换掉。
      const cur = findRow()
      if (!cur) {
        raf = requestAnimationFrame(tick)
        return
      }
      const target = Math.max(0, cur.offsetTop - 80)

      if (elapsed < duration) {
        // 动画阶段：用 ease 平滑滚到 target；target 每帧都重新读，自然追得上后涨高度
        const p = elapsed / duration
        sa.scrollTop = startScroll + (target - startScroll) * ease(p)
      } else {
        // 校准阶段：层出不穷的 1-2 像素抖动忽略；只在偏差明显时硬对齐
        if (Math.abs(sa.scrollTop - target) > 1) sa.scrollTop = target
      }
      raf = requestAnimationFrame(tick)
    }

    const cleanup = () => {
      if (raf) cancelAnimationFrame(raf)
      sa.removeEventListener('wheel', onUserInput)
      sa.removeEventListener('pointerdown', onUserInput)
      sa.removeEventListener('keydown', onUserInput)
      flashStickCleanup = null
    }
    flashStickCleanup = cleanup
    raf = requestAnimationFrame(tick)

    // 闪烁：先清状态，等下一帧再写，确保 CSS 动画从头跑。
    const realIdx = Number(first.dataset.msgIdx ?? idx)
    flashIdx.value = null
    requestAnimationFrame(() => {
      flashIdx.value = realIdx
      window.clearTimeout(flashTimer)
      flashTimer = window.setTimeout(() => {
        flashIdx.value = null
      }, 1400)
    })
  }

  start()
}
// ============================ Live tail: 自动跟随 + "N 条新" pill ============================
//
// 设计：当后端 emit session:append 后，App.vue 把新 Msg 推进 messages，
// 然后调 onLiveAppend(n) 让本组件决定怎么回应：
//   - 用户当前接近底部（100px 以内）→ 自动平滑滚到底，pill 不出现；
//   - 否则 → 在 pill 上累加 N，用户点 pill 才滚到底。
//
// 切换会话 / 关闭后重新打开同一会话时，watch(session.path) 把 newCount 归零，
// 避免把上一条会话的"未读"带到下一条。
const newCount = ref(0)
// 100px 阈值：比 atBottom 用的 8px 宽松得多，鼓励"贴着底"的常态自动跟随。
const FOLLOW_THRESHOLD = 100
function isNearBottom(): boolean {
  const el = scrollEl.value
  if (!el) return true
  return el.scrollTop + el.clientHeight >= el.scrollHeight - FOLLOW_THRESHOLD
}

function onLiveAppend(addedCount: number) {
  if (addedCount <= 0) return
  if (isNearBottom()) {
    // 等新行布局完成再滚 —— 否则 scrollHeight 还是旧值。
    requestAnimationFrame(() => {
      scrollToBottom()
      newCount.value = 0
    })
  } else {
    newCount.value += addedCount
  }
}

function jumpToNewest() {
  newCount.value = 0
  scrollToBottom()
}

// 切换到不同会话 → 清掉"未读"计数。
watch(
  () => props.session?.path,
  () => {
    newCount.value = 0
  },
)

defineExpose({ flashMessage, onLiveAppend })

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
  cancelFlashStick()
  window.clearTimeout(flashTimer)
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

// 消息变化（切换会话 / 刷新）后重新建立标记 + 重新 sweep 折叠态 + 渲染 mermaid 占位符
watch(
  () => props.messages,
  () => {
    nextTick(() => {
      if (toolsCollapsed.value) sweepDetails(false)
      if (search.value) applySearch()
      // 新挂载的 .md-mermaid 占位符 → 调 mermaid.render() 替换。幂等 ——
      // 已渲染过的会带 data-rendered 跳过。
      renderAllMermaid(innerEl.value ?? null)
    })
  },
  { flush: 'post' },
)

// 主题切换：mermaid 不能运行时换色，要把已渲染节点 reset 再 redraw。
watch(theme, () => {
  nextTick(() => {
    resetMermaidForTheme(innerEl.value ?? null)
    renderAllMermaid(innerEl.value ?? null)
  })
})

onMounted(() => {
  setSearchNavigator(navigateMatches)
  document.addEventListener('click', onDocClick)
  // 初次挂载也跑一遍 —— 会话已经有 messages 时 watch 不会触发。
  nextTick(() => renderAllMermaid(innerEl.value ?? null))
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
        <span
          v-if="live && !trashed"
          class="live-badge"
          v-tooltip="t('chat.live.tooltip')"
        >
          <span class="live-dot" />
          <span class="live-label">{{ t('chat.live') }}</span>
        </span>
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
    <!-- 会话统计 + 折叠 Tool calls：原本住在 ChatTopbar.ct-actions 里，
         与 chat-head 的 5 个会话级 icon 隔一行 40px topbar 在同一垂直线上。
         挪进 chat-head 后顶栏只剩 scope+search 一条横线。toolsCollapsed
         走 chatToolbar 模块 ref 共享，原 ChatTopbar 的对应按钮已删除。 -->
    <button
      class="icon-btn"
      v-tooltip="t('chat.tb.sessionStats')"
      @click="$emit('openSessionStats')"
    >
      <IconChart />
    </button>
    <button
      class="icon-btn"
      v-tooltip="
        toolsCollapsed
          ? t('chat.tb.tools.expand')
          : t('chat.tb.tools.collapse')
      "
      @click="toggleTools"
    >
      <component :is="toolsCollapsed ? IconUnfold : IconFold" />
    </button>
    <button
      v-if="!trashed"
      class="icon-btn"
      :class="{ disabled: !canResume }"
      v-tooltip="canResume ? t('chat.action.resume') : t('chat.action.resumeUnavailable')"
      :disabled="!canResume"
      @click="canResume && $emit('resume')"
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
        <button
          class="export-menu-item"
          role="menuitem"
          @click="exportMenuOpen = false; $emit('exportJson')"
        >
          <IconJson />
          <span>{{ t('chat.tb.export.json') }}</span>
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
        :class="[
          systemEventLabel(m) ? 'system' : isToolOnly(m) ? 'tool-only' : m.role,
          { 'msg-flash': flashIdx === i },
        ]"
        :data-search-scope="rowScope(m)"
        :data-msg-idx="i"
        :data-msg-uuid="m.uuid ?? ''"
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
                  <pre class="lang-json" v-html="prettifyAndHighlightJson(b.toolInput ?? '')" />
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
      v-if="newCount > 0"
      class="new-pill"
      @click="jumpToNewest"
    >
      {{ t('chat.newMessages', { n: newCount }) }}
    </button>
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
