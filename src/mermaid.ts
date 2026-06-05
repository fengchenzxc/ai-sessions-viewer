// Mermaid 渲染辅助 —— 给 ChatView 在 v-html 注入完之后扫一遍 DOM 替换占位符。
//
// 渲染管线：
//   1. `renderText` 看到 ```mermaid``` 围栏，发 `<div class="md-mermaid" data-source="...">`
//      占位符，里面塞一份 escaped 源码（fallback / 渲染失败时露出来 + 主题切换时复用）。
//   2. ChatView 在 onMounted / messages 变化的 nextTick 调 `renderAllMermaid(root)`。
//   3. 这里 dynamic-import mermaid，给每个 .md-mermaid 调 mermaid.render() 替换 innerHTML。
//
// 为什么 dynamic-import：mermaid 压缩后约 600KB，没用到 mermaid 的会话不该把它拖进
// 主 bundle。第一次出现 mermaid 块时才 fetch / 解析。
//
// 主题：跟 settings.ts 里 `theme` 联动。light → 'default'，dark → 'dark'。
// 切换主题时需要重渲染所有 .md-mermaid（mermaid 不支持运行时改主题，要拿 source 重画）。

import { theme } from './settings'

let mermaidPromise: Promise<typeof import('mermaid').default> | null = null
let currentTheme: 'light' | 'dark' | null = null
let renderSeq = 0

function effectiveTheme(): 'light' | 'dark' {
  if (theme.value === 'dark') return 'dark'
  if (theme.value === 'light') return 'light'
  // system → 看 prefers-color-scheme
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

async function loadMermaid() {
  if (!mermaidPromise) {
    mermaidPromise = import('mermaid').then((m) => m.default)
  }
  const mermaid = await mermaidPromise
  const themeNow = effectiveTheme()
  if (currentTheme !== themeNow) {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: 'strict',
      theme: themeNow === 'dark' ? 'dark' : 'default',
      // 字体跟全局 UI 一致，避免 mermaid 默认衬线字体在我们的 sans-serif UI 里突兀。
      fontFamily:
        '-apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif',
    })
    currentTheme = themeNow
  }
  return mermaid
}

/** 渲染 root 下所有未渲染过的 .md-mermaid 节点。幂等 —— 已渲染过的（带 data-rendered）会跳过。 */
export async function renderAllMermaid(root: HTMLElement | null): Promise<void> {
  if (!root) return
  const nodes = root.querySelectorAll<HTMLElement>('.md-mermaid:not([data-rendered])')
  if (!nodes.length) return
  let mermaid: Awaited<ReturnType<typeof loadMermaid>>
  try {
    mermaid = await loadMermaid()
  } catch (e) {
    // mermaid 拉不到（离线 / 包损坏）——保留占位符里的源码，不阻断其它消息渲染。
    console.warn('[mermaid] failed to load:', e)
    return
  }
  for (const el of Array.from(nodes)) {
    const src = decodeURIComponent(el.dataset.source ?? '')
    if (!src) {
      el.setAttribute('data-rendered', '1')
      continue
    }
    renderSeq += 1
    const id = `md-mermaid-${renderSeq}`
    try {
      const { svg } = await mermaid.render(id, src)
      el.innerHTML = svg
      el.setAttribute('data-rendered', '1')
    } catch (e) {
      // 语法错误 / 渲染失败：把 .md-mermaid 改成 .md-mermaid-error，露出源码 + 一行错误。
      const msg = (e as Error)?.message ?? String(e)
      el.classList.add('md-mermaid-error')
      el.innerHTML =
        `<div class="md-mermaid-errmsg">mermaid: ${escapeHtml(msg)}</div>` +
        `<pre class="md-mermaid-source">${escapeHtml(src)}</pre>`
      el.setAttribute('data-rendered', '1')
    }
  }
}

/** 主题切换时把所有 .md-mermaid 标记成"未渲染"，下次 renderAllMermaid 会重画。
 *  调用方应在 nextTick 后调 renderAllMermaid。 */
export function resetMermaidForTheme(root: HTMLElement | null): void {
  currentTheme = null
  if (!root) return
  root.querySelectorAll<HTMLElement>('.md-mermaid[data-rendered]').forEach((el) => {
    const src = decodeURIComponent(el.dataset.source ?? '')
    el.removeAttribute('data-rendered')
    el.classList.remove('md-mermaid-error')
    el.innerHTML = `<pre class="md-mermaid-source">${escapeHtml(src)}</pre>`
  })
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}
