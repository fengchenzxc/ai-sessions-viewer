<script setup lang="ts">
import type { ProjectInfo } from '../types'
import { t } from '../i18n'
import {
  IconPinUp,
  IconPinDown,
  IconRefresh,
  IconTrashOpen,
} from '../components/icons'

type ProjState = 'pinned' | 'sunk'

defineProps<{
  x: number
  y: number
  project: ProjectInfo
  projState: ProjState | undefined
}>()

const emit = defineEmits<{
  (e: 'toggle-state', state: ProjState): void
  (e: 'refresh'): void
  (e: 'delete'): void
}>()
</script>

<template>
  <Teleport to="body">
    <div class="ctx-menu" :style="{ left: x + 'px', top: y + 'px' }">
      <button class="ctx-item" @click="emit('toggle-state', 'pinned')">
        <IconPinUp />
        {{ projState === 'pinned' ? t('proj.unpin') : t('proj.pin') }}
      </button>
      <button class="ctx-item" @click="emit('toggle-state', 'sunk')">
        <IconPinDown />
        {{ projState === 'sunk' ? t('proj.unsink') : t('proj.sink') }}
      </button>
      <div class="ctx-sep" />
      <!-- 目录已不存在 → 刷新无意义，连同分隔线一起隐藏 -->
      <template v-if="project.exists">
        <button class="ctx-item" @click="emit('refresh')">
          <IconRefresh />
          {{ t('proj.refresh') }}
        </button>
        <div class="ctx-sep" />
      </template>
      <button class="ctx-item danger" @click="emit('delete')">
        <IconTrashOpen />
        {{ t('proj.delete') }}
      </button>
    </div>
  </Teleport>
</template>
