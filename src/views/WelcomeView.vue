<script setup lang="ts">
import { computed } from 'vue'
import type { Agent, ProjectInfo } from '../types'
import { shortName } from '../format'
import { t } from '../i18n'
import { getRecents } from '../recents'
import {
  IconEmptyBox,
  IconHistory,
  IconChevronRight,
  IconGithub,
  agentIcons,
} from '../components/icons'

const props = defineProps<{
  agent: Agent
  projects: ProjectInfo[]
}>()

const emit = defineEmits<{
  (e: 'select-project', dir: string): void
  (e: 'switch-agent', a: Agent): void
  (e: 'open-repo'): void
}>()

const agents: Agent[] = ['claude', 'codex']
const agentLabel = (a: Agent) => (a === 'codex' ? 'Codex' : 'Claude')

// 最近打开过的项目：拿 recents 里的 dirName 去当前 projects 取真身，
// 过滤掉已删除 / 换 agent 后不存在的（getRecents 读 recents.value，computed 自动随它刷新）。
const recentProjects = computed<ProjectInfo[]>(() => {
  const byDir = new Map(props.projects.map((p) => [p.dirName, p]))
  return getRecents(props.agent)
    .map((dir) => byDir.get(dir))
    .filter((p): p is ProjectInfo => !!p)
})
</script>

<template>
  <div class="welcome">
    <!-- 仓库入口：固定在主页面右上角 -->
    <button
      class="welcome-github"
      v-tooltip="t('topbar.github')"
      @click="emit('open-repo')"
    >
      <IconGithub />
    </button>
    <div class="welcome-inner">
      <div class="welcome-logo"><IconEmptyBox /></div>
      <h1 class="welcome-title">Claude Session Viewer</h1>

      <!-- 当前 agent 切换 -->
      <div class="welcome-agents">
        <button
          v-for="a in agents"
          :key="a"
          class="welcome-agent"
          :class="{ active: a === agent }"
          @click="emit('switch-agent', a)"
        >
          <component :is="agentIcons[a]" />
          {{ agentLabel(a) }}
        </button>
      </div>

      <!-- 最近打开过的项目 —— 快捷跳转 -->
      <div v-if="recentProjects.length" class="welcome-recents">
        <div class="welcome-section">
          <IconHistory />
          <span>{{ t('welcome.recent') }}</span>
        </div>
        <button
          v-for="p in recentProjects"
          :key="p.dirName"
          class="welcome-recent"
          :class="{ missing: !p.exists }"
          v-tooltip:right="p.exists ? p.displayPath : p.displayPath + t('proj.missing')"
          @click="emit('select-project', p.dirName)"
        >
          <span class="welcome-recent-name">{{ shortName(p.displayPath) }}</span>
          <span class="proj-count">{{ p.sessionCount }}</span>
          <IconChevronRight class="welcome-recent-go" />
        </button>
      </div>

      <!-- 没有最近记录时回退到原提示 -->
      <p v-else class="welcome-hint">
        {{ t('main.pickProject', { agent: agentLabel(agent) }) }}
      </p>
    </div>
  </div>
</template>
