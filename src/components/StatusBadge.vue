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
  position: relative;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: currentColor;
}

.ok .dot::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: currentColor;
  animation: ping 1.8s cubic-bezier(0.2, 0.8, 0.2, 1) infinite;
}

.warn .dot {
  animation: blink 0.9s ease-in-out infinite alternate;
}

@keyframes ping {
  to {
    transform: scale(2.4);
    opacity: 0;
  }
}

@keyframes blink {
  to {
    opacity: 0.3;
  }
}
</style>
