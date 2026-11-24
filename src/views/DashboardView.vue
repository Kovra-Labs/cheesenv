<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { api } from '../api';
import { attempt, errorText, runningCount, store, toast } from '../store';
import ServiceCard from '../components/ServiceCard.vue';

const { t } = useI18n();
const busy = ref(false);
const crashed = computed(() => store.services.filter((s) => s.status === 'crashed').length);
const installed = computed(() => new Set(store.services.map((s) => s.id)).size);

async function startAll() {
  busy.value = true;
  try {
    const failures = await api.startAll();
    for (const f of failures) toast(`${f.id}: ${errorText(f.error)}`, 'error');
  } catch (e) {
    toast(errorText(e), 'error');
  } finally {
    busy.value = false;
  }
}

async function stopAll() {
  busy.value = true;
  await attempt(api.stopAll);
  busy.value = false;
}
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1>{{ t('dashboard.title') }}</h1>
        <p>{{ t('dashboard.subtitle') }}</p>
      </div>
      <div v-if="store.services.length" class="row">
        <button class="btn" :disabled="busy" @click="stopAll">{{ t('dashboard.stopAll') }}</button>
        <button class="btn primary" :disabled="busy" @click="startAll">{{ t('dashboard.startAll') }}</button>
      </div>
    </header>

    <section class="stats" :aria-label="t('dashboard.title')">
      <div class="card stat">
        <span class="stat-n">{{ runningCount }}</span>
        <span class="hint">{{ t('dashboard.running') }}</span>
      </div>
      <div class="card stat">
        <span class="stat-n">{{ installed }}</span>
        <span class="hint">{{ t('dashboard.installed') }}</span>
      </div>
      <div class="card stat" :class="{ alert: crashed }">
        <span class="stat-n">{{ crashed }}</span>
        <span class="hint">{{ t('dashboard.crashed') }}</span>
      </div>
    </section>

    <section v-if="store.services.length" class="list">
      <h2 class="sr-only">{{ t('nav.services') }}</h2>
      <ServiceCard v-for="s in store.services" :key="s.id" :service="s" />
    </section>
    <div v-else class="card empty">
      <p>{{ t('dashboard.empty') }}</p>
      <RouterLink class="btn primary" to="/modules">{{ t('dashboard.browse') }}</RouterLink>
    </div>

    <footer v-if="store.info" class="row hint data">
      <span>{{ t('dashboard.dataDir') }}: <code>{{ store.info.data_dir }}</code></span>
      <button class="btn small" @click="attempt(() => api.openFolder('www'))">{{ t('dashboard.openWww') }}</button>
    </footer>
  </div>
</template>

<style scoped>
.stats {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 12px;
}

.stat {
  display: grid;
  gap: 2px;
  padding: 14px 16px;
}

.stat-n {
  font-size: 26px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.stat.alert .stat-n {
  color: var(--danger);
}

.list {
  display: grid;
  gap: 10px;
}

.data {
  justify-content: space-between;
}
</style>
