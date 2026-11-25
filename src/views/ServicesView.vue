<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { cardServices, store } from '../store';
import AppIcon from '../components/AppIcon.vue';
import ServiceCard from '../components/ServiceCard.vue';

type Filter = 'all' | 'running' | 'stopped';

const { t } = useI18n();
const filter = ref<Filter>('all');
const isRunning = (status: string) => status !== 'stopped' && status !== 'crashed';
const counts = computed(() => {
  const running = cardServices.value.filter((s) => isRunning(s.status)).length;
  return { all: cardServices.value.length, running, stopped: cardServices.value.length - running };
});
const shown = computed(() =>
  cardServices.value.filter((s) => filter.value === 'all' || (filter.value === 'running') === isRunning(s.status)),
);
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1>{{ t('services.title') }}</h1>
        <p>{{ t('services.subtitle') }}</p>
      </div>
    </header>

    <template v-if="store.services.length">
      <div class="row" role="group" :aria-label="t('services.filter')">
        <button
          v-for="f in ['all', 'running', 'stopped'] as Filter[]"
          :key="f"
          class="chip"
          :class="{ active: filter === f }"
          :aria-pressed="filter === f"
          @click="filter = f"
        >
          {{ t(`services.filters.${f}`) }} <span class="n">{{ counts[f] }}</span>
        </button>
      </div>
      <TransitionGroup tag="section" name="list" class="list">
        <ServiceCard v-for="s in shown" :key="s.id" :service="s" detailed />
      </TransitionGroup>
      <p v-if="!shown.length" class="card empty">{{ t('services.noneInFilter') }}</p>
    </template>
    <div v-else class="card empty">
      <span class="empty-icon"><AppIcon name="server" :size="30" /></span>
      <strong>{{ t('services.empty') }}</strong>
      <p>{{ t('dashboard.empty') }}</p>
      <RouterLink class="btn lg primary" to="/modules"><AppIcon name="download" />{{ t('dashboard.browse') }}</RouterLink>
    </div>
  </div>
</template>

<style scoped>
.list {
  display: grid;
  gap: 14px;
}
</style>
