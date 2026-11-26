<script setup lang="ts">
import { ref } from 'vue';
import { useI18n } from 'vue-i18n';
import type { Release } from '../changelog';
import AppIcon from './AppIcon.vue';
import ChangelogList from './ChangelogList.vue';

defineProps<{ title: string; hint?: string; releases: Release[] }>();
const { t } = useI18n();
const dialog = ref<HTMLDialogElement | null>(null);

defineExpose({ open: () => dialog.value?.showModal() });
</script>

<template>
  <dialog ref="dialog" class="dialog changelog-dialog" :aria-label="title">
    <div class="dialog-body">
      <div class="dialog-head">
        <span class="tile"><AppIcon name="logs" :size="22" /></span>
        <div>
          <h2>{{ title }}</h2>
          <p v-if="hint">{{ hint }}</p>
        </div>
      </div>
      <div class="scroll">
        <ChangelogList :releases="releases" />
      </div>
      <div class="dialog-actions">
        <button type="button" class="btn primary" autofocus @click="dialog?.close()">{{ t('common.close') }}</button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
.changelog-dialog {
  width: min(600px, calc(100vw - 32px));
}

.dialog-head .tile {
  background: var(--primary-soft);
  color: var(--primary-text);
}

.scroll {
  max-height: min(60vh, 520px);
  overflow-y: auto;
  padding-right: 4px;
}
</style>
