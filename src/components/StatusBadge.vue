<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import type { ServiceStatus } from '../api';

const props = defineProps<{ status: ServiceStatus }>();
const { t } = useI18n();
const tone = computed(
  () => ({ running: 'ok', starting: 'warn', stopping: 'warn', crashed: 'bad', stopped: '' })[props.status],
);
</script>

<template>
  <span class="badge" :class="tone">
    <span class="dot" aria-hidden="true" />
    {{ t(`status.${status}`) }}
  </span>
</template>

<style scoped>
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: currentColor;
}

.warn .dot {
  animation: pulse 1s ease-in-out infinite alternate;
}

@keyframes pulse {
  to {
    opacity: 0.3;
  }
}
</style>
