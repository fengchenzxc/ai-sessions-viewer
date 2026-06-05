<script setup lang="ts">
// 跨 / 单 agent 的统计总览。数据由 `useStatsStream` 流式喂入 —— 后端 worker 每处理
// 一批 JSONL 就 emit 一次 partial AgentStats，本组件用它做骨架替换式渲染：
//
//   1. 切换 scope / range / 刷新 / 进入页面：立即重启一次扫描；先看到 computing
//      骨架 + 进度数字，然后随 partial 逐步把卡片填充进去。
//   2. 没有 cost / by_model 这类纯数字的视觉差异时（譬如 Gemini scope），相关
//      卡片仍然渲染但内容为"无数据"。
//   3. 图表用 AntV G2（每张图一个组件），自己监听 theme/data 变化重建。

import { computed, onMounted, watch } from 'vue'
import type { Agent, StatsRange, StatsScope } from '../types'
import { t } from '../i18n'
import { formatTime, formatTokens, shortName } from '../format'
import {
  IconActivity,
  IconArrowLeft,
  IconChart,
  IconChat,
  IconClose,
  IconRefresh,
  IconWallet,
  IconZap,
} from '../components/icons'
import { useStatsStream } from '../stats'
import { statsRange, statsScope } from '../settings'
import { forceRefresh as forceRefreshPricing, pricingStatus, refreshStatus as refreshPricingStatus, watchUntilReady as watchPricingUntilReady } from '../pricing'
import StatsDailyChart from '../components/StatsDailyChart.vue'
import StatsModelChart from '../components/StatsModelChart.vue'
import StatsActivityChart from '../components/StatsActivityChart.vue'
import StatsLoadingIcon from '../components/StatsLoadingIcon.vue'

const props = defineProps<{
  /** 单会话模式：传入这个对象 → 隐藏 scope / range pills，强制走
   *  `session:<agent>:<path>` 后端 scope。从聊天页顶栏的「会话统计」按钮进入时用。
   *  非 session 模式（全局统计）下，scope 由组件内部状态独立决定，默认 'all'，
   *  不受外层 agent 切换影响。 */
  session?: { agent: Agent; path: string; title?: string } | null
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'open-project', dir: string): void
  /** Top Sessions 行点击 → 钻进这条 session 的单会话统计。
   *  父级用 (agent, path, title) 立刻把 props.session 切到这条，触发流重启。 */
  (e: 'open-session', agent: Agent, path: string, title: string): void
}>()

// ---------- 控制面板 ----------
// scope / range 是全局持久化偏好（settings.ts 里 watch 自动写 localStorage）。
// 默认 all agents + all time；用户切了之后下次进页面保留上次选择。
const scope = statsScope
const range = statsRange

const SCOPES: { value: StatsScope; key: string }[] = [
  { value: 'all', key: 'stats.scope.all' },
  { value: 'claude', key: 'stats.scope.claude' },
  { value: 'codex', key: 'stats.scope.codex' },
  { value: 'gemini', key: 'stats.scope.gemini' },
]
const RANGES: { value: StatsRange; key: string }[] = [
  { value: 'today', key: 'stats.range.today' },
  { value: 'days7', key: 'stats.range.days7' },
  { value: 'days30', key: 'stats.range.days30' },
  { value: 'month', key: 'stats.range.month' },
  { value: 'months6', key: 'stats.range.months6' },
]

const stream = useStatsStream()
const { stats, stage, progress, error } = stream

// 单会话模式：scope 是固定的 session: 串；range 被后端忽略，但本地保留默认值便于 UI。
const isSession = computed(() => !!props.session)
const sessionScope = computed(() =>
  props.session ? `session:${props.session.agent}:${props.session.path}` : '',
)

function refresh() {
  if (isSession.value) {
    stream.start(sessionScope.value, range.value)
  } else {
    stream.start(scope.value, range.value)
  }
}

onMounted(() => {
  // scope 保持组件内部状态：默认 'all'，不跟随侧栏当前 agent 走 ——
  // 统计页是「全局视角」，用户在这里自己决定看哪个 agent。
  refresh()
  // 价格表（LiteLLM 上游）—— 启动期可能还在拉。先读一次状态，没就绪就开 poll。
  refreshPricingStatus().then(() => watchPricingUntilReady())
})

const pricingReady = computed(() => pricingStatus.value.loaded)
const pricingErrored = computed(
  () => !pricingStatus.value.loaded && !!pricingStatus.value.lastError,
)
const pricingLoading = computed(
  () => !pricingStatus.value.loaded && !pricingStatus.value.lastError,
)

async function retryPricing() {
  try {
    await forceRefreshPricing()
  } catch {
    // forceRefresh 内部已 refreshStatus，error 已落进 pricingStatus.lastError
  }
}

// 注意：props.agent（侧栏当前 agent）刻意不 watch —— 用户在侧栏切 agent
// 不应该悄悄改写本页面的 scope，否则 pill 看似"自己跳动"。
watch(scope, () => {
  if (!isSession.value) refresh()
})
watch(range, refresh)
// 切换 session 目标也要重启流：
//   - session A → session B：单会话间跳
//   - session → null：从单会话「返回」回到全局视图（必须 refresh，否则
//     后端 worker 仍在跑 session:<path> scope，前端就一直显示那条 session 的数据）
watch(
  () => props.session?.path,
  () => refresh(),
)

// ---------- 数字格式化 ------------------------------------------------------
function fmtUsd(n: number): string {
  // 不要做 ≥$10 一刀切舍到整数 / ≥$1000 转 K 的事 —— 跟 codeburn / 任何
  // 财务面板对账时 `$38.55` 显示成 `$39` 会让人怀疑算法。统一 2 位小数。
  if (!Number.isFinite(n) || n === 0) return '$0.00'
  if (n < 0.01) return '<$0.01'
  return `$${n.toFixed(2)}`
}
function pct(n: number): string {
  if (!Number.isFinite(n) || n === 0) return '0%'
  return `${(n * 100).toFixed(1)}%`
}

// ---------- 派生数据 ------------------------------------------------------
const isComputing = computed(() => stage.value === 'computing')
const hasStats = computed(() => !!stats.value)
const isEmpty = computed(
  () => stage.value === 'done' && (stats.value?.sessionCount ?? 0) === 0,
)

const headerLine = computed(() => {
  const s = stats.value
  if (!s) return null
  return {
    cost: fmtUsd(s.costUsd),
    calls: s.callCount.toLocaleString(),
    sessions: s.sessionCount.toLocaleString(),
    cacheHit: pct(s.cacheHitRate),
    tokensIn: formatTokens(s.usage.inputTokens),
    tokensOut: formatTokens(s.usage.outputTokens),
    cached: formatTokens(s.usage.cacheReadInputTokens),
    written: formatTokens(s.usage.cacheCreationInputTokens),
  }
})

// 排行块的最大值（用来算横向 bar 的百分比宽度）
function maxOf<T>(arr: T[], pick: (x: T) => number): number {
  let m = 0
  for (const x of arr) m = Math.max(m, pick(x))
  return m || 1
}

// ---------- 图表数据（喂给 G2 子组件）--------------------------------------
const dailyData = computed(() => {
  const s = stats.value
  if (!s) return []
  return s.dailyActivity.map((d) => ({
    date: d.date,
    cost: d.costUsd,
    calls: d.callCount,
  }))
})

const modelData = computed(() => {
  const s = stats.value
  if (!s) return []
  return s.byModel.map((m) => ({
    label: m.label || m.model,
    cost: m.costUsd,
  }))
})

const activityData = computed(() => {
  const s = stats.value
  if (!s) return []
  return s.byActivity.map((a) => ({
    name: t(`stats.activity.${a.key}`),
    cost: a.costUsd,
  }))
})

// 子组件 helper —— 把可能为空的数字稳妥地变成可读 string。
function fmtNum(n: number | undefined): string {
  if (!n || n === 0) return '0'
  return n.toLocaleString()
}

function emptyHint(arr: { length: number } | undefined): boolean {
  return !arr || arr.length === 0
}

function asAgent(name: string): Agent {
  return (name === 'codex' || name === 'gemini' ? name : 'claude') as Agent
}
</script>

<template>
  <div class="stats-view">
    <!-- 顶栏 -->
    <div class="stats-head">
      <!-- 单会话模式：左侧加一个明确的「返回」按钮，发 close 事件后 App.vue
           保留 openSession 不动，自动回到原聊天页。右侧的 close X 仍保留作为
           兜底，但视觉重心放在这个箭头上 —— 它语义上是「回到我刚才在看的会话」。 -->
      <button
        v-if="isSession"
        class="icon-btn stats-back-btn"
        v-tooltip="t('chat.back')"
        @click="emit('close')"
      >
        <IconArrowLeft />
      </button>
      <div class="stats-title">
        {{ isSession ? t('stats.sessionTitle') : t('stats.title') }}
        <span v-if="isSession && props.session?.title" class="stats-title-sub">
          · {{ props.session.title }}
        </span>
      </div>
      <div class="stats-controls">
        <!-- 单会话模式下隐藏 scope / range pills —— 单条 JSONL 没有 scope/range 切换的意义 -->
        <template v-if="!isSession">
          <div class="stats-pill-group">
            <span class="stats-pill-label">{{ t('stats.scope.label') }}:</span>
            <div class="stats-pills">
              <button
                v-for="s in SCOPES"
                :key="s.value"
                class="stats-pill"
                :class="{ active: scope === s.value }"
                @click="scope = s.value"
              >
                {{ t(s.key) }}
              </button>
            </div>
          </div>
          <div class="stats-pill-group">
            <span class="stats-pill-label">{{ t('stats.range.label') }}:</span>
            <div class="stats-pills">
              <button
                v-for="r in RANGES"
                :key="r.value"
                class="stats-pill"
                :class="{ active: range === r.value }"
                @click="range = r.value"
              >
                {{ t(r.key) }}
              </button>
            </div>
          </div>
        </template>
      </div>
      <div class="stats-actions">
        <button
          class="icon-btn"
          :class="{ spinning: isComputing }"
          v-tooltip="t('stats.refresh')"
          @click="refresh"
        >
          <IconRefresh />
        </button>
        <button class="icon-btn" v-tooltip="t('common.close')" @click="emit('close')">
          <IconClose />
        </button>
      </div>
    </div>

    <!-- Hero 顶栏：scope/range 标签 + 4 个 KPI 卡片（cost / calls / sessions / cache hit）+ 副 token 行 -->
    <!-- 价格表没就绪时不渲染 hero —— hero 里那一坨 cost 都是 0，避免歧义。 -->
    <div class="stats-hero" v-if="headerLine && pricingReady">
      <div class="stats-hero-row">
        <template v-if="isSession">
          <span class="stats-hero-scope">{{ props.session?.title || t('stats.sessionTitle') }}</span>
        </template>
        <template v-else>
          <span class="stats-hero-scope">{{ t(`stats.scope.${scope}`) }}</span>
          <span class="stats-hero-range">· {{ t(`stats.range.${range}`) }}</span>
        </template>
        <span v-if="isComputing" class="stats-hero-status">
          {{ progress.total
            ? t('stats.computing', { processed: progress.processed, total: progress.total })
            : t('stats.computingNoTotal')
          }}
        </span>
      </div>
      <div class="kpi-grid">
        <div class="kpi-card kpi-card--brand">
          <div class="kpi-card-icon"><IconWallet /></div>
          <div class="kpi-card-meta">
            <div class="kpi-card-label">{{ t('stats.header.cost') }}</div>
            <div class="kpi-card-num">{{ headerLine.cost }}</div>
          </div>
        </div>
        <div class="kpi-card">
          <div class="kpi-card-icon"><IconActivity /></div>
          <div class="kpi-card-meta">
            <div class="kpi-card-label">{{ t('stats.header.calls') }}</div>
            <div class="kpi-card-num">{{ headerLine.calls }}</div>
          </div>
        </div>
        <div class="kpi-card">
          <div class="kpi-card-icon"><IconChat /></div>
          <div class="kpi-card-meta">
            <div class="kpi-card-label">{{ t('stats.header.sessions') }}</div>
            <div class="kpi-card-num">{{ headerLine.sessions }}</div>
          </div>
        </div>
        <div class="kpi-card">
          <div class="kpi-card-icon"><IconZap /></div>
          <div class="kpi-card-meta">
            <div class="kpi-card-label">{{ t('stats.header.cacheHit') }}</div>
            <div class="kpi-card-num">{{ headerLine.cacheHit }}</div>
          </div>
        </div>
      </div>
      <div class="stats-hero-tokens">
        <span><strong>{{ headerLine.tokensIn }}</strong> {{ t('stats.header.tokensIn') }}</span>
        <span class="kpi-sep" />
        <span><strong>{{ headerLine.tokensOut }}</strong> {{ t('stats.header.tokensOut') }}</span>
        <span class="kpi-sep" />
        <span><strong>{{ headerLine.cached }}</strong> {{ t('stats.header.cached') }}</span>
        <span class="kpi-sep" />
        <span><strong>{{ headerLine.written }}</strong> {{ t('stats.header.written') }}</span>
      </div>
    </div>

    <!-- 价格表加载失败 —— 优先级最高：没有价格 cost 全是 0，stats 没意义。
         一行说明 + 一个 Retry 按钮（调 refresh_pricing Tauri 命令重拉一次）。 -->
    <div v-if="pricingErrored" class="stats-empty error">
      <div>{{ t('stats.pricing.error') }}</div>
      <button class="btn" style="margin-top: 12px" @click="retryPricing">
        {{ t('stats.pricing.retry') }}
      </button>
    </div>

    <!-- 价格表还在拉（启动期 / 用户点过 Retry）—— 复用 scan loading 视觉 -->
    <div v-else-if="pricingLoading" class="stats-empty">
      <div class="big"><StatsLoadingIcon /></div>
      <div class="stats-loading-dots">{{ t('stats.pricing.loading').replace(/[.…]+$/, '') }}</div>
    </div>

    <!-- 扫描错误 -->
    <div v-else-if="stage === 'error'" class="stats-empty error">
      <div>{{ t('stats.error', { e: error }) }}</div>
    </div>

    <!-- computing 骨架（首次扫描，还没有 partial）-->
    <div v-else-if="isComputing && !hasStats" class="stats-empty">
      <div class="big"><StatsLoadingIcon /></div>
      <div class="stats-loading-dots">{{ t('stats.computingNoTotal').replace(/[.…]+$/, '') }}</div>
    </div>

    <!-- 空状态（已完成但没数据）-->
    <div v-else-if="isEmpty" class="stats-empty">
      <div class="big"><IconChart /></div>
      <div>{{ t('stats.empty') }}</div>
    </div>

    <!-- 主体 -->
    <div v-else-if="stats" class="stats-body">
      <!-- 行 1：Daily activity 折线/柱状 + By Project bar list（session 模式下只剩 Daily）-->
      <div class="stats-row" :class="{ 'stats-row-cols': !isSession }">
        <div class="stats-block stats-block-chart">
          <div class="stats-block-title">{{ t('stats.daily.title') }}</div>
          <div v-if="emptyHint(stats.dailyActivity)" class="stats-block-empty">
            {{ t('stats.daily.empty') }}
          </div>
          <div v-else class="stats-chart">
            <StatsDailyChart
              :key="`${scope}-${range}-daily`"
              :data="dailyData"
            />
          </div>
        </div>
        <div class="stats-block" v-if="!isSession">
          <div class="stats-block-title">{{ t('stats.byProject.title') }}</div>
          <div v-if="emptyHint(stats.projects)" class="stats-block-empty">
            {{ t('common.loading') }}
          </div>
          <div v-else class="bar-list">
            <div
              v-for="p in stats.projects.slice(0, 8)"
              :key="p.dirName"
              class="bar-row"
              role="button"
              tabindex="0"
              v-tooltip="p.displayPath"
              @click="emit('open-project', p.dirName)"
              @keydown.enter.prevent="emit('open-project', p.dirName)"
            >
              <div class="bar-name">{{ shortName(p.displayPath) }}</div>
              <div class="bar-track">
                <div
                  class="bar-fill"
                  :style="{
                    width: `${(p.costUsd / maxOf(stats!.projects, (x) => x.costUsd)) * 100}%`,
                  }"
                />
              </div>
              <div class="bar-val">{{ fmtUsd(p.costUsd) }}</div>
              <div class="bar-meta">{{ fmtNum(p.sessionCount) }}</div>
            </div>
          </div>
        </div>
      </div>

      <!-- 行 2：Top sessions full-width bar list（session 模式下省略 —— 只一条没意义）-->
      <div class="stats-block" v-if="!isSession">
        <div class="stats-block-title">{{ t('stats.topSessions.title') }}</div>
        <div v-if="emptyHint(stats.topSessions)" class="stats-block-empty">—</div>
        <div v-else class="bar-list bar-list-sessions">
          <div
            v-for="s in stats.topSessions"
            :key="s.path"
            class="bar-row"
            role="button"
            tabindex="0"
            v-tooltip="`${s.path}`"
            @click="emit('open-session', asAgent(s.agent), s.path, s.title)"
            @keydown.enter.prevent="emit('open-session', asAgent(s.agent), s.path, s.title)"
          >
            <div class="bar-date">{{ formatTime(s.lastModified) }}</div>
            <div class="bar-name">{{ shortName(s.projectDisplay) }}</div>
            <div class="bar-title">{{ s.title }}</div>
            <div class="bar-track">
              <div
                class="bar-fill"
                :style="{
                  width: `${(s.costUsd / maxOf(stats!.topSessions, (x) => x.costUsd)) * 100}%`,
                }"
              />
            </div>
            <div class="bar-val">{{ fmtUsd(s.costUsd) }}</div>
            <div class="bar-meta">{{ fmtNum(s.callCount) }}</div>
          </div>
        </div>
      </div>

      <!-- 行 3：By Activity 横向柱状 + By Model 圆环 -->
      <div class="stats-row stats-row-cols">
        <div class="stats-block stats-block-chart">
          <div class="stats-block-title">{{ t('stats.byActivity.title') }}</div>
          <div v-if="emptyHint(stats.byActivity)" class="stats-block-empty">—</div>
          <div v-else class="stats-chart stats-chart-tall">
            <StatsActivityChart
              :key="`${scope}-${range}-activity`"
              :data="activityData"
            />
          </div>
        </div>
        <div class="stats-block stats-block-chart">
          <div class="stats-block-title">{{ t('stats.byModel.title') }}</div>
          <div v-if="emptyHint(stats.byModel)" class="stats-block-empty">—</div>
          <div v-else class="stats-chart stats-chart-tall">
            <StatsModelChart
              :key="`${scope}-${range}-model`"
              :data="modelData"
            />
          </div>
        </div>
      </div>

      <!-- 行 4：Core Tools + Shell Commands -->
      <div class="stats-row stats-row-cols">
        <div class="stats-block">
          <div class="stats-block-title">{{ t('stats.byTool.title') }}</div>
          <div v-if="emptyHint(stats.byTool)" class="stats-block-empty">—</div>
          <div v-else class="bar-list">
            <div
              v-for="x in stats.byTool.slice(0, 10)"
              :key="x.name"
              class="bar-row no-click"
            >
              <div class="bar-name">{{ x.name }}</div>
              <div class="bar-track">
                <div
                  class="bar-fill"
                  :style="{
                    width: `${(x.count / maxOf(stats!.byTool, (y) => y.count)) * 100}%`,
                  }"
                />
              </div>
              <div class="bar-val">{{ x.count.toLocaleString() }}</div>
            </div>
          </div>
        </div>
        <div class="stats-block">
          <div class="stats-block-title">{{ t('stats.byShell.title') }}</div>
          <div v-if="emptyHint(stats.byShell)" class="stats-block-empty">—</div>
          <div v-else class="bar-list">
            <div
              v-for="x in stats.byShell.slice(0, 10)"
              :key="x.name"
              class="bar-row no-click"
            >
              <div class="bar-name">{{ x.name }}</div>
              <div class="bar-track">
                <div
                  class="bar-fill"
                  :style="{
                    width: `${(x.count / maxOf(stats!.byShell, (y) => y.count)) * 100}%`,
                  }"
                />
              </div>
              <div class="bar-val">{{ x.count.toLocaleString() }}</div>
            </div>
          </div>
        </div>
      </div>

      <!-- 行 5：MCP servers full-width（一般条目不多）-->
      <div class="stats-block" v-if="stats.byMcp.length">
        <div class="stats-block-title">{{ t('stats.byMcp.title') }}</div>
        <div class="bar-list">
          <div
            v-for="x in stats.byMcp.slice(0, 10)"
            :key="x.name"
            class="bar-row no-click"
          >
            <div class="bar-name">{{ x.name }}</div>
            <div class="bar-track">
              <div
                class="bar-fill"
                :style="{
                  width: `${(x.count / maxOf(stats!.byMcp, (y) => y.count)) * 100}%`,
                }"
              />
            </div>
            <div class="bar-val">{{ x.count.toLocaleString() }}</div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
