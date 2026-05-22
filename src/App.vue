<script setup lang="ts">
import { ref, computed, onMounted, watch, nextTick } from 'vue'
import type { Agent, ProjectInfo, SessionMeta, TrashItem, Msg } from './types'
import * as api from './api'
import { shortName } from './format'
import { t } from './i18n'
import { clearAppCache } from './settings'
import { resetChatToolbar } from './chatToolbar'
import { resetTrashToolbar, exitSelectMode, selectedTrash } from './trashToolbar'
import { resetSessionsToolbar, sessionsFilterActive } from './sessionsToolbar'
import { exportMarkdown, exportHtml } from './export'
import { fly } from './fly'
import { recordRecent } from './recents'
import ChatView from './views/ChatView.vue'
import SettingsModal from './components/SettingsModal.vue'
import ChatTopbar from './components/topbar/ChatTopbar.vue'
import TrashTopbar from './components/topbar/TrashTopbar.vue'
import SessionsTopbar from './components/topbar/SessionsTopbar.vue'
import TrashView from './views/TrashView.vue'
import SessionsView from './views/SessionsView.vue'
import WelcomeView from './views/WelcomeView.vue'
import Sidebar from './components/Sidebar.vue'
import SidebarTopbar from './components/SidebarTopbar.vue'
import ConfirmModal from './modals/ConfirmModal.vue'
import RenameModal from './modals/RenameModal.vue'
import ProjectContextMenu from './modals/ProjectContextMenu.vue'

// ---------- 状态 ----------
const agent = ref<Agent>('claude')
const projects = ref<ProjectInfo[]>([])
const activeDir = ref<string | null>(null)
const showTrash = ref(false)
const showSettings = ref(false)
const sidebarOpen = ref(true)
const refreshing = ref(false)
function toggleSidebar() {
  sidebarOpen.value = !sidebarOpen.value
}

/** 顶栏刷新：重新拉取项目 + 当前列表 + 当前打开的对话，全部静默，不动选中与滚动。 */
async function refreshAll() {
  if (refreshing.value) return
  refreshing.value = true
  const tasks: Promise<unknown>[] = []

  // 1. 项目列表（保留 activeDir）
  tasks.push(
    api.listProjects(agent.value).then((p) => {
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
        .listSessions(agent.value, activeDir.value, 0, n)
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

const sessionsViewRef = ref<InstanceType<typeof SessionsView> | null>(null)
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
          const page = await api.listSessions(agent.value, p.dirName, offset, 200)
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
    projects.value = await api.listProjects(agent.value)
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
  loadProjects()
}

async function selectProject(dir: string) {
  // 再次点击当前已选中的项目 → 收起，回到「请选择项目」空状态
  if (activeDir.value === dir && !showTrash.value) {
    activeDir.value = null
    openSession.value = null
    sessions.value = []
    sessionTotal.value = 0
    resetSessionsToolbar()
    return
  }
  showTrash.value = false
  activeDir.value = dir
  recordRecent(agent.value, dir)
  openSession.value = null
  sessions.value = []
  sessionTotal.value = 0
  savedListScroll = 0
  resetSessionsToolbar()
  loadingList.value = true
  try {
    const page = await api.listSessions(agent.value, dir, 0, PAGE_SIZE)
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
    )
    sessions.value = page.sessions
    sessionTotal.value = page.total
  } catch (e) {
    notify(t('toast.loadSessionsFail', { e: String(e) }), true)
  } finally {
    loadingList.value = false
  }
}

async function loadTrash() {
  showTrash.value = true
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
  try {
    chatMsgs.value = await api.readSession(agent.value, s.path)
  } catch (e) {
    notify(t('toast.readFail', { e: String(e) }), true)
    openSession.value = null
  } finally {
    loadingChat.value = false
  }
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

async function exportSession(kind: 'md' | 'html') {
  if (!openSession.value) return
  try {
    const fn = kind === 'md' ? exportMarkdown : exportHtml
    const path = await fn(openSession.value, chatMsgs.value, agent.value)
    // 用户在 Save As 对话框点了取消时返回 null —— 静默放弃
    if (!path) return
    notify(t('toast.exported', { path }))
    api.revealInFinder(path).catch(() => {})
  } catch (e) {
    notify(t('toast.exportFail', { e: String(e) }), true)
  }
}

// 列表里直接导出某个会话：不打开会话，临时把消息读出来即可。
async function exportFromList(s: SessionMeta, kind: 'md' | 'html') {
  try {
    const msgs = await api.readSession(agent.value, s.path)
    const fn = kind === 'md' ? exportMarkdown : exportHtml
    const path = await fn(s, msgs, agent.value)
    if (!path) return
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
    await api.resumeSession(
      agent.value,
      s.id,
      s.cwd ?? activeProject.value?.displayPath ?? '',
    )
    notify(t('toast.resumed'))
  } catch (e) {
    notify(`${e}`, true)
  }
}

// 在终端里为当前项目开一个全新会话（不带 --resume）。
async function newSession() {
  if (!activeProject.value) return
  try {
    await api.newSession(agent.value, activeProject.value.displayPath)
    notify(t('toast.newSession'))
  } catch (e) {
    notify(`${e}`, true)
  }
}

// 顶栏右上角的仓库入口
const REPO_URL = 'https://github.com/jerrywu001/cc-sessions-viewer'
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
})
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
        :has-trash="trash.length > 0"
        @toggle-sidebar="toggleSidebar"
        @refresh="refreshAll"
        @open-trash="loadTrash"
      />
      <!-- 顶栏右侧分发：每个页面把自己的工具栏组件挂这里。
           本身仍是 macOS 拖动区域，组件内部的可交互元素由 CSS 单独标 no-drag。 -->
      <div class="topbar-drag">
        <ChatTopbar v-if="openSession" />
        <TrashTopbar
          v-else-if="showTrash"
          :items="trash"
          @batch-restore="batchRestore"
        />
        <SessionsTopbar v-else-if="activeProject" />
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
      <template v-if="openSession">
        <div v-if="loadingChat" class="loading">{{ t('common.loading') }}</div>
        <ChatView
          v-else
          :agent="chatAgent"
          :session="openSession"
          :messages="chatMsgs"
          :trashed="!!openTrashItem"
          @back="openSession = null"
          @refresh="openChat(openSession)"
          @delete="deleteSession(openSession)"
          @resume="resume(openSession)"
          @rename="openRename(openSession)"
          @reveal="reveal(openSession.path)"
          @copy-id="copyText(openSession.id)"
          @export-md="exportSession('md')"
          @export-html="exportSession('html')"
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

      <!-- 会话列表视图 -->
      <SessionsView
        v-else-if="activeProject"
        ref="sessionsViewRef"
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
