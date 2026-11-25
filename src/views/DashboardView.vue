<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { api } from '../api';
import { attempt, cardServices, errorText, runningCount, store, toast } from '../store';
import AppIcon, { type IconName } from '../components/AppIcon.vue';
import ServiceCard from '../components/ServiceCard.vue';

const { t } = useI18n();
const busy = ref<'start' | 'stop' | null>(null);
const crashed = computed(() => store.services.filter((s) => s.status === 'crashed').length);
const installed = computed(() => cardServices.value.length);

const stats = computed<{ key: string; n: number; icon: IconName; tone: string }[]>(() => [
  { key: 'running', n: runningCount.value, icon: 'activity', tone: 'ok' },
  { key: 'installed', n: installed.value, icon: 'box', tone: 'info' },
  { key: 'crashed', n: crashed.value, icon: 'alert', tone: crashed.value ? 'bad' : 'muted' },
]);

async function startAll() {
  busy.value = 'start';
  try {
    const failures = await api.startAll();
    for (const f of failures) toast(`${f.id}: ${errorText(f.error)}`, 'error');
  } catch (e) {
    toast(errorText(e), 'error');
  } finally {
    busy.value = null;
  }
}

async function stopAll() {
  busy.value = 'stop';
  await attempt(api.stopAll);
  busy.value = null;
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
        <button class="btn lg" :disabled="busy !== null || !runningCount" @click="stopAll">
          <AppIcon :name="busy === 'stop' ? 'restart' : 'stop'" :class="{ spin: busy === 'stop' }" />
          {{ t('dashboard.stopAll') }}
        </button>
        <button class="btn lg primary" :disabled="busy !== null" @click="startAll">
          <AppIcon :name="busy === 'start' ? 'restart' : 'play'" :class="{ spin: busy === 'start' }" />
          {{ t('dashboard.startAll') }}
        </button>
      </div>
    </header>

    <section class="stats" :aria-label="t('dashboard.title')">
      <div v-for="s in stats" :key="s.key" class="card stat" :class="s.tone">
        <span class="tile"><AppIcon :name="s.icon" :size="22" /></span>
        <div>
          <span class="stat-n">{{ s.n }}</span>
          <span class="hint">{{ t(`dashboard.${s.key}`) }}</span>
        </div>
      </div>
    </section>

    <section class="quick" :aria-label="t('dashboard.quick')">
      <button class="card qa" @click="attempt(() => api.openFolder('www'))">
        <span class="tile"><AppIcon name="globe" /></span>
        <span><strong>{{ t('dashboard.openWww') }}</strong><small>{{ t('dashboard.openWwwHint') }}</small></span>
      </button>
      <button class="card qa" @click="attempt(() => api.openFolder('root'))">
        <span class="tile"><AppIcon name="folder" /></span>
        <span><strong>{{ t('dashboard.openData') }}</strong><small>{{ t('dashboard.openDataHint') }}</small></span>
      </button>
      <RouterLink class="card qa" to="/modules">
        <span class="tile"><AppIcon name="download" /></span>
        <span><strong>{{ t('dashboard.browse') }}</strong><small>{{ t('dashboard.browseHint') }}</small></span>
      </RouterLink>
      <RouterLink class="card qa" to="/logs">
        <span class="tile"><AppIcon name="logs" /></span>
        <span><strong>{{ t('dashboard.openLogs') }}</strong><small>{{ t('dashboard.openLogsHint') }}</small></span>
      </RouterLink>
    </section>

    <section class="list">
      <div class="section-title">
        <h2>{{ t('dashboard.yourServices') }}</h2>
        <RouterLink v-if="store.services.length" class="btn sm ghost" to="/services">
          {{ t('dashboard.manage') }}<AppIcon name="arrowRight" :size="16" />
        </RouterLink>
      </div>
      <TransitionGroup v-if="store.services.length" tag="div" name="list" class="cards">
        <ServiceCard v-for="s in cardServices" :key="s.id" :service="s" />
      </TransitionGroup>
      <div v-else class="card empty">
        <span class="empty-icon"><AppIcon name="box" :size="30" /></span>
        <strong>{{ t('dashboard.emptyTitle') }}</strong>
        <p>{{ t('dashboard.empty') }}</p>
        <RouterLink class="btn lg primary" to="/modules">
          <AppIcon name="download" />{{ t('dashboard.browse') }}
        </RouterLink>
      </div>
    </section>

    <footer v-if="store.info" class="hint data">
      {{ t('dashboard.dataDir') }}: <code>{{ store.info.data_dir }}</code>
    </footer>
  </div>
</template>

<style scoped>
.stats {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 16px;
}

.stat {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 20px;
}

.stat:hover {
  transform: translateY(-2px);
  box-shadow: var(--shadow-md);
}

.stat > div {
  display: grid;
}

.stat .tile {
  width: 52px;
  height: 52px;
  border-radius: 14px;
}

.stat.ok .tile {
  background: var(--success-soft);
  color: var(--success);
}

.stat.bad .tile {
  background: var(--danger-soft);
  color: var(--danger);
}

.stat.muted .tile {
  background: var(--surface-2);
  color: var(--ink-2);
}

.stat-n {
  font-size: 30px;
  font-weight: 700;
  line-height: 1.1;
  font-variant-numeric: tabular-nums;
}

.stat.bad .stat-n {
  color: var(--danger);
}

.list,
.cards {
  display: grid;
  gap: 14px;
}

.quick {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 14px;
}

.qa {
  display: flex;
  align-items: center;
  gap: 14px;
  width: 100%;
  padding: 16px;
  color: inherit;
  font: inherit;
  text-align: left;
  text-decoration: none;
  cursor: pointer;
  transition:
    background-color var(--dur) var(--ease),
    transform var(--dur) var(--ease);
}

.qa:hover {
  border-color: var(--primary);
  box-shadow: var(--shadow-md);
  transform: translateY(-2px);
}

.qa:hover .tile {
  background: var(--primary);
  color: var(--on-primary);
}

.qa:active {
  transform: scale(0.98);
}

.qa > span:last-child {
  display: grid;
  line-height: 1.35;
}

.qa small {
  color: var(--ink-2);
  font-size: 12.5px;
}

.data code {
  color: var(--ink);
  overflow-wrap: anywhere;
}

@media (max-width: 1080px) {
  .quick {
    grid-template-columns: repeat(2, 1fr);
  }
}
</style>
