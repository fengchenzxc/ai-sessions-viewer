<script setup lang="ts">
import { computed } from 'vue'
import type { Agent, ProjectInfo } from '../types'
import { shortName } from '../format'
import { t } from '../i18n'
import { IconExternalLink, IconRefresh, IconSettings, agentIcons } from './icons'
import { latestVersion, openReleasePage, updateAvailable } from '../updateCheck'

type ProjState = 'pinned' | 'sunk'

const props = defineProps<{
  agent: Agent
  projects: ProjectInfo[]
  activeDir: string | null
  showTrash: boolean
  projPrefs: Record<string, ProjState>
  refreshing?: boolean
}>()

const emit = defineEmits<{
  (e: 'switch-agent', a: Agent): void
  (e: 'select-project', dir: string): void
  (e: 'context-menu', evt: MouseEvent, p: ProjectInfo): void
  (e: 'open-settings'): void
  (e: 'refresh'): void
}>()

const agents: Agent[] = ['claude', 'codex', 'gemini']
const agentLabel = (a: Agent) =>
  a === 'codex' ? 'Codex' : a === 'gemini' ? 'Gemini' : 'Claude'
const agentName = computed(() => agentLabel(props.agent))

function prefKey(p: ProjectInfo): string {
  return `${props.agent}::${p.dirName}`
}
function projStateOf(p: ProjectInfo): ProjState | undefined {
  return props.projPrefs[prefKey(p)]
}

const sortedProjects = computed(() => {
  const rank = (p: ProjectInfo) =>
    projStateOf(p) === 'pinned' ? 0 : projStateOf(p) === 'sunk' ? 2 : 1
  return [...props.projects].sort((a, b) => rank(a) - rank(b))
})

function pinColor(p: ProjectInfo): string {
  let h = 0
  const s = p.dirName
  for (let i = 0; i < s.length; i++) h = ((h << 5) - h + s.charCodeAt(i)) | 0
  const hue = ((h % 360) + 360) % 360
  return `hsl(${hue} 72% 52%)`
}
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar-top">
      <div class="agent-switch">
        <button
          v-for="a in agents"
          :key="a"
          :class="{ active: agent === a }"
          @click="emit('switch-agent', a)"
        >
          <component :is="agentIcons[a]" />
          <span>{{ agentLabel(a) }}</span>
        </button>
      </div>
      <div class="sidebar-sub">
        <span class="sidebar-sub-label">
          {{ agentName }} ·
          {{ t('sidebar.projectsCount', { count: projects.length }) }}
        </span>
        <!-- 刷新按钮：只重拉当前 agent 的项目 / 会话 / 当前打开的对话。
             之前挂在顶部 SidebarTopbar 上离 agent switch 较远，挪到这里
             跟 "{agent} · N projects" 同行，"刷新这家 agent" 的语义更直观。 -->
        <button
          type="button"
          class="sidebar-sub-refresh"
          :class="{ spinning: refreshing }"
          v-tooltip="t('sidebar.refresh')"
          :disabled="refreshing"
          @click="emit('refresh')"
        >
          <IconRefresh />
        </button>
      </div>
    </div>

    <div class="proj-list">
      <div
        v-for="p in sortedProjects"
        :key="p.dirName"
        class="proj-item"
        :data-path="p.displayPath"
        :class="{
          active: activeDir === p.dirName && !showTrash,
          missing: !p.exists,
          pinned: projStateOf(p) === 'pinned',
          sunk: projStateOf(p) === 'sunk',
        }"
        v-tooltip:right="p.exists ? p.displayPath : p.displayPath + t('proj.missing')"
        @click="emit('select-project', p.dirName)"
        @contextmenu="emit('context-menu', $event, p)"
      >
        <!-- 置顶项目前的小圆点：颜色按项目名稳定哈希，不同项目互不相同 -->
        <span
          v-if="projStateOf(p) === 'pinned'"
          class="pin-dot"
          :style="{ background: pinColor(p) }"
          :aria-label="t('proj.pin')"
        />
        <span class="proj-name">{{ shortName(p.displayPath) }}</span>
        <span class="proj-count">{{ p.sessionCount }}</span>
      </div>
      <div v-if="!projects.length" class="sidebar-sub" style="padding: 12px">
        {{ t('sidebar.noSessions', { agent: agentName }) }}
      </div>
    </div>

    <div class="sidebar-footer">
      <button
        class="trash-tab"
        :class="{ 'has-update': updateAvailable }"
        v-tooltip="updateAvailable
          ? t('sidebar.updateAvailable', { v: latestVersion ?? '' })
          : t('sidebar.settings')"
        @click="emit('open-settings')"
      >
        <IconSettings /> {{ t('sidebar.settings') }}
        <!-- 有新版本时，行尾多挂一个"打开 release 页"按钮（点它直接去 GitHub）+
             指示红点。@click.stop 防止冒泡到外层 button，否则会顺手把 Settings
             也打开 —— 用户其实只想去 release 页。 -->
        <span
          v-if="updateAvailable"
          class="sidebar-release-btn"
          role="button"
          tabindex="0"
          v-tooltip="t('sidebar.openRelease', { v: latestVersion ?? '' })"
          :aria-label="t('sidebar.openRelease', { v: latestVersion ?? '' })"
          @click.stop="openReleasePage()"
          @keydown.enter.stop.prevent="openReleasePage()"
          @keydown.space.stop.prevent="openReleasePage()"
        >
          <IconExternalLink />
        </span>
        <span v-if="updateAvailable" class="update-dot" aria-hidden="true" />
      </button>
    </div>
  </aside>
</template>
