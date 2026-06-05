// 统一图标层：所有图标改用 Iconify（lucide 集合）按需打包，编译期内联 SVG，
// 运行时不联网（Tauri 离线友好）。如需替换图标，直接换 import 路径即可：
//   import IconFoo from '~icons/lucide/foo-name'
// 浏览所有可用图标：https://iconify.design/

import IconPinUpRaw from '~icons/lucide/arrow-up-to-line'
import IconPinDownRaw from '~icons/lucide/arrow-down-to-line'
import IconTrashRaw from '~icons/lucide/trash-2'
import IconTrashOpenRaw from '~icons/quill/folder-trash'
import IconRestoreRaw from '~icons/lucide/archive-restore'
import IconSettingsRaw from '~icons/lucide/settings'
import IconPlayRaw from '~icons/lucide/play'
import IconChatRaw from '~icons/lucide/message-circle'
import IconFolderRaw from '~icons/lucide/folder'
import IconInboxRaw from '~icons/lucide/inbox'
import IconRefreshRaw from '~icons/lucide/rotate-cw'
import IconArrowLeftRaw from '~icons/lucide/arrow-left'
import IconArrowUpRaw from '~icons/lucide/arrow-up'
import IconArrowDownRaw from '~icons/lucide/arrow-down'
import IconChevronRightRaw from '~icons/lucide/chevron-right'
import IconEmptyBoxRaw from '~icons/lucide/package'
import IconPointLeftRaw from '~icons/lucide/chevron-left'
import IconSidebarRaw from '~icons/lucide/panel-left'
import IconCloseRaw from '~icons/lucide/x'
import IconSunRaw from '~icons/lucide/sun'
import IconMoonRaw from '~icons/lucide/moon'
import IconMonitorRaw from '~icons/lucide/monitor'
import IconTerminalRaw from '~icons/lucide/square-terminal'
import IconLanguagesRaw from '~icons/lucide/languages'
import IconDatabaseRaw from '~icons/lucide/database'
import IconInfoRaw from '~icons/lucide/info'
import IconPaletteRaw from '~icons/lucide/palette'
import IconCheckRaw from '~icons/lucide/check'
import IconPencilRaw from '~icons/lucide/pencil'
import IconCopyRaw from '~icons/lucide/copy'
import IconSearchRaw from '~icons/lucide/search'
import IconChevronUpRaw from '~icons/lucide/chevron-up'
import IconChevronDownRaw from '~icons/lucide/chevron-down'
import IconFoldRaw from '~icons/lucide/chevrons-down-up'
import IconUnfoldRaw from '~icons/lucide/chevrons-up-down'
import IconDownloadRaw from '~icons/lucide/download'
import IconMarkdownRaw from '~icons/lucide/file-text'
import IconHtmlRaw from '~icons/lucide/file-code'
import IconJsonRaw from '~icons/lucide/braces'
import IconSortRaw from '~icons/lucide/arrow-down-up'
import IconSelectRaw from '~icons/lucide/list-checks'
import IconPlusRaw from '~icons/lucide/plus'
import IconHistoryRaw from '~icons/lucide/history'
import IconExportHistoryRaw from '~icons/lucide/clock-arrow-down'
import IconMoreRaw from '~icons/lucide/more-horizontal'
import IconPriceTagRaw from '~icons/lucide/circle-dollar-sign'
import IconGithubRaw from '~icons/lucide/github'
import IconCornerDownLeftRaw from '~icons/lucide/corner-down-left'
import IconChartRaw from '~icons/lucide/bar-chart-3'
import IconWalletRaw from '~icons/lucide/wallet'
import IconActivityRaw from '~icons/lucide/activity'
import IconLayersRaw from '~icons/lucide/layers'
import IconZapRaw from '~icons/lucide/zap'
import IconExternalLinkRaw from '~icons/lucide/external-link'
import IconArchiveRaw from '~icons/lucide/archive'
import IconShieldCheckRaw from '~icons/lucide/shield-check'
import IconClaudeRaw from '~icons/material-icon-theme/claude'
import IconCodexRaw from '~icons/arcticons/openai-chatgpt'
import IconGeminiRaw from '~icons/material-icon-theme/gemini-ai'

export const IconPinUp = IconPinUpRaw
export const IconPinDown = IconPinDownRaw
export const IconTrash = IconTrashRaw
export const IconTrashOpen = IconTrashOpenRaw
export const IconRestore = IconRestoreRaw
export const IconSettings = IconSettingsRaw
export const IconPlay = IconPlayRaw
export const IconChat = IconChatRaw
export const IconFolder = IconFolderRaw
export const IconInbox = IconInboxRaw
export const IconRefresh = IconRefreshRaw
export const IconArrowLeft = IconArrowLeftRaw
export const IconArrowUp = IconArrowUpRaw
export const IconArrowDown = IconArrowDownRaw
export const IconChevronRight = IconChevronRightRaw
export const IconEmptyBox = IconEmptyBoxRaw
export const IconPointLeft = IconPointLeftRaw
export const IconSidebar = IconSidebarRaw
export const IconClose = IconCloseRaw
export const IconSun = IconSunRaw
export const IconMoon = IconMoonRaw
export const IconMonitor = IconMonitorRaw
export const IconTerminal = IconTerminalRaw
export const IconLanguages = IconLanguagesRaw
export const IconDatabase = IconDatabaseRaw
export const IconInfo = IconInfoRaw
export const IconPalette = IconPaletteRaw
export const IconCheck = IconCheckRaw
export const IconPencil = IconPencilRaw
export const IconCopy = IconCopyRaw
export const IconSearch = IconSearchRaw
export const IconChevronUp = IconChevronUpRaw
export const IconChevronDown = IconChevronDownRaw
export const IconFold = IconFoldRaw
export const IconUnfold = IconUnfoldRaw
export const IconDownload = IconDownloadRaw
export const IconMarkdown = IconMarkdownRaw
export const IconHtml = IconHtmlRaw
export const IconJson = IconJsonRaw
export const IconSort = IconSortRaw
export const IconSelect = IconSelectRaw
export const IconPlus = IconPlusRaw
export const IconHistory = IconHistoryRaw
export const IconExportHistory = IconExportHistoryRaw
export const IconMore = IconMoreRaw
export const IconPriceTag = IconPriceTagRaw
export const IconGithub = IconGithubRaw
export const IconCornerDownLeft = IconCornerDownLeftRaw
export const IconChart = IconChartRaw
export const IconWallet = IconWalletRaw
export const IconActivity = IconActivityRaw
export const IconLayers = IconLayersRaw
export const IconZap = IconZapRaw
export const IconExternalLink = IconExternalLinkRaw
export const IconArchive = IconArchiveRaw
export const IconShieldCheck = IconShieldCheckRaw
// 「已 pin」状态的小圆点指示器；6×6 实心圆，自己拼比拉一整个集合便宜。
import { defineComponent, h, type Component } from 'vue'
import type { Agent } from '../types'
export const IconPinFilled = defineComponent({
  name: 'IconPinFilled',
  setup() {
    return () =>
      h(
        'svg',
        {
          viewBox: '0 0 24 24',
          fill: 'currentColor',
          'aria-hidden': 'true',
        },
        [h('circle', { cx: 12, cy: 12, r: 6 })],
      )
  },
})

// Brand marks for the agents, pulled from iconify at build time so
// runtime stays offline-friendly. Sources: `material-icon-theme:claude`,
// `arcticons:openai-chatgpt`, and `material-icon-theme:gemini-ai`.
// Re-exported individually for direct use and aggregated into `agentIcons`
// for dispatch-by-agent.
export const IconClaude = IconClaudeRaw
export const IconCodex = IconCodexRaw
export const IconGemini = IconGeminiRaw

/**
 * Global dictionary of agent → brand-mark icon component. Use as
 * `<component :is="agentIcons[agent]" />` so consumers don't have to
 * branch on the agent name themselves. Keep additions to this map in
 * sync with `Agent` in `src/types.ts`.
 */
export const agentIcons: Record<Agent, Component> = {
  claude: IconClaudeRaw,
  codex: IconCodexRaw,
  gemini: IconGeminiRaw,
}
