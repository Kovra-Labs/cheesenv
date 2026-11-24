<script setup lang="ts">
import { onMounted, onUnmounted, reactive, ref } from 'vue';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { useI18n } from 'vue-i18n';
import { api, events, type Settings } from '../api';
import { applySettings, attempt, store, toast, toastError } from '../store';

const { t } = useI18n();
const s = store.settings!;
const form = reactive({ data_dir: s.data_dir, registry_url: s.registry_url });
const saving = ref(false);

/** Language, theme and auto-restart apply immediately. */
async function patch(update: Partial<Settings>) {
  await attempt(async () => applySettings(await api.updateSettings(update)));
}

const checking = ref(false);
const installing = ref(false);
const progress = ref('');
let unlisten: UnlistenFn | undefined;

async function checkUpdate() {
  checking.value = true;
  try {
    store.update = await api.checkUpdate();
    if (!store.update) toast(t('update.upToDate'));
  } catch (e) {
    toastError(e);
  } finally {
    checking.value = false;
  }
}

async function installUpdate() {
  installing.value = true;
  progress.value = '';
  try {
    await api.installUpdate(); // the app relaunches on success
  } catch (e) {
    toastError(e);
    store.update = null;
    installing.value = false;
  }
}

onMounted(async () => {
  unlisten = await events.updateProgress((p) => {
    const mb = (n: number) => (n / 1048576).toFixed(1);
    progress.value = p.total ? `${mb(p.downloaded)} / ${mb(p.total)} MB` : `${mb(p.downloaded)} MB`;
  });
});
onUnmounted(() => unlisten?.());

async function saveAdvanced() {
  saving.value = true;
  const ok = await attempt(async () => {
    applySettings(await api.updateSettings({ data_dir: form.data_dir.trim(), registry_url: form.registry_url.trim() }));
    store.info = await api.appInfo();
  });
  saving.value = false;
  if (ok) toast(t('settings.saved'));
}
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1>{{ t('settings.title') }}</h1>
        <p>{{ t('settings.subtitle') }}</p>
      </div>
    </header>

    <section class="card group">
      <label class="field">
        <span>{{ t('settings.language') }}</span>
        <select
          class="select"
          :value="store.settings?.language"
          @change="patch({ language: ($event.target as HTMLSelectElement).value as Settings['language'] })"
        >
          <option value="id">Bahasa Indonesia</option>
          <option value="en">English</option>
        </select>
      </label>
      <label class="field">
        <span>{{ t('settings.theme') }}</span>
        <select
          class="select"
          :value="store.settings?.theme"
          @change="patch({ theme: ($event.target as HTMLSelectElement).value as Settings['theme'] })"
        >
          <option v-for="th in ['system', 'light', 'dark']" :key="th" :value="th">{{ t(`settings.themes.${th}`) }}</option>
        </select>
      </label>
      <label class="check wide">
        <input
          type="checkbox"
          :checked="store.settings?.auto_restart"
          @change="patch({ auto_restart: ($event.target as HTMLInputElement).checked })"
        />
        <span>{{ t('settings.autoRestart') }}</span>
      </label>
    </section>

    <form class="card group" @submit.prevent="saveAdvanced">
      <label class="field wide">
        <span>{{ t('settings.dataDir') }}</span>
        <input v-model="form.data_dir" class="input mono" required spellcheck="false" />
        <small class="hint">{{ t('settings.dataDirHint') }}</small>
      </label>
      <label class="field wide">
        <span>{{ t('settings.registry') }}</span>
        <input v-model="form.registry_url" class="input mono" type="url" required spellcheck="false" />
        <small class="hint">{{ t('settings.registryHint') }}</small>
      </label>
      <div class="wide row">
        <button class="btn primary" type="submit" :disabled="saving">{{ t('common.save') }}</button>
        <button class="btn" type="button" @click="attempt(() => api.openFolder('root'))">{{ t('common.folder') }}</button>
      </div>
    </form>

    <section class="card group">
      <h2 class="wide">{{ t('update.title') }}</h2>
      <p class="wide hint">{{ t('settings.version', { v: store.info?.version ?? '', p: store.info?.platform ?? '' }) }}</p>
      <div v-if="store.update" class="wide update-box">
        <strong>{{ t('update.newVersion', { v: store.update.version }) }}</strong>
        <p v-if="store.update.notes" class="notes">{{ store.update.notes }}</p>
        <p class="hint">{{ t('update.note') }}</p>
        <div class="row">
          <button class="btn primary" :disabled="installing" @click="installUpdate">{{ t('update.install') }}</button>
          <span v-if="installing" class="hint mono" role="status">{{ t('update.installing', { p: progress }) }}</span>
        </div>
      </div>
      <div class="wide row">
        <button class="btn" :disabled="checking || installing" @click="checkUpdate">
          {{ checking ? t('update.checking') : t('update.check') }}
        </button>
      </div>
      <label class="check wide">
        <input
          type="checkbox"
          :checked="store.settings?.check_updates"
          @change="patch({ check_updates: ($event.target as HTMLInputElement).checked })"
        />
        <span>{{ t('update.auto') }}</span>
      </label>
    </section>

    <section v-if="store.info" class="card group">
      <h2 class="wide">{{ t('settings.about') }}</h2>
      <p class="wide hint">
        Stackyard · {{ t('settings.version', { v: store.info.version, p: store.info.platform }) }} · MIT
      </p>
    </section>
  </div>
</template>

<style scoped>
.group {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 16px 20px;
  padding: 18px;
}

.wide {
  grid-column: 1 / -1;
}

.update-box {
  display: grid;
  gap: 10px;
  padding: 14px;
  border-radius: 8px;
  background: var(--accent-soft);
}

.notes {
  white-space: pre-wrap;
}
</style>
