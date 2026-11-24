<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { api, type Credentials, type ServiceInfo } from '../api';
import { toast, toastError } from '../store';

const props = defineProps<{ service: ServiceInfo }>();
const { t } = useI18n();
const creds = ref<Credentials | null>(null);
const reveal = ref(false);

const SCHEMES: Record<string, { scheme: string; db: string }> = {
  mysql: { scheme: 'mysql', db: '' },
  mariadb: { scheme: 'mysql', db: '' },
  postgresql: { scheme: 'postgresql', db: '/postgres' },
};

const url = computed(() => {
  const c = creds.value;
  const s = SCHEMES[props.service.name];
  if (!c || !s) return '';
  const pw = reveal.value ? encodeURIComponent(c.password) : '••••••';
  return `${s.scheme}://${c.user}:${pw}@${c.host}:${c.port ?? ''}${s.db}`;
});

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast(t('services.copied'));
  } catch (e) {
    toastError(e);
  }
}

function copyUrl() {
  const c = creds.value;
  const s = SCHEMES[props.service.name];
  if (c && s) copy(`${s.scheme}://${c.user}:${encodeURIComponent(c.password)}@${c.host}:${c.port ?? ''}${s.db}`);
}

onMounted(async () => {
  try {
    creds.value = await api.getCredentials(props.service.id);
  } catch {
    creds.value = null; // not initialised yet
  }
});
</script>

<template>
  <div class="creds">
    <p v-if="!creds" class="hint">{{ t('services.notInitialized') }}</p>
    <template v-else>
      <dl>
        <dt>{{ t('services.host') }}</dt>
        <dd class="mono">{{ creds.host }}:{{ creds.port }}</dd>
        <dt>{{ t('services.user') }}</dt>
        <dd class="mono">
          {{ creds.user }}
          <button class="btn small" @click="copy(creds.user)">{{ t('services.copy') }}</button>
        </dd>
        <dt>{{ t('services.password') }}</dt>
        <dd class="mono">
          <span>{{ reveal ? creds.password : '••••••••••••' }}</span>
          <button class="btn small" :aria-pressed="reveal" @click="reveal = !reveal">
            {{ reveal ? t('services.hide') : t('services.show') }}
          </button>
          <button class="btn small" @click="copy(creds.password)">{{ t('services.copy') }}</button>
        </dd>
        <template v-if="url">
          <dt>{{ t('services.connection') }}</dt>
          <dd class="mono">
            <span class="url">{{ url }}</span>
            <button class="btn small" @click="copyUrl">{{ t('services.copy') }}</button>
          </dd>
        </template>
      </dl>
      <p class="hint" :class="{ warn: creds.storage === 'file' }">
        {{ creds.storage === 'file' ? t('services.storedFile') : t('services.storedKeychain') }}
      </p>
    </template>
  </div>
</template>

<style scoped>
.creds {
  display: grid;
  gap: 10px;
  padding: 12px;
  border-radius: 8px;
  background: var(--surface-2);
}

dl {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 8px 16px;
  margin: 0;
  align-items: center;
}

dt {
  font-weight: 600;
}

dd {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  min-width: 0;
  flex-wrap: wrap;
}

.url {
  overflow-wrap: anywhere;
}

.warn {
  color: var(--warn);
}
</style>
