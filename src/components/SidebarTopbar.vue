<script setup lang="ts">
import { t } from '../i18n'
import { IconSidebar, IconRefresh, IconTrashOpen } from './icons'

defineProps<{
  refreshing: boolean
  showTrash: boolean
  hasTrash: boolean
}>()

const emit = defineEmits<{
  (e: 'toggle-sidebar'): void
  (e: 'refresh'): void
  (e: 'open-trash'): void
}>()
</script>

<template>
  <div class="topbar-sidebar-zone">
    <div class="topbar-icons">
      <button
        class="top-btn"
        v-tooltip="t('sidebar.toggle')"
        @click="emit('toggle-sidebar')"
      >
        <IconSidebar />
      </button>
      <button
        class="top-btn"
        :class="{ spinning: refreshing }"
        v-tooltip="t('sidebar.refresh')"
        :disabled="refreshing"
        @click="emit('refresh')"
      >
        <IconRefresh />
      </button>
    </div>
    <button
      class="top-btn topbar-trash-btn"
      :class="{ active: showTrash }"
      v-tooltip="t('sidebar.trash')"
      @click="emit('open-trash')"
    >
      <IconTrashOpen />
      <span v-if="hasTrash" class="trash-dot" aria-hidden="true" />
    </button>
  </div>
</template>
