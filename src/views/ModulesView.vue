<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { api, events, type IndexView, type InstallProgress, type ManifestView, type ModuleView } from '../api';
import { attempt, errorText, store, toast } from '../store';

const { t, locale } = useI18n();
const index = ref<IndexView | null>(null);
const loading = ref(false);
const loadError = ref('');
const query = ref('');
const category = ref('all');
const expanded = ref<string | null>(null);
const manifests = reactive<Record<string, ManifestView>>({});
const progress = reactive<Record<string, InstallProgress>>({});
const pending = reactive(new Set<string>());

const uninstallDialog = ref<HTMLDialogElement | null>(null);
const toRemove = ref<{ name: string; version: string } | null>(null);
const deleteData = ref(false);

let unlisten: UnlistenFn | undefined;

const categories = computed(() => ['all', ...new Set(index.value?.modules.map((m) => m.category) ?? [])]);
const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  return (index.value?.modules ?? []).filter(
    (m) =>
      (category.value === 'all' || m.category === category.value) &&
      (!q || m.name.includes(q) || describe(m).toLowerCase().includes(q)),
  );
});

function describe(m: ModuleView) {
  return m.description[locale.value] ?? m.description.en ?? '';
}

function categoryLabel(c: string) {
  if (c === 'all') return t('modules.all');
  const key = `category.${c}`;
  return t(key) === key ? c : t(key);
}

function fmtBytes(n: number) {
  return n > 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.round(n / 1024)} KB`;
}

function progressText(p: InstallProgress) {
  const pct = p.total ? ` ${Math.floor((p.downloaded / p.total) * 100)}%` : '';
  return t('modules.downloading', { p: `${fmtBytes(p.downloaded)}${pct}` });
}

async function load(force = false) {
  loading.value = true;
  loadError.value = '';
  try {
    index.value = await api.registryIndex(force);
    if (force) for (const k of Object.keys(manifests)) delete manifests[k];
  } catch (e) {
    loadError.value = `${t('modules.loadFailed')} ${errorText(e)}`;
  } finally {
    loading.value = false;
  }
}

async function toggle(name: string) {
  if (expanded.value === name) {
    expanded.value = null;
    return;
  }
  expanded.value = name;
  if (!manifests[name]) {
    await attempt(async () => (manifests[name] = await api.registryModule(name)));
  }
}

async function refreshModule(name: string) {
  await load();
  manifests[name] = await api.registryModule(name);
}

async function install(name: string, version: string) {
  const key = `${name}@${version}`;
  pending.add(key);
  try {
    await api.installModule(name, version);
    toast(t('modules.installedOk', { name, version }));
    await refreshModule(name);
  } catch (e) {
    toast(errorText(e), 'error');
  } finally {
    pending.delete(key);
    delete progress[key];
  }
}

function askUninstall(name: string, version: string) {
  toRemove.value = { name, version };
  deleteData.value = false;
  uninstallDialog.value?.showModal();
}

async function confirmUninstall() {
  const target = toRemove.value;
  uninstallDialog.value?.close();
  if (!target) return;
  const ok = await attempt(() => api.uninstallModule(target.name, target.version, deleteData.value));
  if (ok) {
    toast(t('modules.uninstalledOk', target));
    await refreshModule(target.name);
  }
}

onMounted(async () => {
  unlisten = await events.installProgress((p) => {
    if (p.stage === 'download') progress[`${p.name}@${p.version}`] = p;
  });
  await load();
});
onUnmounted(() => unlisten?.());
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1>{{ t('modules.title') }}</h1>
        <p>{{ t('modules.subtitle') }}</p>
      </div>
      <button class="btn" :disabled="loading" @click="load(true)">{{ t('common.refresh') }}</button>
    </header>

    <div class="row toolbar">
      <label class="sr-only" for="module-search">{{ t('modules.search') }}</label>
      <input id="module-search" v-model="query" class="input search" type="search" :placeholder="t('modules.search')" />
      <div class="row chips" role="group">
        <button
          v-for="c in categories"
          :key="c"
          class="btn small"
          :class="{ primary: category === c }"
          :aria-pressed="category === c"
          @click="category = c"
        >
          {{ categoryLabel(c) }}
        </button>
      </div>
    </div>

    <p v-if="index?.stale" class="badge warn">{{ t('modules.stale') }}</p>
    <p v-if="loadError" class="card empty" role="alert">{{ loadError }}</p>
    <p v-else-if="loading && !index" class="hint">{{ t('common.loading') }}</p>

    <section v-if="index" class="grid">
      <article v-for="m in filtered" :key="m.name" class="card mod" :class="{ open: expanded === m.name }">
        <button class="mod-head" :aria-expanded="expanded === m.name" @click="toggle(m.name)">
          <span class="mod-name">
            <h2>{{ m.name }}</h2>
            <span class="badge">{{ categoryLabel(m.category) }}</span>
            <span v-if="m.update_available" class="badge warn">{{ t('modules.updateAvailable') }}</span>
          </span>
          <span class="hint">{{ describe(m) }}</span>
          <span class="row hint meta">
            <span>{{ t('modules.latest', { v: m.latest }) }}</span>
            <span v-if="m.installed.length" class="badge ok">{{ t('modules.installed', { v: m.installed.join(', ') }) }}</span>
            <span v-if="m.license">{{ m.license }}</span>
          </span>
        </button>

        <div v-if="expanded === m.name" class="versions">
          <p v-if="!manifests[m.name]" class="hint">{{ t('common.loading') }}</p>
          <ul v-else>
            <li v-for="v in manifests[m.name].versions" :key="v.version">
              <span class="mono">{{ v.version }}</span>
              <span v-if="progress[`${m.name}@${v.version}`]" class="hint prog">
                <progress
                  :value="progress[`${m.name}@${v.version}`].downloaded"
                  :max="progress[`${m.name}@${v.version}`].total ?? undefined"
                />
                {{ progressText(progress[`${m.name}@${v.version}`]) }}
              </span>
              <span class="spacer" />
              <span v-if="!v.available" class="hint">{{ t('modules.notAvailable') }}</span>
              <button
                v-else-if="v.installed"
                class="btn small danger"
                :disabled="pending.has(`${m.name}@${v.version}`)"
                @click="askUninstall(m.name, v.version)"
              >
                {{ t('common.uninstall') }}
              </button>
              <button
                v-else
                class="btn small primary"
                :disabled="pending.has(`${m.name}@${v.version}`)"
                @click="install(m.name, v.version)"
              >
                {{ t('common.install') }}
              </button>
            </li>
          </ul>
        </div>
      </article>
      <p v-if="!filtered.length" class="card empty">{{ t('modules.empty') }}</p>
    </section>

    <p v-if="index && store.info" class="hint mono">
      {{ index.platform }} · {{ t('modules.fetchedAt', { t: new Date(index.fetched_at * 1000).toLocaleString(locale) }) }}
    </p>

    <dialog ref="uninstallDialog" class="card dialog" @close="toRemove = null">
      <form method="dialog" class="dialog-body" @submit.prevent="confirmUninstall">
        <h2>{{ t('modules.confirmUninstall', toRemove ?? { name: '', version: '' }) }}</h2>
        <label class="check">
          <input v-model="deleteData" type="checkbox" />
          <span>{{ t('modules.deleteData') }}</span>
        </label>
        <div class="row end">
          <button type="button" class="btn" @click="uninstallDialog?.close()">{{ t('common.cancel') }}</button>
          <button type="submit" class="btn primary">{{ t('common.uninstall') }}</button>
        </div>
      </form>
    </dialog>
  </div>
</template>

<style scoped>
.toolbar {
  justify-content: space-between;
}

.search {
  flex: 1 1 240px;
  max-width: 360px;
}

.grid {
  display: grid;
  gap: 10px;
}

.mod-head {
  display: grid;
  gap: 4px;
  width: 100%;
  padding: 14px 16px;
  border: 0;
  background: none;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
  border-radius: var(--radius);
}

.mod-head:hover {
  background: var(--surface-2);
}

.mod-name {
  display: flex;
  align-items: center;
  gap: 8px;
}

.meta {
  margin-top: 4px;
}

.versions {
  border-top: 1px solid var(--line);
  padding: 8px 16px 12px;
}

.versions ul {
  list-style: none;
  margin: 0;
  padding: 0;
}

.versions li {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 0;
}

.versions li + li {
  border-top: 1px dashed var(--line);
}

.spacer {
  flex: 1;
}

.prog {
  display: flex;
  align-items: center;
  gap: 8px;
}

progress {
  width: 140px;
  accent-color: var(--accent);
}

.dialog {
  padding: 0;
  max-width: 420px;
  color: var(--ink);
}

.dialog::backdrop {
  background: rgb(0 0 0 / 0.35);
}

.dialog-body {
  display: grid;
  gap: 16px;
  padding: 20px;
}

.end {
  justify-content: flex-end;
}
</style>
