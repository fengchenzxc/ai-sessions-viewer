<script setup lang="ts">
import { computed } from 'vue'
import type { Agent, ProjectInfo } from '../types'
import { shortName } from '../format'
import { t } from '../i18n'
import { IconSettings } from './icons'

type ProjState = 'pinned' | 'sunk'

const props = defineProps<{
  agent: Agent
  projects: ProjectInfo[]
  activeDir: string | null
  showTrash: boolean
  projPrefs: Record<string, ProjState>
}>()

const emit = defineEmits<{
  (e: 'switch-agent', a: Agent): void
  (e: 'select-project', dir: string): void
  (e: 'context-menu', evt: MouseEvent, p: ProjectInfo): void
  (e: 'open-settings'): void
}>()

const agentName = computed(() => (props.agent === 'codex' ? 'Codex' : 'Claude'))

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
          :class="{ active: agent === 'claude' }"
          @click="emit('switch-agent', 'claude')"
        >
          Claude
        </button>
        <button
          :class="{ active: agent === 'codex' }"
          @click="emit('switch-agent', 'codex')"
        >
          Codex
        </button>
      </div>
      <div class="sidebar-sub">
        {{ agentName }} ·
        {{ t('sidebar.projectsCount', { count: projects.length }) }}
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
      <button class="trash-tab" @click="emit('open-settings')">
        <IconSettings /> {{ t('sidebar.settings') }}
      </button>
    </div>
  </aside>
</template>
