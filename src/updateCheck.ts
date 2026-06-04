// 后台版本检查 —— 启动时跑一次，结果缓存到 localStorage 24h；侧边栏 Settings
// 入口据此显示一个小红点提示有新版本。失败完全静默 —— 这是后台检查，不该
// 拿网络问题打扰用户；点 Settings 里「检查更新」会拿到真实的报错信息。
//
// 与 src/api.ts 的 checkUpdate 的关系：api 层负责调 GitHub、解析响应；本模块
// 负责"什么时候检查、结果存哪、UI 据此渲染什么"。SettingsModal 手动检查
// 完成后也会回调 syncFromManualCheck，让红点与手动结果保持一致。

import { ref } from 'vue'
import { appVersion, checkUpdate, openUrl, type UpdateInfo } from './api'

// 没拿到具体 release 的 html_url 时的兜底地址。和 App.vue 的 REPO_URL 同源；
// /releases/latest 永远会重定向到当前最新 release 页面，等价于"先点 Latest"。
const RELEASES_LATEST_PAGE =
  'https://github.com/fengchenzxc/ai-sessions-viewer/releases/latest'

const CACHE_KEY = 'updateCheck:v1'
const TTL_MS = 24 * 60 * 60 * 1000 // 一天 —— GitHub 未授权 API 是 60 次/小时/IP，足够安全

interface Cached {
  checkedAt: number
  latest: string
  htmlUrl?: string
}

/** 有新版本时为 true；驱动侧边栏 Settings 按钮的小红点。 */
export const updateAvailable = ref(false)
/** 远端最新版本号（不带 v 前缀），用于 tooltip / Settings 里展示。 */
export const latestVersion = ref<string | null>(null)
/** 对应 GitHub release 页 URL，后续可以做"点击直达"。 */
export const releaseUrl = ref<string | null>(null)

function loadCache(): Cached | null {
  try {
    const raw = localStorage.getItem(CACHE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as Cached
    if (typeof parsed?.checkedAt !== 'number' || typeof parsed?.latest !== 'string') {
      return null
    }
    return parsed
  } catch {
    return null
  }
}

function saveCache(c: Cached) {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(c))
  } catch {
    /* 配额耗尽等场景静默忽略，不阻塞后续逻辑 */
  }
}

// 复制一份小 semver 比较 —— 与 api.ts 内部那份逻辑一致，不跨模块借用以保持
// api.ts 作为纯 invoke 包装层的边界。
function compareVer(a: string, b: string): number {
  const pa = a.replace(/^v/i, '').split(/[.-]/).map((x) => parseInt(x, 10) || 0)
  const pb = b.replace(/^v/i, '').split(/[.-]/).map((x) => parseInt(x, 10) || 0)
  const n = Math.max(pa.length, pb.length)
  for (let i = 0; i < n; i++) {
    const da = pa[i] ?? 0
    const db = pb[i] ?? 0
    if (da !== db) return da - db
  }
  return 0
}

function applyInfo(info: UpdateInfo) {
  updateAvailable.value = info.hasUpdate
  latestVersion.value = info.latest
  releaseUrl.value = info.htmlUrl ?? null
}

/**
 * 应用启动时调用一次。
 *   1. 先用 localStorage 缓存即时把红点/版本号点亮（同步显示）；
 *      跟 fresh appVersion 比对，万一用户已经升级过了缓存还说"有更新"，立刻清掉。
 *   2. 如果缓存超过 24h（或没缓存）再去发一次真实请求。失败完全静默。
 */
export async function runBackgroundCheck(): Promise<void> {
  const cached = loadCache()
  const current = await appVersion().catch(() => null)

  // 优先用缓存即刻刷新 UI；hasUpdate 用现在的 current 和缓存里的 latest 现算 ——
  // 这样用户升级后第一次启动就能正确清掉红点，不需要等下一次 24h 后的新请求。
  if (cached && current) {
    updateAvailable.value = compareVer(cached.latest, current) > 0
    latestVersion.value = cached.latest
    releaseUrl.value = cached.htmlUrl ?? null
  }

  const fresh = cached && Date.now() - cached.checkedAt < TTL_MS
  if (fresh) return

  try {
    const info = await checkUpdate()
    applyInfo(info)
    saveCache({ checkedAt: Date.now(), latest: info.latest, htmlUrl: info.htmlUrl })
  } catch {
    /* 后台检查的网络/HTTP 错误静默吞掉 —— 手动检查会把真实错误展示给用户 */
  }
}

/**
 * SettingsModal 手动「检查更新」完成后调一下，把红点状态与最新一次手动检查
 * 对齐（顺便刷新 TTL —— 用户刚手动看过，没必要 24h 内再背着他打一次）。
 */
export function syncFromManualCheck(info: UpdateInfo): void {
  applyInfo(info)
  saveCache({ checkedAt: Date.now(), latest: info.latest, htmlUrl: info.htmlUrl })
}

/**
 * 在系统浏览器中打开当前已知最新版本的 release 页。优先用 GitHub API 返回
 * 的 html_url（精确到那一条 release）；拿不到就退到 /releases/latest（GitHub
 * 会自动 302 到最新一条）。出错只在 console 留个痕，不抛 —— 调用方一般是
 * 装饰性按钮，失败也不该阻塞主流程。
 */
export async function openReleasePage(): Promise<void> {
  const url = releaseUrl.value ?? RELEASES_LATEST_PAGE
  try {
    await openUrl(url)
  } catch (e) {
    console.warn('[updateCheck] openUrl failed', e)
  }
}
