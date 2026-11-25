<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { api, type ServiceInfo } from '../api';
import { attempt, reloadServices, toast } from '../store';
import AppIcon from './AppIcon.vue';
import StatusBadge from './StatusBadge.vue';

/** A web panel (phpMyAdmin, pgAdmin) shown inside the card of the database it manages. */
const props = defineProps<{ panel: ServiceInfo; host: ServiceInfo; detailed?: boolean }>();
const { t } = useI18n();

const busy = ref(false);
const portInput = ref<number | null>(props.panel.port);
watch(
  () => props.panel.port,
  (p) => (portInput.value = p),
);

const active = computed(() => props.panel.status !== 'stopped' && props.panel.status !== 'crashed');
const hostRunning = computed(() => props.host.status === 'running');
const portDirty = computed(() => portInput.value !== props.panel.port);
/** The panel follows its database: its port changes only while both are stopped. */
const portLocked = computed(() => active.value || hostRunning.value);

async function run(fn: () => Promise<unknown>) {
  busy.value = true;
  try {
    await fn();
  } finally {
    busy.value = false;
  }
}

/** Starts the panel if needed and opens it in the browser. */
const open = () => run(() => attempt(() => api.openPanel(props.panel.id)));
const stop = () => run(() => attempt(() => api.stopService(props.panel.id)));

async function savePort() {
  const port = portInput.value;
  if (!port || port < 1 || port > 65535) {
    toast(t('errors.INVALID_INPUT'), 'error');
    return;
  }
  await run(async () => {
    if (!(await attempt(() => api.setServiceOptions(props.panel.id, { port })))) return;
    await reloadServices();
    toast(t('services.portSaved'));
    const check = await api.checkPort(port).catch(() => null);
    if (check && !check.free) toast(t('services.portBusy', { port }), 'error');
  });
}
</script>

<template>
  <div class="panel">
    <div class="panel-name">
      <AppIcon name="external" :size="18" />
      <strong>{{ panel.name }}</strong>
      <span class="badge mono">{{ panel.version }}</span>
      <StatusBadge v-if="active" :status="panel.status" />
    </div>

    <label v-if="detailed" class="row nowrap panel-port">
      <span class="label">{{ t('common.port') }}</span>
      <input
        v-model.number="portInput"
        class="input mono port"
        type="number"
        min="1"
        max="65535"
        :disabled="portLocked || busy"
        :title="portLocked ? t('services.stopPanelFirst', { name: host.name }) : undefined"
        @keyup.enter="portDirty && savePort()"
      />
      <Transition name="fade">
        <button v-if="portDirty" class="btn sm primary" :disabled="busy" @click.prevent="savePort">
          <AppIcon name="check" :size="16" />{{ t('common.save') }}
        </button>
      </Transition>
    </label>
    <span v-else-if="panel.port" class="hint">{{ t('common.port') }} <b class="mono">{{ panel.port }}</b></span>

    <div class="panel-actions">
      <button
        class="btn sm"
        :disabled="busy || !hostRunning"
        :title="hostRunning ? undefined : t('services.panelNeedsHost', { name: host.name })"
        @click="open"
      >
        <AppIcon :name="busy && !active ? 'restart' : 'external'" :size="16" :class="{ spin: busy && !active }" />
        {{ busy && !active ? t('services.panelStarting') : t('services.openPanel', { name: panel.name }) }}
      </button>
      <button v-if="active" class="btn sm danger" :disabled="busy || panel.status === 'stopping'" @click="stop">
        <AppIcon name="stop" :size="16" />{{ t('common.stop') }}
      </button>
    </div>
  </div>
  <p v-if="detailed" class="hint panel-hint">
    {{ t('services.panelHint', { target: panel.panel_for.join(' / ') }) }}
  </p>
</template>

<style scoped>
.panel {
  display: flex;
  align-items: center;
  gap: 12px 20px;
  flex-wrap: wrap;
  padding: 10px 14px;
  border-radius: var(--radius-sm);
  background: var(--surface-2);
}

.panel-name {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1 1 200px;
  min-width: 0;
  color: var(--ink-2);
}

.panel-name strong {
  color: var(--ink);
}

.panel-port {
  gap: 8px;
  align-items: center;
}

.port {
  width: 110px;
  height: 36px;
}

.panel-actions {
  display: flex;
  gap: 8px;
  margin-left: auto;
}

.panel-hint {
  font-size: 13px;
}
</style>
