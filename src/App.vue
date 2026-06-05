<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import type { Agent, ProjectInfo, SessionMeta, TrashItem, Msg } from './types'
import * as api from './api'
import { shortName } from './format'
import { t } from './i18n'
import {
  clearAppCache,
  codexShowArchivedSessions,
  codexShowInternalSessions,
  lang,
  setLang,
  terminalApp,
  setTheme,
  theme,
} from './settings'
import { focusSearchBox, navigate as chatNavigate, resetChatToolbar } from './chatToolbar'
import { emitMenuSync, installMenuRouter } from './menu'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { resetTrashToolbar, exitSelectMode, selectedTrash } from './trashToolbar'
import {
  resetSessionsToolbar,
  sessionsFilterActive,
  selectedSessions,
  exitSessionSelectMode,
} from './sessionsToolbar'
import {
  exportMarkdown,
  exportHtml,
  exportJson,
  exportMarkdownToDir,
  exportHtmlToDir,
  exportJsonToDir,
  pickExportDir,
  batchExportFolderName,
  type ExportKind,
} from './export'
import { fly } from './fly'
import { recordRecent } from './recents'
import { recordExport, type ExportRecord } from './exportHistory'
import { globalSearchOpen, openGlobalSearch } from './globalSearch'
import { runBackgroundCheck } from './updateCheck'
import type { SearchHit } from './types'
import ChatView from './views/ChatView.vue'
import SettingsModal from './components/SettingsModal.vue'
import ChatTopbar from './components/topbar/ChatTopbar.vue'
import TrashTopbar from './components/topbar/TrashTopbar.vue'
import SessionsTopbar from './components/topbar/SessionsTopbar.vue'
import TrashView from './views/TrashView.vue'
import SessionsView from './views/SessionsView.vue'
import WelcomeView from './views/WelcomeView.vue'
import StatsView from './views/StatsView.vue'
import ExportHistoryView from './views/ExportHistoryView.vue'
import PricingView from './views/PricingView.vue'
import Sidebar from './components/Sidebar.vue'
import SidebarTopbar from './components/SidebarTopbar.vue'
import ConfirmModal from './modals/ConfirmModal.vue'
import RenameModal from './modals/RenameModal.vue'
import GlobalSearchModal from './modals/GlobalSearchModal.vue'
import ProjectContextMenu from './modals/ProjectContextMenu.vue'

// ---------- 状态 ----------
const agent = ref<Agent>('claude')
const projects = ref<ProjectInfo[]>([])
const activeDir = ref<string | null>(null)
const showTrash = ref(false)
const showStats = ref(false)
const showExportHistory = ref(false)
const showPricing = ref(false)
const showSettings = ref(false)
const sidebarOpen = ref(true)
const refreshing = ref(false)
function toggleSidebar() {
  sidebarOpen.value = !sidebarOpen.value
}

const codexSessionOptions = computed(() => ({
  includeCodexInternal: codexShowInternalSessions.value,
  includeCodexArchived: codexShowArchivedSessions.value,
}))

function sessionListOptions() {
  return agent.value === 'codex' ? codexSessionOptions.value : undefined
}

/** 顶栏刷新：重新拉取项目 + 当前列表 + 当前打开的对话，全部静默，不动选中与滚动。 */
async function refreshAll() {
  if (refreshing.value) return
  refreshing.value = true
  const tasks: Promise<unknown>[] = []

  // 1. 项目列表（保留 activeDir）
  tasks.push(
    api.listProjects(agent.value, sessionListOptions()).then((p) => {
      projects.value = p
    }).catch(() => {}),
  )

  // 2. 当前列表（项目会话 or 回收站）
  if (showTrash.value) {
    tasks.push(
      api.listTrash().then((t) => {
        trash.value = t
      }).catch(() => {}),
    )
  } else if (activeDir.value) {
    const keepScroll = listScrollEl.value?.scrollTop ?? savedListScroll
    // 保留当前已加载数量，避免分页回退
    const n = Math.max(sessions.value.length, PAGE_SIZE)
    tasks.push(
      api
        .listSessions(agent.value, activeDir.value, 0, n, sessionListOptions())
        .then((page) => {
          sessions.value = page.sessions
          sessionTotal.value = page.total
          nextTick(() => {
            if (listScrollEl.value) listScrollEl.value.scrollTop = keepScroll
          })
        })
        .catch(() => {}),
    )
  }

  // 3. 当前打开的对话（如有）—— 静默替换 messages
  if (openSession.value) {
    tasks.push(
      api
        .readSession(agent.value, openSession.value.path)
        .then((msgs) => {
          chatMsgs.value = msgs
        })
        .catch(() => {}),
    )
  }

  try {
    await Promise.all(tasks)
  } finally {
    refreshing.value = false
  }
}
const sessions = ref<SessionMeta[]>([])
const sessionTotal = ref(0)
const loadingMore = ref(false)
const trash = ref<TrashItem[]>([])
const loadingList = ref(false)

const PAGE_SIZE = 40

const openSession = ref<SessionMeta | null>(null)
// 非空表示当前打开的会话来自回收站（只读查看）—— 详情页据此切换为「回收站模式」。
const openTrashItem = ref<TrashItem | null>(null)
const chatMsgs = ref<Msg[]>([])
const loadingChat = ref(false)
// "● Live" 徽章：仅当会话**确实正在被写入**时为 true。
//   - 打开时 mtime 距今 < FRESH_MS → 视作"刚才还在跑"，先亮起来
//   - 收到 session:append 事件 → 文件真的有新增 → 亮起 / 续命
//   - 安静 STALE_MS 后自动熄灭 —— CLI 进程通常已结束
// 这与"是否在后端追这个文件"分离：watcher 对所有非回收站会话都开，
// 否则用户从终端 resume 一个老会话时我们就漏掉了。
const liveTailing = ref(false)
// "Live"判定阈值，单位 ms
const LIVE_FRESH_MS = 3 * 60 * 1000 // 打开时：3 分钟内动过 → 算 live
const LIVE_STALE_MS = 2 * 60 * 1000 // append 后：2 分钟内没新动静 → 熄灭
let liveFadeTimer = 0
function markLive() {
  liveTailing.value = true
  window.clearTimeout(liveFadeTimer)
  liveFadeTimer = window.setTimeout(() => {
    liveTailing.value = false
  }, LIVE_STALE_MS)
}
function clearLive() {
  liveTailing.value = false
  window.clearTimeout(liveFadeTimer)
  liveFadeTimer = 0
}

// 单会话统计目标。非空 → StatsView 切换到 session 模式，scope 锁定到这条 JSONL。
// 与 showStats=true 联用：全局统计时此值为 null，会话统计时填上 {agent, path, title}。
const sessionStatsTarget = ref<{ agent: Agent; path: string; title?: string } | null>(null)
// 单会话统计是从哪进入的：决定「返回」按钮往哪走。
//   'chat'   ← ChatTopbar 的统计按钮（关闭 → 回到原聊天）
//   'global' ← 全局 StatsView Top Sessions 行点击（关闭 → 回到全局 StatsView）
const sessionStatsFrom = ref<'chat' | 'global' | null>(null)

const sessionsViewRef = ref<InstanceType<typeof SessionsView> | null>(null)
const chatViewRef = ref<InstanceType<typeof ChatView> | null>(null)
const listScrollEl = computed<HTMLElement | undefined>(
  () => sessionsViewRef.value?.scrollEl,
)
let savedListScroll = 0

watch(openSession, (val, old) => {
  // 切换 / 关闭会话时把聊天页顶栏（搜索 / 折叠 / 等）状态归零，
  // 否则前一个会话的搜索词 / 折叠态会留到下一个，体验古怪。
  if (val?.path !== old?.path) resetChatToolbar()
  // 关闭会话即退出回收站模式 —— openTrashItem 永远不残留到下一次打开。
  if (!val) openTrashItem.value = null
  // 切到别的会话 / 关闭会话 → 立刻让后端停掉旧 watcher。
  // openChat 里会再起新的；openTrashSession / null 都不需要 watcher。
  if (val?.path !== old?.path) {
    clearLive()
    api.unwatchSession().catch(() => {})
  }
  if (!val && old) {
    nextTick(() => {
      if (listScrollEl.value) listScrollEl.value.scrollTop = savedListScroll
    })
  }
})

const activeProject = computed(() =>
  projects.value.find((p) => p.dirName === activeDir.value),
)
// 详情页用的 agent：回收站会话用条目自己的 agent（可能与当前侧栏 agent 不同）。
const chatAgent = computed<Agent>(() => openTrashItem.value?.agent ?? agent.value)

// ---------- 项目置顶 / 沉底偏好（持久化到 localStorage）----------
type ProjState = 'pinned' | 'sunk'
const PREFS_KEY = 'projPrefs:v1'

function loadPrefs(): Record<string, ProjState> {
  try {
    return JSON.parse(localStorage.getItem(PREFS_KEY) || '{}')
  } catch {
    return {}
  }
}
const projPrefs = ref<Record<string, ProjState>>(loadPrefs())

function prefKey(p: ProjectInfo): string {
  return `${agent.value}::${p.dirName}`
}
function projStateOf(p: ProjectInfo): ProjState | undefined {
  return projPrefs.value[prefKey(p)]
}
function setProjState(p: ProjectInfo, state: ProjState) {
  const key = prefKey(p)
  if (projPrefs.value[key] === state) {
    delete projPrefs.value[key]
  } else {
    projPrefs.value[key] = state
  }
  projPrefs.value = { ...projPrefs.value }
  localStorage.setItem(PREFS_KEY, JSON.stringify(projPrefs.value))
}

// "缓存"目前只有置顶/沉底偏好这一项，字节数等于其 JSON 序列化后的 UTF-8 长度。
const cacheBytes = computed(() => {
  const json = JSON.stringify(projPrefs.value)
  if (json === '{}') return 0
  return new TextEncoder().encode(json).length
})

// ---------- 项目右键菜单 ----------
interface CtxMenu {
  x: number
  y: number
  project: ProjectInfo
}
const ctxMenu = ref<CtxMenu | null>(null)
function openCtxMenu(e: MouseEvent, p: ProjectInfo) {
  e.preventDefault()
  // 菜单大约 168×180，靠近视口右/下边时收回来一点，避免被截掉
  const W = 176
  const H = 180
  const x = Math.min(e.clientX, window.innerWidth - W - 8)
  const y = Math.min(e.clientY, window.innerHeight - H - 8)
  ctxMenu.value = { x, y, project: p }
}
function closeCtxMenu() {
  ctxMenu.value = null
}
function ctxToggleState(state: ProjState) {
  if (!ctxMenu.value) return
  setProjState(ctxMenu.value.project, state)
  closeCtxMenu()
}
function ctxRefresh() {
  closeCtxMenu()
  refreshAll()
}
function ctxDeleteProject() {
  const p = ctxMenu.value?.project
  closeCtxMenu()
  if (!p) return
  deleteProject(p)
}

// 删除当前打开的项目 —— SessionsView 顶部操作区的删除按钮。
function deleteActiveProject() {
  if (activeProject.value) deleteProject(activeProject.value)
}

function deleteProject(p: ProjectInfo) {
  ask({
    title: t('dialog.deleteProject.title'),
    message: t('dialog.deleteProject.body', {
      name: shortName(p.displayPath),
      n: p.sessionCount,
    }),
    okText: t('dialog.deleteProject.ok'),
    danger: true,
    onOk: async () => {
      // 在该项目从侧边栏移除前抓取起点，触发飞向回收站的弧线动画
      const srcRect = projectSourceRect(p)
      try {
        // 把该项目所有会话分页拉出来，再逐个软删；trash 里仍可逐个恢复
        const all: SessionMeta[] = []
        let offset = 0
        while (true) {
          const page = await api.listSessions(
            agent.value,
            p.dirName,
            offset,
            200,
            sessionListOptions(),
          )
          all.push(...page.sessions)
          offset += page.sessions.length
          if (all.length >= page.total || page.sessions.length === 0) break
        }
        for (const s of all) {
          try {
            await api.softDeleteSession(agent.value, s.path, p.displayPath)
          } catch {}
        }
        fly({
          from: srcRect,
          to: document.querySelector<HTMLElement>('.topbar-trash-btn'),
          variant: 'trash',
        })
        if (activeDir.value === p.dirName) {
          activeDir.value = null
          sessions.value = []
          openSession.value = null
        }
        await loadProjects()
        // 批量删除后刷新回收站，保持顶栏红点准确
        api.listTrash().then((items) => { trash.value = items }).catch(() => {})
        notify(t('toast.projDeleted'))
      } catch (e) {
        notify(t('toast.deleteFail', { e: String(e) }), true)
      }
    },
  })
}

// ---------- 确认弹窗 ----------
interface ConfirmState {
  show: boolean
  title: string
  message: string
  okText: string
  danger: boolean
  onOk: () => void
}
const confirm = ref<ConfirmState>({
  show: false,
  title: '',
  message: '',
  okText: '',
  danger: false,
  onOk: () => {},
})
function ask(opts: Partial<ConfirmState> & { onOk: () => void }) {
  confirm.value = {
    show: true,
    title: opts.title ?? t('common.confirm'),
    message: opts.message ?? '',
    okText: opts.okText ?? t('common.ok'),
    danger: opts.danger ?? false,
    onOk: opts.onOk,
  }
}
function runConfirm() {
  const fn = confirm.value.onOk
  confirm.value.show = false
  fn()
}

// ---------- 重命名会话 ----------
// 等价于 Claude Code 的 `/rename` —— 后端往原 JSONL 末尾追加官方 schema 的
// 元数据行（Claude 是 custom-title，Codex 是 event_msg.thread_name_updated），
// 不动用户对话内容，CLI 端再次读取这个会话时也会看到新名字。
interface RenameState {
  show: boolean
  agent: Agent
  path: string
  id: string
  value: string
  defaultTitle: string
}
const renameModal = ref<RenameState>({
  show: false,
  agent: 'claude',
  path: '',
  id: '',
  value: '',
  defaultTitle: '',
})
const renaming = ref(false)
function openRename(s: SessionMeta) {
  renameModal.value = {
    show: true,
    agent: agent.value,
    path: s.path,
    id: s.id,
    value: s.title,
    defaultTitle: s.title,
  }
}
async function confirmRename() {
  const m = renameModal.value
  if (!m.show || renaming.value) return
  const name = m.value.trim()
  if (!name || name === m.defaultTitle) {
    m.show = false
    return
  }
  renaming.value = true
  try {
    await api.renameSession(m.agent, m.path, name)
    // 立刻把内存里这条 session 的 title 更新成新名字，避免等下次刷新
    const patch = (s: SessionMeta) =>
      s.path === m.path ? { ...s, title: name } : s
    sessions.value = sessions.value.map(patch)
    if (openSession.value?.path === m.path) {
      openSession.value = { ...openSession.value, title: name }
    }
    m.show = false
    notify(t('toast.renamed'))
  } catch (e) {
    notify(t('toast.renameFail', { e: String(e) }), true)
  } finally {
    renaming.value = false
  }
}

// ---------- toast ----------
const toast = ref({ show: false, msg: '', error: false })
let toastTimer: number | undefined
function notify(msg: string, error = false) {
  toast.value = { show: true, msg, error }
  clearTimeout(toastTimer)
  toastTimer = window.setTimeout(() => (toast.value.show = false), 2600)
}

// ---------- 数据加载 ----------
async function loadProjects() {
  try {
    projects.value = await api.listProjects(agent.value, sessionListOptions())
  } catch (e) {
    notify(t('toast.loadProjectsFail', { e: String(e) }), true)
    projects.value = []
  }
}

function switchAgent(a: Agent) {
  if (agent.value === a) return
  agent.value = a
  activeDir.value = null
  sessions.value = []
  openSession.value = null
  showTrash.value = false
  showExportHistory.value = false
  showPricing.value = false
  // showStats 不重置 —— 统计是 agent-scoped，切 agent 后 StatsView 自己 refetch。
  loadProjects()
}

async function selectProject(dir: string) {
  // 再次点击当前已选中的项目：
  //   - 若右侧是会话详情 → 关闭详情，回到会话列表（不收起项目）
  //   - 若右侧已是会话列表 → 收起项目，回到「请选择项目」空状态
  if (activeDir.value === dir && !showTrash.value && !showStats.value) {
    if (openSession.value) {
      openSession.value = null
      return
    }
    activeDir.value = null
    sessions.value = []
    sessionTotal.value = 0
    resetSessionsToolbar()
    return
  }
  showTrash.value = false
  showStats.value = false
  showExportHistory.value = false
  showPricing.value = false
  sessionStatsTarget.value = null
  activeDir.value = dir
  recordRecent(agent.value, dir)
  openSession.value = null
  sessions.value = []
  sessionTotal.value = 0
  savedListScroll = 0
  resetSessionsToolbar()
  loadingList.value = true
  try {
    const page = await api.listSessions(agent.value, dir, 0, PAGE_SIZE, sessionListOptions())
    sessions.value = page.sessions
    sessionTotal.value = page.total
  } catch (e) {
    notify(t('toast.loadSessionsFail', { e: String(e) }), true)
    sessions.value = []
  } finally {
    loadingList.value = false
  }
}

async function loadMore() {
  if (loadingMore.value || loadingList.value || !activeDir.value) return
  if (sessions.value.length >= sessionTotal.value) return
  loadingMore.value = true
  try {
    const page = await api.listSessions(
      agent.value,
      activeDir.value,
      sessions.value.length,
      PAGE_SIZE,
      sessionListOptions(),
    )
    sessions.value.push(...page.sessions)
    sessionTotal.value = page.total
  } catch (e) {
    notify(t('toast.loadMoreFail', { e: String(e) }), true)
  } finally {
    loadingMore.value = false
  }
}

function onListScroll(scrollTop: number) {
  savedListScroll = scrollTop
}

// 一次性把当前项目剩余的会话全部拉进来。分页窗口只覆盖已滚动到的部分，
// 而搜索 / 排序需要面向整个项目才正确，故工具栏一旦被激活就补齐全量。
async function loadAllSessions() {
  if (!activeDir.value || loadingList.value || loadingMore.value) return
  if (sessions.value.length >= sessionTotal.value) return
  loadingMore.value = true
  try {
    const page = await api.listSessions(
      agent.value,
      activeDir.value,
      0,
      sessionTotal.value,
      sessionListOptions(),
    )
    sessions.value = page.sessions
    sessionTotal.value = page.total
  } catch (e) {
    notify(t('toast.loadMoreFail', { e: String(e) }), true)
  } finally {
    loadingMore.value = false
  }
}

// 工具栏从默认态切到「有筛选」时补齐全量会话；清空筛选后已加载的全量列表保留即可。
watch(sessionsFilterActive, (active) => {
  if (active) loadAllSessions()
})

async function refreshSessions() {
  if (!activeDir.value || loadingList.value) return
  loadingList.value = true
  try {
    const page = await api.listSessions(
      agent.value,
      activeDir.value,
      0,
      Math.max(PAGE_SIZE, sessions.value.length),
      sessionListOptions(),
    )
    sessions.value = page.sessions
    sessionTotal.value = page.total
  } catch (e) {
    notify(t('toast.loadSessionsFail', { e: String(e) }), true)
  } finally {
    loadingList.value = false
  }
}

// 打开统计概览：和回收站 / 会话视图互斥；再点一次同一按钮收起。
// 数据加载自身在 StatsView 里完成，App 这一层只切顶层状态。
function openStats() {
  if (showStats.value) {
    showStats.value = false
    sessionStatsTarget.value = null
    return
  }
  showStats.value = true
  // 全局统计模式：清掉单会话目标，避免上次留下来。
  sessionStatsTarget.value = null
  showTrash.value = false
  showExportHistory.value = false
  showPricing.value = false
  activeDir.value = null
  openSession.value = null
  sessions.value = []
  sessionTotal.value = 0
}

async function loadTrash() {
  showTrash.value = true
  showStats.value = false
  showExportHistory.value = false
  showPricing.value = false
  sessionStatsTarget.value = null
  activeDir.value = null
  openSession.value = null
  resetTrashToolbar()
  loadingList.value = true
  try {
    trash.value = await api.listTrash()
  } catch (e) {
    notify(t('toast.loadTrashFail', { e: String(e) }), true)
    trash.value = []
  } finally {
    loadingList.value = false
  }
}

async function openChat(s: SessionMeta) {
  loadingChat.value = true
  openTrashItem.value = null
  openSession.value = s
  chatMsgs.value = []
  clearLive()
  try {
    chatMsgs.value = await api.readSession(agent.value, s.path)
    // 整文件读完再开 watcher。watch_session 内部会把当前 Msg 数记为 baseline，
    // 后续只 emit 新增；read 之前开则可能把整段当 append 推回来。
    // watcher 始终启用 —— 即使会话当前看似"完成"，用户也可能从终端 resume，
    // 那一刻文件会重新被写，append 事件会把 Live 徽章亮起来。
    try {
      await api.watchSession(agent.value, s.path)
      // mtime 是毫秒。session.modified 由 agent 模块写入，单位与 now_millis 一致。
      const ageMs = Date.now() - (s.modified ?? 0)
      if (ageMs >= 0 && ageMs < LIVE_FRESH_MS) {
        markLive()
      }
    } catch {
      // watcher 起不来：不显示 Live（也不抛错 —— 只是失去自动刷新而已）
    }
  } catch (e) {
    notify(t('toast.readFail', { e: String(e) }), true)
    openSession.value = null
  } finally {
    loadingChat.value = false
  }
  // ⚠️ 这里曾经会顺手拉一次 api.sessionUsage 给顶栏角标用。后端 session_usage
  // 会全文件再扫一次 JSONL，长会话下明显拖累聊天首屏 —— 已经移到独立的会话
  // 统计页面，由用户点 ChatTopbar 的「统计」按钮按需触发（流式推送）。
}

function openExportHistory() {
  if (showExportHistory.value) {
    showExportHistory.value = false
    return
  }
  showExportHistory.value = true
  showPricing.value = false
  showTrash.value = false
  showStats.value = false
  sessionStatsTarget.value = null
  activeDir.value = null
  openSession.value = null
  sessions.value = []
  sessionTotal.value = 0
}

function openPricing() {
  if (showPricing.value) {
    showPricing.value = false
    return
  }
  showPricing.value = true
  showExportHistory.value = false
  showTrash.value = false
  showStats.value = false
  sessionStatsTarget.value = null
  activeDir.value = null
  openSession.value = null
  sessions.value = []
  sessionTotal.value = 0
}

async function openHistorySession(rec: ExportRecord) {
  const previousAgent = agent.value
  agent.value = rec.agent
  showExportHistory.value = false
  showPricing.value = false
  showTrash.value = false
  showStats.value = false
  sessionStatsTarget.value = null
  activeDir.value = null
  openTrashItem.value = null
  const s: SessionMeta = {
    id: rec.sessionId,
    fileName: rec.path.split('/').pop() || rec.sessionId || 'session.jsonl',
    path: rec.path,
    title: rec.title,
    cwd: rec.cwd,
    modified: rec.exportedAt,
    size: 0,
    messageCount: 0,
    codexAppListRank: null,
    codexAppListScanned: 0,
    codexAppFirstPageSize: 50,
    codexAppFirstPagePosition: 0,
    codexInternal: false,
    codexArchived: false,
  }
  try {
    if (previousAgent !== rec.agent) {
      projects.value = await api.listProjects(rec.agent, sessionListOptions())
    }
    await openChat(s)
  } catch (e) {
    notify(t('toast.readFail', { e: String(e) }), true)
  }
}

// 会话统计入口：从 ChatTopbar 的统计按钮触发，跳到独立统计页面。
// 走和全局统计一样的 SSE 推送通道，主聊天页面保持轻量 —— 后端 scope 拼成
// `session:<agent>:<path>`，由 stats::stream::run_session_scope 单独处理。
function openSessionStats() {
  if (!openSession.value) return
  const sess = openSession.value
  sessionStatsTarget.value = {
    agent: chatAgent.value,
    path: sess.path,
    title: sess.title,
  }
  sessionStatsFrom.value = 'chat'
  showStats.value = true
  showTrash.value = false
  // 注意：不清空 openSession / activeDir —— 用户关闭统计页时回到原会话上下文。
}

// 从全局 StatsView 的 Top Sessions 列表跳进单会话统计。和上面的区别只在 "from"，
// 决定返回时回到全局统计而不是某个聊天。
function openSessionStatsFromGlobal(a: Agent, path: string, title?: string) {
  sessionStatsTarget.value = { agent: a, path, title }
  sessionStatsFrom.value = 'global'
  // showStats 保持 true —— 我们仍然在 StatsView 里，只是 props.session 变了，
  // StatsView 内部的 watch(props.session?.path) 会重启流。
}

function closeStats() {
  // 单会话模式下点「返回」：根据进入路径决定回到哪
  if (sessionStatsTarget.value) {
    if (sessionStatsFrom.value === 'global') {
      // 仍留在 StatsView，但切回全局视图
      sessionStatsTarget.value = null
      sessionStatsFrom.value = null
      return
    }
    // 'chat' / null：完整关闭，openSession 还在 → 自动回落到 ChatView
  }
  showStats.value = false
  sessionStatsTarget.value = null
  sessionStatsFrom.value = null
}

// 在回收站里打开一个已删除会话的只读详情。回收站 JSONL 仍是完整文件，
// 直接按 trashPath 解析即可；详情页通过 openTrashItem 进入「回收站模式」。
async function openTrashSession(item: TrashItem) {
  loadingChat.value = true
  openTrashItem.value = item
  openSession.value = {
    id: '',
    fileName: item.trashFile,
    path: item.trashPath,
    title: item.title,
    modified: item.deletedAt,
    size: item.size,
    messageCount: 0,
    codexAppListRank: null,
    codexAppListScanned: 0,
    codexAppFirstPageSize: 50,
    codexAppFirstPagePosition: 0,
    codexInternal: false,
    codexArchived: false,
  }
  chatMsgs.value = []
  try {
    chatMsgs.value = await api.readSession(item.agent, item.trashPath)
  } catch (e) {
    notify(t('toast.readFail', { e: String(e) }), true)
    openSession.value = null
  } finally {
    loadingChat.value = false
  }
}

// ---------- 删除 / 恢复 ----------
// 删除起点矩形：列表里取对应 .session-card，详情页取聊天顶栏的删除按钮。
function deleteSourceRect(s: SessionMeta): DOMRect | null {
  const cards = document.querySelectorAll<HTMLElement>('.session-card')
  for (const c of cards) {
    if (c.dataset.path === s.path) return c.getBoundingClientRect()
  }
  const chatDel = document.querySelector<HTMLElement>('.chat-head .icon-btn.danger')
  return chatDel ? chatDel.getBoundingClientRect() : null
}

// 删除项目起点矩形：侧边栏里该项目的行。
function projectSourceRect(p: ProjectInfo): DOMRect | null {
  for (const el of document.querySelectorAll<HTMLElement>('.proj-item')) {
    if (el.dataset.path === p.displayPath) return el.getBoundingClientRect()
  }
  return null
}

// 恢复起点矩形：回收站列表里对应的 .session-card（按 trashFile 匹配），
// 在回收站详情页里恢复时没有列表卡片，改用顶栏的恢复按钮作起点。
function restoreSourceRect(item: TrashItem): DOMRect | null {
  for (const c of document.querySelectorAll<HTMLElement>('.session-card')) {
    if (c.dataset.trash === item.trashFile) return c.getBoundingClientRect()
  }
  const headBtn = document.querySelector<HTMLElement>('.chat-head .chat-restore-btn')
  return headBtn ? headBtn.getBoundingClientRect() : null
}

// 恢复落点：侧边栏里该会话所属项目的行（trashFile 的 projectLabel == 项目 displayPath）；
// 项目此刻尚未出现在侧边栏时退回到整个项目列表容器。
function restoreTarget(item: TrashItem): HTMLElement | null {
  for (const el of document.querySelectorAll<HTMLElement>('.proj-item')) {
    if (el.dataset.path === item.projectLabel) return el
  }
  return document.querySelector<HTMLElement>('.proj-list')
}

function deleteSession(s: SessionMeta) {
  ask({
    title: t('dialog.delete.title'),
    message: t('dialog.delete.body', { title: s.title }),
    okText: t('dialog.delete.ok'),
    onOk: async () => {
      // 在移除该行之前抓取起点，触发飞向回收站的弧线动画
      const srcRect = deleteSourceRect(s)
      try {
        await api.softDeleteSession(
          agent.value,
          s.path,
          activeProject.value?.displayPath ?? '',
        )
        fly({
          from: srcRect,
          to: document.querySelector<HTMLElement>('.topbar-trash-btn'),
          variant: 'trash',
        })
        sessions.value = sessions.value.filter((x) => x.path !== s.path)
        sessionTotal.value = Math.max(0, sessionTotal.value - 1)
        if (openSession.value?.path === s.path) openSession.value = null
        await loadProjects()
        // 刷新回收站列表，让顶栏红点立即反映新状态
        api.listTrash().then((items) => { trash.value = items }).catch(() => {})
        notify(t('toast.moved'))
      } catch (e) {
        notify(t('toast.deleteFail', { e: String(e) }), true)
      }
    },
  })
}

function restore(item: TrashItem) {
  ask({
    title: t('dialog.restore.title'),
    message: t('dialog.restore.body', { title: item.title }),
    okText: t('dialog.restore.ok'),
    onOk: async () => {
      // 在该行被移除前抓取起点与落点，触发飞回侧边栏项目列表的弧线动画
      const srcRect = restoreSourceRect(item)
      const target = restoreTarget(item)
      try {
        await api.restoreSession(item.trashFile)
        fly({ from: srcRect, to: target, variant: 'restore' })
        trash.value = trash.value.filter((x) => x.trashFile !== item.trashFile)
        // 若正在回收站详情里查看的就是这条，恢复后退回回收站列表。
        if (openTrashItem.value?.trashFile === item.trashFile) {
          openSession.value = null
        }
        await loadProjects()
        notify(t('toast.restored'))
      } catch (e) {
        notify(t('toast.restoreFail', { e: String(e) }), true)
      }
    },
  })
}

function permanentDelete(item: TrashItem) {
  ask({
    title: t('dialog.perm.title'),
    message: t('dialog.perm.body', { title: item.title }),
    okText: t('dialog.perm.ok'),
    danger: true,
    onOk: async () => {
      try {
        await api.permanentDeleteTrash(item.trashFile)
        trash.value = trash.value.filter((x) => x.trashFile !== item.trashFile)
        notify(t('toast.permDeleted'))
      } catch (e) {
        notify(t('toast.deleteFail', { e: String(e) }), true)
      }
    },
  })
}

// 批量恢复：恢复 trashToolbar 里勾选的会话。失败项跳过，只从 trash 移除成功项。
function batchRestore() {
  const keys = new Set(selectedTrash.value)
  const items = trash.value.filter((x) => keys.has(x.trashFile))
  if (!items.length) return
  ask({
    title: t('dialog.batchRestore.title'),
    message: t('dialog.batchRestore.body', { n: items.length }),
    okText: t('dialog.batchRestore.ok'),
    onOk: async () => {
      const restored = new Set<string>()
      for (const it of items) {
        try {
          await api.restoreSession(it.trashFile)
          restored.add(it.trashFile)
        } catch {
          /* 跳过失败项，继续恢复其余 */
        }
      }
      trash.value = trash.value.filter((x) => !restored.has(x.trashFile))
      exitSelectMode()
      await loadProjects()
      notify(t('toast.batchRestored', { n: restored.size }))
    },
  })
}

// 批量删除：把会话列表里勾选的会话一并 soft-delete 进回收站。失败项跳过，
// 不重置滚动；单条删除的弧线动画在此处一并跳过（一次性 N 个抛物线太喧闹）。
function batchDeleteSessions() {
  const keys = new Set(selectedSessions.value)
  const items = sessions.value.filter((s) => keys.has(s.path))
  if (!items.length) return
  ask({
    title: t('dialog.batchDelete.title'),
    message: t('dialog.batchDelete.body', { n: items.length }),
    okText: t('dialog.batchDelete.ok'),
    danger: true,
    onOk: async () => {
      const dir = activeProject.value?.displayPath ?? ''
      const deleted = new Set<string>()
      for (const s of items) {
        try {
          await api.softDeleteSession(agent.value, s.path, dir)
          deleted.add(s.path)
        } catch {
          /* 跳过失败项，继续删除其余 */
        }
      }
      sessions.value = sessions.value.filter((x) => !deleted.has(x.path))
      sessionTotal.value = Math.max(0, sessionTotal.value - deleted.size)
      if (openSession.value && deleted.has(openSession.value.path)) {
        openSession.value = null
      }
      exitSessionSelectMode()
      await loadProjects()
      api.listTrash().then((items) => { trash.value = items }).catch(() => {})
      notify(t('toast.batchDeleted', { n: deleted.size }))
    },
  })
}

// 批量导出：让用户挑一个目标目录，把勾选的会话一次性写成 MD / HTML / JSON 文件。
// 失败项跳过，结尾给一个汇总 toast。逐个 readSession 是简单可控的做法
// （会话数量本就不会很大），可以接受。
const exportToDirFn: Record<ExportKind, typeof exportMarkdownToDir> = {
  md: exportMarkdownToDir,
  html: exportHtmlToDir,
  json: exportJsonToDir,
}

async function batchExportSessions(kind: ExportKind) {
  const keys = new Set(selectedSessions.value)
  const items = sessions.value.filter((s) => keys.has(s.path))
  if (!items.length) return
  let parent: string | null = null
  try {
    parent = await pickExportDir()
  } catch (e) {
    notify(t('toast.batchExportFail', { e: String(e) }), true)
    return
  }
  if (!parent) return
  // 在用户选的目录里按约定再开一个子目录：`export-YYYYMMDD-HHMMSS-<kind>/`。
  // 这样多次批量导出不会互相覆盖，文件夹名一眼就能看出是什么时候、哪种格式的导出。
  // write_file 会自动 create_dir_all 父目录，不需要单独再发一次"建目录"命令。
  const dir = `${parent}/${batchExportFolderName(kind)}`
  let ok = 0
  let lastPath = ''
  for (const s of items) {
    try {
      const msgs = await api.readSession(agent.value, s.path)
      lastPath = await exportToDirFn[kind](s, msgs, agent.value, dir)
      recordExport({ path: s.path, title: s.title, agent: agent.value, sessionId: s.id, cwd: s.cwd, exportedAt: Date.now() })
      ok++
    } catch {
      /* 跳过失败项，继续导出其余 */
    }
  }
  exitSessionSelectMode()
  if (ok > 0) {
    notify(t('toast.batchExported', { n: ok, dir }))
    if (lastPath) api.revealInFinder(lastPath).catch(() => {})
  } else {
    notify(t('toast.batchExportFail', { e: t('toast.batchExportNone') }), true)
  }
}

function clearTrash() {
  if (!trash.value.length) return
  ask({
    title: t('dialog.empty.title'),
    message: t('dialog.empty.body', { n: trash.value.length }),
    okText: t('dialog.empty.ok'),
    danger: true,
    onOk: async () => {
      try {
        await api.emptyTrash()
        trash.value = []
        exitSelectMode()
        notify(t('toast.trashEmptied'))
      } catch (e) {
        notify(t('toast.emptyFail', { e: String(e) }), true)
      }
    },
  })
}

async function reveal(path: string) {
  try {
    await api.revealInFinder(path)
  } catch (e) {
    notify(`${e}`, true)
  }
}

const exportFn: Record<ExportKind, typeof exportMarkdown> = {
  md: exportMarkdown,
  html: exportHtml,
  json: exportJson,
}

async function exportSession(kind: ExportKind) {
  if (!openSession.value) return
  try {
    const path = await exportFn[kind](openSession.value, chatMsgs.value, agent.value)
    // 用户在 Save As 对话框点了取消时返回 null —— 静默放弃
    if (!path) return
    recordExport({
      path: openSession.value.path,
      title: openSession.value.title,
      agent: chatAgent.value,
      sessionId: openSession.value.id,
      cwd: openSession.value.cwd,
      exportedAt: Date.now(),
    })
    notify(t('toast.exported', { path }))
    api.revealInFinder(path).catch(() => {})
  } catch (e) {
    notify(t('toast.exportFail', { e: String(e) }), true)
  }
}

// 列表里直接导出某个会话：不打开会话，临时把消息读出来即可。
async function exportFromList(s: SessionMeta, kind: ExportKind) {
  try {
    const msgs = await api.readSession(agent.value, s.path)
    const path = await exportFn[kind](s, msgs, agent.value)
    if (!path) return
    recordExport({ path: s.path, title: s.title, agent: agent.value, sessionId: s.id, cwd: s.cwd, exportedAt: Date.now() })
    notify(t('toast.exported', { path }))
    api.revealInFinder(path).catch(() => {})
  } catch (e) {
    notify(t('toast.exportFail', { e: String(e) }), true)
  }
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    notify(t('toast.copied'))
  } catch (e) {
    notify(t('toast.copyFail', { e: String(e) }), true)
  }
}

async function resume(s: SessionMeta) {
  try {
    // Gemini 必须在对应的项目根目录下 resume 才能找对索引/ID。
    // 优先用 s.cwd（从 .project_root 反查出的绝对路径）。
    const cwd = s.cwd || activeProject.value?.displayPath || ''
    await api.resumeSession(agent.value, s.id, cwd, s.path, terminalApp.value)
    notify(t('toast.resumed'))
  } catch (e) {
    notify(`${e}`, true)
  }
}

// 在终端里为当前项目开一个全新会话（不带 --resume）。
async function newSession() {
  if (!activeProject.value) return
  try {
    await api.newSession(agent.value, activeProject.value.displayPath, terminalApp.value)
    notify(t('toast.newSession'))
  } catch (e) {
    notify(`${e}`, true)
  }
}

// 顶栏右上角的仓库入口
const REPO_URL = 'https://github.com/fengchenzxc/ai-sessions-viewer'
function openRepo() {
  api.openUrl(REPO_URL).catch((e) => notify(`${e}`, true))
}

function onClearCache() {
  ask({
    title: t('dialog.clearCache.title'),
    message: t('dialog.clearCache.body'),
    okText: t('dialog.clearCache.ok'),
    danger: true,
    onOk: () => {
      clearAppCache()
      projPrefs.value = {}
      notify(t('toast.cacheCleared'))
    },
  })
}

// ---------- 窗口聚焦 / 失焦：与 Codex 一致的弱化态 ----------
const windowFocused = ref(document.hasFocus())
function onFocus() {
  windowFocused.value = true
}
function onBlur() {
  windowFocused.value = false
}

onMounted(() => {
  loadProjects()
  // 启动时拉一次回收站，让顶栏红点从一开始就准确（不必先打开回收站视图）
  api.listTrash().then((items) => { trash.value = items }).catch(() => {})
  // 后台检查 GitHub release —— 缓存 24h，失败完全静默；结果驱动侧边栏 Settings
  // 按钮上的"有新版本"小红点。
  runBackgroundCheck()
  window.addEventListener('focus', onFocus)
  window.addEventListener('blur', onBlur)
  // 右键菜单的全局关闭：任意点击 / 滚轮 / ESC
  document.addEventListener('mousedown', (e) => {
    if (!ctxMenu.value) return
    const target = e.target as HTMLElement | null
    if (target && target.closest('.ctx-menu')) return
    closeCtxMenu()
  })
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && ctxMenu.value) closeCtxMenu()
  })
  window.addEventListener('blur', closeCtxMenu)
  document.addEventListener('wheel', closeCtxMenu, { passive: true })

  // 全局搜索：⌘⇧F (macOS) / Ctrl⇧F (Win/Linux)；与文本输入框里的 ⌘F 互不冲突。
  // 在 capture 阶段拦截，确保不会被任何子组件 stopPropagation 掉。
  // 注：macOS 上若菜单 accelerator 抢先触发会走 menu://action，这条监听吃不到事件；
  // 二者结果相同（都开浮层），保留这条是给菜单未注册成功时兜底。
  window.addEventListener(
    'keydown',
    (e) => {
      if (e.key !== 'f' && e.key !== 'F') return
      if (!e.shiftKey) return
      const isMac = /Mac/i.test(navigator.platform)
      const want = isMac ? e.metaKey : e.ctrlKey
      const other = isMac ? e.ctrlKey : e.metaKey
      if (!want || other || e.altKey) return
      e.preventDefault()
      openGlobalSearch()
    },
    true,
  )

  // 原生菜单 → 前端动作路由。菜单项的 id 在 src-tauri/src/menu.rs 里定义。
  installMenuRouter({
    'open-global-search': () => openGlobalSearch(),
    'find-in-session': () => focusSearchBox(),
    'find-next': () => chatNavigate(1),
    'find-prev': () => chatNavigate(-1),
    'toggle-sidebar': toggleSidebar,
    'new-session': () => newSession(),
    'open-settings': () => {
      showSettings.value = true
    },
    'export-session': () => {
      if (!openSession.value) {
        notify(t('toast.exportNoSession'))
        return
      }
      // 没法在原生菜单里二选一 —— 默认走 Markdown；HTML 仍可在卡片导出菜单里选。
      exportSession('md')
    },
    'open-trash': () => loadTrash(),
    'open-stats': openStats,
    'check-update': () => {
      showSettings.value = true
    },
    'theme:light': () => setTheme('light'),
    'theme:dark': () => setTheme('dark'),
    'theme:system': () => setTheme('system'),
    'lang:en': () => setLang('en'),
    'lang:zh': () => setLang('zh'),
    'lang:zh-TW': () => setLang('zh-TW'),
    'lang:ja': () => setLang('ja'),
    'help-docs': () => api.openUrl(`${REPO_URL}#readme`).catch((e) => notify(`${e}`, true)),
    'help-repo': () => openRepo(),
    'help-issue': () => api.openUrl(`${REPO_URL}/issues`).catch((e) => notify(`${e}`, true)),
  }).then((fn) => {
    menuUnlisten = fn
  })

  // 启动时把当前 theme / lang 同步给菜单的 CheckMenuItem 勾选态。
  emitMenuSync('theme', theme.value)
  emitMenuSync('lang', lang.value)
})

// 主题 / 语言变化 → 同步菜单勾选态。
watch(theme, (v) => emitMenuSync('theme', v))
watch(lang, (v) => emitMenuSync('lang', v))

watch([codexShowInternalSessions, codexShowArchivedSessions], () => {
  if (agent.value !== 'codex') return
  loadProjects()
  if (activeDir.value && !showTrash.value && !showStats.value) {
    refreshSessions()
  }
})

let menuUnlisten: UnlistenFn | null = null

// Live tail：监听 watch.rs emit 的 3 个事件。安装一次，整个应用生命周期共用。
//   session:append → 后端把新增的尾段 Msg 推过来；前端 push 进 chatMsgs，
//                    再调 ChatView.onLiveAppend(n) 让它做 smart-scroll。
//   session:reset  → 文件被截断 / 替换 → 整段重拉。
//   session:gone   → 文件不在了 → 关闭当前会话，toast 一下。
// path 兜底校验：用户在 emit 飞过来的极短窗口里切换了会话 / 关掉了详情页，
// 我们只接当前 openSession.path 一致的事件，避免把 A 会话的尾段塞到 B 里。
let liveUnlisteners: UnlistenFn[] = []

async function installLiveTailListeners() {
  const appendUnlisten = await listen<{ path: string; messages: Msg[] }>(
    'session:append',
    (e) => {
      const cur = openSession.value
      if (!cur || cur.path !== e.payload.path) return
      const added = e.payload.messages
      if (!added.length) return
      chatMsgs.value = chatMsgs.value.concat(added)
      // 真的有新增 → 标"Live"，并续命 fade 定时器。
      markLive()
      // 等 v-for 把新行挂上 DOM，再交给 ChatView 决定是否自动滚到底。
      nextTick(() => chatViewRef.value?.onLiveAppend?.(added.length))
    },
  )
  const resetUnlisten = await listen<{ path: string }>('session:reset', async (e) => {
    const cur = openSession.value
    if (!cur || cur.path !== e.payload.path) return
    // 整段重读 —— 不动 openSession 自身，避免 watch 重置 chat-toolbar 状态。
    try {
      chatMsgs.value = await api.readSession(chatAgent.value, cur.path)
    } catch {
      // 读不出来通常是文件刚被换掉；下一次 emit 会再来一次。
    }
  })
  const goneUnlisten = await listen<{ path: string }>('session:gone', (e) => {
    const cur = openSession.value
    if (!cur || cur.path !== e.payload.path) return
    notify(t('toast.sessionGone'))
    openSession.value = null
  })
  liveUnlisteners.push(appendUnlisten, resetUnlisten, goneUnlisten)
}

onMounted(() => {
  installLiveTailListeners()
})

onUnmounted(() => {
  menuUnlisten?.()
  menuUnlisten = null
  liveUnlisteners.forEach((u) => u())
  liveUnlisteners = []
  clearLive()
  api.unwatchSession().catch(() => {})
})

// 全局搜索命中：跳到对应项目并打开会话；正文命中再滚到目标消息并触发闪烁动画。
// 如果命中所在项目不在已加载列表里（极少见 —— list_projects 通常涵盖全部），
// 先刷一次项目列表再跳。
async function onGlobalSearchOpen(hit: SearchHit) {
  if (activeDir.value !== hit.projectKey) {
    if (!projects.value.some((p) => p.dirName === hit.projectKey)) {
      await loadProjects()
    }
    await selectProject(hit.projectKey)
  }
  await openChat(hit.session)
  // 文本命中带上消息坐标 —— 等 ChatView 挂载并把 messages 渲染出来后再跳。
  // 这里走 2 次 nextTick：一次让 ChatView 拿到 ref，一次让长列表渲染稳定后再 query 节点。
  if (hit.matchedField === 'text' && typeof hit.matchMsgIndex === 'number') {
    await nextTick()
    await nextTick()
    chatViewRef.value?.flashMessage(hit.matchMsgIndex, hit.matchMsgUuid ?? undefined)
  }
}
</script>

<template>
  <div
    class="app"
    :class="[
      `agent-${agent}`,
      sidebarOpen ? 'sidebar-open' : 'sidebar-closed',
      { 'is-blurred': !windowFocused },
    ]"
  >
    <!-- 顶栏：normal flow，整条都是 macOS 拖动区。
         data-tauri-drag-region="deep" 让整个子树（除按钮等可点击元素外）
         都触发原生 startDragging；button/A/INPUT 等会自动 block 拖动，
         不需要手动 no-drag。同时保留 -webkit-app-region: drag 做 OS 层兜底。 -->
    <div class="app-topbar" data-tauri-drag-region="deep">
      <SidebarTopbar
        :refreshing="refreshing"
        :show-trash="showTrash"
        :show-stats="showStats"
        :show-history="showExportHistory"
        :show-pricing="showPricing"
        :has-trash="trash.length > 0"
        @toggle-sidebar="toggleSidebar"
        @refresh="refreshAll"
        @open-trash="loadTrash"
        @open-stats="openStats"
        @open-history="openExportHistory"
        @open-pricing="openPricing"
      />
      <!-- 顶栏右侧分发：每个页面把自己的工具栏组件挂这里。
           本身仍是 macOS 拖动区域，组件内部的可交互元素由 CSS 单独标 no-drag。 -->
      <div class="topbar-drag">
        <!-- StatsView 自带顶部控制条，这里就让出空间（保持拖动区域）。
             showStats 优先级要高于 openSession，否则进入会话统计模式时
             还会渲染 ChatTopbar 的「会话统计」按钮，造成视觉重复。 -->
        <div v-if="showStats" />
        <ChatTopbar v-else-if="openSession" @open-session-stats="openSessionStats" />
        <TrashTopbar
          v-else-if="showTrash"
          :items="trash"
          @batch-restore="batchRestore"
        />
        <SessionsTopbar
          v-else-if="activeProject"
          :sessions="sessions"
          @batch-delete="batchDeleteSessions"
          @batch-export="batchExportSessions"
        />
        <div v-else />
      </div>
    </div>

    <div class="app-body">
    <!-- 侧栏 -->
    <Sidebar
      v-show="sidebarOpen"
      :agent="agent"
      :projects="projects"
      :active-dir="activeDir"
      :show-trash="showTrash"
      :proj-prefs="projPrefs"
      @switch-agent="switchAgent"
      @select-project="selectProject"
      @context-menu="openCtxMenu"
      @open-settings="showSettings = true"
    />

    <!-- 主区 -->
    <main class="main">
      <!-- 统计页面优先级最高：全局统计 *或* 单会话统计都走这一块。
           单会话模式时 openSession 仍保留，关闭统计页就能回到原聊天上下文。
           注意：不传 agent —— StatsView 的 scope 是组件内部独立状态（默认 all），
           不受侧栏当前 agent 影响。 -->
      <StatsView
        v-if="showStats"
        :session="sessionStatsTarget"
        @close="closeStats"
        @open-project="(dir) => selectProject(dir)"
        @open-session="openSessionStatsFromGlobal"
      />

      <template v-else-if="openSession">
        <div v-if="loadingChat" class="loading">{{ t('common.loading') }}</div>
        <ChatView
          v-else
          ref="chatViewRef"
          :agent="chatAgent"
          :session="openSession"
          :messages="chatMsgs"
          :trashed="!!openTrashItem"
          :live="liveTailing"
          @back="openSession = null"
          @refresh="openChat(openSession)"
          @delete="deleteSession(openSession)"
          @resume="resume(openSession)"
          @rename="openRename(openSession)"
          @reveal="reveal(openSession.path)"
          @copy-id="copyText(openSession.id)"
          @export-md="exportSession('md')"
          @export-html="exportSession('html')"
          @export-json="exportSession('json')"
          @restore="openTrashItem && restore(openTrashItem)"
        />
      </template>

      <!-- 回收站视图 -->
      <TrashView
        v-else-if="showTrash"
        :trash="trash"
        :loading="loadingList"
        @clear="clearTrash"
        @open="openTrashSession"
        @restore="restore"
        @permanent-delete="permanentDelete"
      />

      <ExportHistoryView
        v-else-if="showExportHistory"
        @open="openHistorySession"
      />

      <PricingView v-else-if="showPricing" />

      <!-- 会话列表视图 -->
      <SessionsView
        v-else-if="activeProject"
        ref="sessionsViewRef"
        :agent="agent"
        :project="activeProject"
        :sessions="sessions"
        :session-total="sessionTotal"
        :loading="loadingList"
        :loading-more="loadingMore"
        @open="openChat"
        @rename="openRename"
        @resume="resume"
        @reveal="reveal"
        @delete="deleteSession"
        @copy="copyText"
        @export="exportFromList"
        @refresh="refreshSessions"
        @new-session="newSession"
        @delete-project="deleteActiveProject"
        @load-more="loadMore"
        @scroll="onListScroll"
      />

      <WelcomeView
        v-else
        :agent="agent"
        :projects="projects"
        @select-project="selectProject"
        @switch-agent="switchAgent"
        @open-repo="openRepo"
      />
    </main>
    </div>

    <!-- 确认弹窗 -->
    <ConfirmModal
      :show="confirm.show"
      :title="confirm.title"
      :message="confirm.message"
      :ok-text="confirm.okText"
      :danger="confirm.danger"
      @confirm="runConfirm"
      @cancel="confirm.show = false"
    />

    <!-- 设置弹窗 -->
    <Transition name="fade">
      <SettingsModal
        v-if="showSettings"
        :cache-bytes="cacheBytes"
        @close="showSettings = false"
        @clear-cache="onClearCache"
      />
    </Transition>

    <!-- 重命名会话 -->
    <RenameModal
      v-model="renameModal.value"
      :show="renameModal.show"
      :default-title="renameModal.defaultTitle"
      @confirm="confirmRename"
      @cancel="renameModal.show = false"
    />

    <!-- 全局搜索（⌘⇧F / Ctrl⇧F） -->
    <GlobalSearchModal
      :show="globalSearchOpen"
      :agent="agent"
      @update:show="globalSearchOpen = $event"
      @open="onGlobalSearchOpen"
    />

    <!-- 项目右键菜单 -->
    <ProjectContextMenu
      v-if="ctxMenu"
      :x="ctxMenu.x"
      :y="ctxMenu.y"
      :project="ctxMenu.project"
      :proj-state="projStateOf(ctxMenu.project)"
      @toggle-state="ctxToggleState"
      @refresh="ctxRefresh"
      @delete="ctxDeleteProject"
    />

    <!-- toast -->
    <Transition name="fade">
      <div v-if="toast.show" class="toast" :class="{ error: toast.error }">
        {{ toast.msg }}
      </div>
    </Transition>
  </div>
</template>
