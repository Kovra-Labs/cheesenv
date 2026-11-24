<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { api, isAppError, type ServiceInfo } from '../api';
import { attempt, toast, toastError } from '../store';
import StatusBadge from './StatusBadge.vue';

const props = defineProps<{ service: ServiceInfo; detailed?: boolean }>();
const { t } = useI18n();

const busy = ref(false);
const conflict = ref<{ port: number; suggestion: number | null } | null>(null);
const portInput = ref<number | null>(props.service.port);
watch(
  () => props.service.port,
  (p) => (portInput.value = p),
);

const active = computed(() => props.service.status !== 'stopped' && props.service.status !== 'crashed');
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
  <article class="card svc">
    <div class="svc-main">
      <div class="svc-title">
        <h3>{{ service.name }}</h3>
        <span class="mono ver">{{ service.version }}</span>
        <StatusBadge :status="service.status" />
      </div>
      <p class="hint">
        <span v-if="service.port">{{ t('common.port') }} <span class="mono">{{ service.port }}</span></span>
        <span v-if="service.pid"> · {{ t('services.pid') }} <span class="mono">{{ service.pid }}</span></span>
        <span v-if="service.recent_crashes" class="crash"> · {{ t('services.crashes', { n: service.recent_crashes }) }}</span>
      </p>
    </div>

    <div class="row actions">
      <button v-if="service.status === 'running' && service.category === 'web-server'" class="btn small" @click="open">
        {{ t('services.openBrowser') }}
      </button>
      <button v-if="!active" class="btn small primary" :disabled="busy" @click="start">{{ t('common.start') }}</button>
      <template v-else>
        <button class="btn small" :disabled="busy || service.status !== 'running'" @click="restart">
          {{ t('common.restart') }}
        </button>
        <button class="btn small" :disabled="busy || service.status === 'stopping'" @click="stop">
          {{ t('common.stop') }}
        </button>
      </template>
    </div>

    <div v-if="conflict" class="conflict" role="alert">
      <span>{{ t('services.portBusy', { port: conflict.port }) }}</span>
      <button v-if="conflict.suggestion" class="btn small" :disabled="busy" @click="useSuggestion">
        {{ t('services.useSuggestion', { port: conflict.suggestion }) }}
      </button>
    </div>

    <div v-if="detailed" class="svc-detail">
      <label class="field port">
        <span>{{ t('common.port') }}</span>
        <div class="row">
          <input
            v-model.number="portInput"
            class="input mono"
            type="number"
            min="1"
            max="65535"
            :disabled="active || busy"
            :title="active ? t('services.stopFirst') : undefined"
          />
          <button v-if="portDirty" class="btn small" :disabled="busy" @click="savePort(portInput)">
            {{ t('common.save') }}
          </button>
        </div>
      </label>
      <label class="check">
        <input type="checkbox" :checked="service.autostart" @change="setAutostart(($event.target as HTMLInputElement).checked)" />
        <span>{{ t('services.autostart') }}</span>
      </label>
      <button class="btn small" @click="openConfig">{{ t('common.config') }}</button>
    </div>
  </article>
</template>

<style scoped>
.svc {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: 10px 16px;
  padding: 14px 16px;
  align-items: center;
}

.svc-title {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.ver {
  color: var(--ink-2);
}

.crash {
  color: var(--danger);
}

.conflict,
.svc-detail {
  grid-column: 1 / -1;
}

.conflict {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: 8px;
  background: var(--warn-soft);
  color: var(--warn);
  font-weight: 500;
}

.svc-detail {
  display: flex;
  align-items: flex-end;
  gap: 20px;
  flex-wrap: wrap;
  padding-top: 12px;
  border-top: 1px solid var(--line);
}

.port .input {
  width: 110px;
}

.svc-detail > .btn {
  margin-left: auto;
}
</style>
