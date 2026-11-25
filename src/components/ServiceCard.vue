<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { api, isAppError, type ServiceInfo } from '../api';
import { attempt, reloadServices, store, toast, toastError } from '../store';
import { categoryIcon, categoryLabel } from '../ui';
import AppIcon from './AppIcon.vue';
import StatusBadge from './StatusBadge.vue';
import CredentialsPanel from './CredentialsPanel.vue';
import CollapseTransition from './CollapseTransition.vue';

const props = defineProps<{ service: ServiceInfo; detailed?: boolean }>();
const { t } = useI18n();

const busy = ref(false);
const showCreds = ref(false);
const conflict = ref<{ port: number; suggestion: number | null } | null>(null);
const portInput = ref<number | null>(props.service.port);
watch(
  () => props.service.port,
  (p) => (portInput.value = p),
);

/** Installed web panels (phpMyAdmin, pgAdmin) that manage this service's module. */
const panels = computed(() => store.services.filter((s) => s.panel_for === props.service.name));
const openingPanel = ref<string | null>(null);

async function openPanel(id: string) {
  openingPanel.value = id;
  try {
    await attempt(() => api.openPanel(id));
  } finally {
    openingPanel.value = null;
  }
}

const active = computed(() => props.service.status !== 'stopped' && props.service.status !== 'crashed');
const running = computed(() => props.service.status === 'running');
const portDirty = computed(() => portInput.value !== props.service.port);

async function run(fn: () => Promise<unknown>) {
  busy.value = true;
  try {
    await fn();
  } finally {
    busy.value = false;
  }
}

function start() {
  return run(async () => {
    conflict.value = null;
    try {
      await api.startService(props.service.id);
      // First start may have initialised the database (credentials now exist).
      if (!props.service.initialized) await reloadServices();
    } catch (e) {
      if (isAppError(e) && e.code === 'PORT_IN_USE' && e.port) {
        conflict.value = { port: e.port, suggestion: e.suggestion ?? null };
      }
      toastError(e);
    }
  });
}

const stop = () => run(() => attempt(() => api.stopService(props.service.id)));
const restart = () => run(() => attempt(() => api.restartService(props.service.id)));
const open = () => attempt(() => api.openServiceUrl(props.service.id));
const openConfig = () => attempt(() => api.openFolder('config', props.service.id));

async function savePort(port: number | null) {
  if (!port || port < 1 || port > 65535) {
    toast(t('errors.INVALID_INPUT'), 'error');
    return;
  }
  await run(async () => {
    const ok = await attempt(() => api.setServiceOptions(props.service.id, { port }));
    if (ok) {
      await reloadServices();
      toast(t('services.portSaved'));
      const check = await api.checkPort(port).catch(() => null);
      if (check && !check.free) conflict.value = { port, suggestion: check.suggestion };
    }
  });
}

async function useSuggestion() {
  const port = conflict.value?.suggestion;
  if (!port) return;
  conflict.value = null;
  await savePort(port);
  await start();
}

const setAutostart = (v: boolean) => attempt(() => api.setServiceOptions(props.service.id, { autostart: v }));
</script>

<template>
  <article class="card svc" :class="[service.status, { detailed }]">
    <div class="svc-head">
      <span class="tile"><AppIcon :name="categoryIcon(service.category)" :size="22" /></span>
      <div class="svc-info">
        <div class="svc-title">
          <h3>{{ service.name }}</h3>
          <span class="badge mono">{{ service.version }}</span>
          <StatusBadge :status="service.status" />
        </div>
        <p class="meta">
          <span v-if="service.port">{{ t('common.port') }} <b class="mono">{{ service.port }}</b></span>
          <span v-if="service.pid">{{ t('services.pid') }} <b class="mono">{{ service.pid }}</b></span>
          <span v-if="service.recent_crashes" class="crash">{{ t('services.crashes', { n: service.recent_crashes }) }}</span>
          <span v-if="!service.port && !service.pid" class="hint">{{ categoryLabel(service.category) }}</span>
        </p>
      </div>

      <div class="actions">
        <button v-if="running && service.category === 'web-server'" class="btn" @click="open">
          <AppIcon name="external" />{{ t('services.openBrowser') }}
        </button>
        <template v-if="running">
          <button v-for="p in panels" :key="p.id" class="btn" :disabled="openingPanel !== null" @click="openPanel(p.id)">
            <AppIcon :name="openingPanel === p.id ? 'restart' : 'external'" :class="{ spin: openingPanel === p.id }" />
            {{ openingPanel === p.id ? t('services.panelStarting') : t('services.openPanel', { name: p.name }) }}
          </button>
        </template>
        <button v-if="service.panel_for && running" class="btn" :disabled="openingPanel !== null" @click="openPanel(service.id)">
          <AppIcon :name="openingPanel === service.id ? 'restart' : 'external'" :class="{ spin: openingPanel === service.id }" />
          {{ openingPanel === service.id ? t('services.panelStarting') : t('services.openPanel', { name: service.name }) }}
        </button>
        <button
          v-if="service.credentials_user"
          class="btn"
          :class="{ active: showCreds }"
          :aria-expanded="showCreds"
          @click="showCreds = !showCreds"
        >
          <AppIcon name="key" />{{ t('services.credentials') }}
        </button>
        <button v-if="!active" class="btn primary" :disabled="busy" @click="start">
          <AppIcon :name="busy ? 'restart' : 'play'" :class="{ spin: busy }" />
          {{ busy && !service.initialized ? t('services.initializing') : t('common.start') }}
        </button>
        <template v-else>
          <button
            class="btn icon-only"
            :disabled="busy || !running"
            :title="t('common.restart')"
            :aria-label="t('common.restart')"
            @click="restart"
          >
            <AppIcon name="restart" :class="{ spin: busy && running }" />
          </button>
          <button class="btn danger" :disabled="busy || service.status === 'stopping'" @click="stop">
            <AppIcon name="stop" />{{ t('common.stop') }}
          </button>
        </template>
      </div>
    </div>

    <p v-if="!service.initialized && !active" class="alert info">
      <AppIcon name="info" />{{ t('services.firstStart') }}
    </p>
    <p v-if="detailed && service.panel_for" class="hint">{{ t('services.panelHint', { target: service.panel_for }) }}</p>

    <CollapseTransition>
      <div v-if="showCreds && service.credentials_user">
        <CredentialsPanel :key="`${service.initialized}`" :service="service" />
      </div>
    </CollapseTransition>

    <CollapseTransition>
      <div v-if="conflict">
        <div class="alert" role="alert">
          <AppIcon name="alert" />
          <span class="spacer">{{ t('services.portBusy', { port: conflict.port }) }}</span>
          <button v-if="conflict.suggestion" class="btn sm primary" :disabled="busy" @click="useSuggestion">
            {{ t('services.useSuggestion', { port: conflict.suggestion }) }}
          </button>
        </div>
      </div>
    </CollapseTransition>

    <div v-if="detailed" class="svc-detail">
      <label class="field">
        <span class="label">{{ t('common.port') }}</span>
        <div class="row nowrap">
          <input
            v-model.number="portInput"
            class="input mono port"
            type="number"
            min="1"
            max="65535"
            :disabled="active || busy"
            :title="active ? t('services.stopFirst') : undefined"
            @keyup.enter="portDirty && savePort(portInput)"
          />
          <Transition name="fade">
            <button v-if="portDirty" class="btn primary" :disabled="busy" @click="savePort(portInput)">
              <AppIcon name="check" />{{ t('common.save') }}
            </button>
          </Transition>
        </div>
        <small v-if="active" class="hint">{{ t('services.stopFirst') }}</small>
      </label>
      <label class="check auto">
        <input
          class="switch"
          type="checkbox"
          :checked="service.autostart"
          @change="setAutostart(($event.target as HTMLInputElement).checked)"
        />
        <span>{{ t('services.autostart') }}</span>
      </label>
      <button class="btn" @click="openConfig"><AppIcon name="folder" />{{ t('common.config') }}</button>
    </div>
  </article>
</template>

<style scoped>
.svc {
  display: grid;
  gap: 14px;
  padding: 18px 20px;
}

.svc:hover {
  border-color: var(--line-strong);
  box-shadow: var(--shadow-md);
}

.svc-head {
  display: flex;
  align-items: center;
  gap: 16px;
  flex-wrap: wrap;
}

.svc.running .tile {
  background: var(--success-soft);
  color: var(--success);
}

.svc.crashed .tile {
  background: var(--danger-soft);
  color: var(--danger);
}

.svc.stopped .tile {
  background: var(--surface-2);
  color: var(--ink-2);
}

.svc-info {
  display: grid;
  gap: 4px;
  flex: 1 1 220px;
  min-width: 0;
}

.svc-title {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.meta {
  display: flex;
  gap: 16px;
  flex-wrap: wrap;
  color: var(--ink-2);
  font-size: 13.5px;
}

.meta b {
  color: var(--ink);
  font-weight: 600;
}

.crash {
  color: var(--danger);
  font-weight: 600;
}

.actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.btn.active {
  --btn-bg: var(--primary-soft);
  --btn-ink: var(--primary-text);
  --btn-line: transparent;
}

.svc-detail {
  display: flex;
  align-items: flex-end;
  gap: 24px;
  flex-wrap: wrap;
  padding-top: 16px;
  border-top: 1px solid var(--line);
}

.nowrap {
  flex-wrap: nowrap;
}

.port {
  width: 130px;
}

.auto {
  min-height: 42px;
}

.svc-detail > .btn {
  margin-left: auto;
}
</style>
