<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { api, type Credentials, type ServiceInfo } from '../api';
import { toast, toastError } from '../store';
import AppIcon from './AppIcon.vue';

const props = defineProps<{ service: ServiceInfo }>();
const { t } = useI18n();
const creds = ref<Credentials | null>(null);
const loaded = ref(false);
const reveal = ref(false);

const SCHEMES: Record<string, { scheme: string; db: string }> = {
  mysql: { scheme: 'mysql', db: '' },
  mariadb: { scheme: 'mysql', db: '' },
  postgresql: { scheme: 'postgresql', db: '/postgres' },
};

function connectionUrl(password: string) {
  const c = creds.value;
  const s = SCHEMES[props.service.name];
  if (!c || !s) return '';
  return `${s.scheme}://${c.user}:${password}@${c.host}:${c.port ?? ''}${s.db}`;
}

const url = computed(() =>
  creds.value ? connectionUrl(reveal.value ? encodeURIComponent(creds.value.password) : '••••••') : '',
);

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast(t('services.copied'));
  } catch (e) {
    toastError(e);
  }
}

const copyUrl = () => creds.value && copy(connectionUrl(encodeURIComponent(creds.value.password)));

onMounted(async () => {
  try {
    creds.value = await api.getCredentials(props.service.id);
  } catch {
    creds.value = null; // not initialised yet
  } finally {
    loaded.value = true;
  }
});
</script>

<template>
  <div class="creds">
    <div v-if="!loaded" class="skeleton line" />
    <p v-else-if="!creds" class="hint">{{ t('services.notInitialized') }}</p>
    <template v-else>
      <div class="item">
        <span class="k">{{ t('services.host') }}</span>
        <span class="v mono">{{ creds.host }}:{{ creds.port }}</span>
      </div>
      <div class="item">
        <span class="k">{{ t('services.user') }}</span>
        <span class="v mono">{{ creds.user }}</span>
        <button class="btn sm ghost icon-only" :title="t('services.copy')" :aria-label="t('services.copy')" @click="copy(creds.user)">
          <AppIcon name="copy" :size="16" />
        </button>
      </div>
      <div class="item">
        <span class="k">{{ t('services.password') }}</span>
        <span class="v mono">{{ reveal ? creds.password : '••••••••••••' }}</span>
        <button
          class="btn sm ghost icon-only"
          :aria-pressed="reveal"
          :title="reveal ? t('services.hide') : t('services.show')"
          :aria-label="reveal ? t('services.hide') : t('services.show')"
          @click="reveal = !reveal"
        >
          <AppIcon :name="reveal ? 'eyeOff' : 'eye'" :size="16" />
        </button>
        <button class="btn sm ghost icon-only" :title="t('services.copy')" :aria-label="t('services.copy')" @click="copy(creds.password)">
          <AppIcon name="copy" :size="16" />
        </button>
      </div>
      <div v-if="url" class="item">
        <span class="k">{{ t('services.connection') }}</span>
        <span class="v mono url">{{ url }}</span>
        <button class="btn sm ghost icon-only" :title="t('services.copy')" :aria-label="t('services.copy')" @click="copyUrl">
          <AppIcon name="copy" :size="16" />
        </button>
      </div>
      <p class="hint store" :class="{ warn: creds.storage === 'file' }">
        <AppIcon :name="creds.storage === 'file' ? 'alert' : 'shield'" :size="16" />
        {{ creds.storage === 'file' ? t('services.storedFile') : t('services.storedKeychain') }}
      </p>
    </template>
  </div>
</template>

<style scoped>
.creds {
  display: grid;
  gap: 4px;
  padding: 10px 14px;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--surface-2);
}

.item {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 44px;
}

.item + .item {
  border-top: 1px dashed var(--line);
}

.k {
  flex: none;
  width: 150px;
  color: var(--ink-2);
  font-weight: 600;
}

.v {
  flex: 1;
  min-width: 0;
}

.url {
  overflow-wrap: anywhere;
}

.store {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 8px;
}

.warn {
  color: var(--warn);
}

.line {
  height: 20px;
  margin: 10px 0;
}
</style>
