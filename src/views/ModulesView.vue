<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { api, events, type IndexView, type InstallProgress, type ManifestView, type ModuleView } from '../api';
import { attempt, errorText, toast } from '../store';
import { categoryIcon, categoryLabel, fmtBytes } from '../ui';
import AppIcon from '../components/AppIcon.vue';
import CollapseTransition from '../components/CollapseTransition.vue';

const INSTALLED = 'installed';

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

/** The built-in panels (phpMyAdmin, pgAdmin) are installed and updated by the app, not from the hub. */
const modules = computed(() => {
  const all = index.value?.modules ?? [];
  const panels = new Set(all.flatMap((m) => m.companions ?? []));
  return all.filter((m) => !panels.has(m.name));
});
const categories = computed(() => ['all', ...new Set(modules.value.map((m) => m.category))]);
const installedCount = computed(() => modules.value.filter((m) => m.installed.length).length);
const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  const matches = (m: ModuleView) =>
    m.name.includes(q) || describe(m).toLowerCase().includes(q) || (m.companions ?? []).some((c) => c.includes(q));
  return modules.value.filter(
    (m) =>
      (category.value === 'all' ||
        (category.value === INSTALLED ? m.installed.length > 0 : m.category === category.value)) &&
      (!q || matches(m)),
  );
});

function describe(m: ModuleView) {
  return m.description[locale.value] ?? m.description.en ?? '';
}

function chipLabel(c: string) {
  return c === 'all' ? t('modules.all') : categoryLabel(c);
}

const key = (name: string, version: string) => `${name}@${version}`;

function progressText(p: InstallProgress) {
  if (p.stage === 'setup') return t('modules.settingUp');
  const pct = p.total ? ` · ${Math.floor((p.downloaded / p.total) * 100)}%` : '';
  return t('modules.downloading', { p: `${fmtBytes(p.downloaded)}${pct}` });
}

function progressPct(p: InstallProgress) {
  return p.stage === 'download' && p.total ? Math.min(100, (p.downloaded / p.total) * 100) : null;
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
  const k = key(name, version);
  pending.add(k);
  try {
    const outcome = await api.installModule(name, version);
    toast(
      outcome.companions.length
        ? t('modules.installedWith', { name, version, companions: outcome.companions.join(', ') })
        : t('modules.installedOk', { name, version }),
    );
    for (const err of outcome.companion_errors) toast(t('modules.companionFailed', { error: err }), 'error');
    await refreshModule(name);
    // Companions changed too: refresh their open version lists.
    for (const c of outcome.companions) if (manifests[c]) await attempt(() => refreshModule(c));
  } catch (e) {
    toast(errorText(e), 'error');
  } finally {
    pending.delete(k);
    delete progress[k];
  }
}

function askUninstall(name: string, version: string) {
  toRemove.value = { name, version };
  deleteData.value = false;
  uninstallDialog.value?.showModal();
}

async function setDefault(name: string, version: string, on: boolean) {
  const k = key(name, version);
  pending.add(k);
  const ok = await attempt(() => api.setRuntime(name, on ? version : null));
  if (ok) {
    toast(on ? t('modules.defaultSet', { name, version }) : t('modules.defaultUnset', { name }));
    await attempt(async () => (manifests[name] = await api.registryModule(name)));
  }
  pending.delete(k);
}

async function confirmUninstall() {
  const target = toRemove.value;
  uninstallDialog.value?.close();
  if (!target) return;
  const k = key(target.name, target.version);
  pending.add(k);
  const ok = await attempt(() => api.uninstallModule(target.name, target.version, deleteData.value));
  if (ok) {
    toast(t('modules.uninstalledOk', target));
    await attempt(() => refreshModule(target.name));
  }
  pending.delete(k);
}

onMounted(async () => {
  unlisten = await events.installProgress((p) => {
    if (p.stage === 'download' || p.stage === 'setup') progress[key(p.name, p.version)] = p;
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
      <button class="btn" :disabled="loading" @click="load(true)">
        <AppIcon name="restart" :class="{ spin: loading }" />{{ t('common.refresh') }}
      </button>
    </header>

    <div class="toolbar">
      <label class="input-icon search">
        <span class="sr-only">{{ t('modules.search') }}</span>
        <AppIcon name="search" />
        <input v-model="query" class="input" type="search" :placeholder="t('modules.search')" />
      </label>
      <div class="row" role="group" :aria-label="t('modules.categories')">
        <button
          v-for="c in categories"
          :key="c"
          class="chip"
          :class="{ active: category === c }"
          :aria-pressed="category === c"
          @click="category = c"
        >
          {{ chipLabel(c) }}
        </button>
        <button
          v-if="installedCount"
          class="chip"
          :class="{ active: category === INSTALLED }"
          :aria-pressed="category === INSTALLED"
          @click="category = INSTALLED"
        >
          <AppIcon name="check" :size="16" />{{ t('modules.installedFilter') }} <span class="n">{{ installedCount }}</span>
        </button>
      </div>
    </div>

    <Transition name="fade">
      <p v-if="index?.stale" class="alert"><AppIcon name="cloud" />{{ t('modules.stale') }}</p>
    </Transition>
    <div v-if="loadError" class="alert error" role="alert">
      <AppIcon name="alert" />
      <span class="spacer">{{ loadError }}</span>
      <button class="btn sm" @click="load(true)">{{ t('modules.retry') }}</button>
    </div>

    <section v-else-if="loading && !index" class="grid" aria-busy="true">
      <div v-for="i in 6" :key="i" class="card mod sk">
        <div class="mod-top">
          <span class="skeleton sk-tile" />
          <span class="skeleton sk-title" />
        </div>
        <span class="skeleton sk-line" />
        <span class="skeleton sk-line short" />
      </div>
    </section>

    <TransitionGroup v-if="index" tag="section" name="list" class="grid">
      <article v-for="m in filtered" :key="m.name" class="card mod" :class="{ open: expanded === m.name }">
        <div class="mod-top">
          <span class="tile"><AppIcon :name="categoryIcon(m.category)" :size="22" /></span>
          <div class="mod-name">
            <h2>{{ m.name }}</h2>
            <span class="hint">{{ categoryLabel(m.category) }}</span>
          </div>
          <span v-if="m.update_available" class="badge warn">{{ t('modules.updateAvailable') }}</span>
          <span v-else-if="m.installed.length" class="badge ok"><AppIcon name="check" :size="14" />{{ t('modules.isInstalled') }}</span>
        </div>
        <p class="desc">{{ describe(m) }}</p>
        <p v-for="c in m.companions ?? []" :key="c" class="companion">
          <AppIcon name="external" :size="16" />
          <span><b>{{ t('modules.includes', { name: c }) }}</b> · {{ t('modules.builtIn') }}</span>
        </p>
        <div class="meta">
          <span class="badge info mono">{{ t('modules.latest', { v: m.latest }) }}</span>
          <span v-if="m.installed.length" class="badge mono">{{ t('modules.installed', { v: m.installed.join(', ') }) }}</span>
          <span v-if="m.license" class="badge">{{ m.license }}</span>
          <button class="btn sm toggle" :class="{ primary: !m.installed.length && expanded !== m.name }" :aria-expanded="expanded === m.name" @click="toggle(m.name)">
            {{ expanded === m.name ? t('modules.hideVersions') : t('modules.showVersions') }}
            <AppIcon name="chevron" :size="16" class="chev" />
          </button>
        </div>

        <CollapseTransition>
          <div v-if="expanded === m.name" class="versions">
            <div v-if="!manifests[m.name]" class="skeleton sk-line" />
            <ul v-else>
              <li v-for="v in manifests[m.name].versions" :key="v.version">
                <div class="v-main">
                  <span class="mono v-name">{{ v.version }}</span>
                  <span v-if="v.default" class="badge info"><AppIcon name="terminal" :size="14" />{{ t('modules.isDefault') }}</span>
                  <span v-else-if="v.installed" class="badge ok"><AppIcon name="check" :size="14" />{{ t('modules.isInstalled') }}</span>
                  <span v-else-if="!v.available" class="hint">{{ t('modules.notAvailable') }}</span>
                  <span class="v-actions">
                  <template v-if="manifests[m.name].runtime && v.installed">
                    <button
                      v-if="v.default"
                      class="btn sm ghost"
                      :disabled="pending.has(key(m.name, v.version))"
                      @click="setDefault(m.name, v.version, false)"
                    >
                      {{ t('modules.unsetDefault') }}
                    </button>
                    <button v-else class="btn sm" :disabled="pending.has(key(m.name, v.version))" @click="setDefault(m.name, v.version, true)">
                      <AppIcon name="terminal" :size="16" />{{ t('modules.makeDefault') }}
                    </button>
                  </template>
                  <button
                    v-if="v.installed"
                    class="btn sm danger"
                    :disabled="pending.has(key(m.name, v.version))"
                    @click="askUninstall(m.name, v.version)"
                  >
                    <AppIcon name="trash" :size="16" />{{ t('common.uninstall') }}
                  </button>
                  <button
                    v-else-if="v.available"
                    class="btn sm primary"
                    :disabled="pending.has(key(m.name, v.version))"
                    @click="install(m.name, v.version)"
                  >
                    <AppIcon
                      :name="pending.has(key(m.name, v.version)) ? 'restart' : 'download'"
                      :size="16"
                      :class="{ spin: pending.has(key(m.name, v.version)) }"
                    />
                    {{ pending.has(key(m.name, v.version)) ? t('modules.installing') : t('common.install') }}
                  </button>
                  </span>
                </div>
                <CollapseTransition>
                  <div v-if="progress[key(m.name, v.version)]" class="prog">
                    <div class="bar" :class="{ indeterminate: progressPct(progress[key(m.name, v.version)]) === null }">
                      <span :style="{ width: `${progressPct(progress[key(m.name, v.version)]) ?? 0}%` }" />
                    </div>
                    <span class="hint mono">{{ progressText(progress[key(m.name, v.version)]) }}</span>
                  </div>
                </CollapseTransition>
              </li>
            </ul>
            <p v-if="manifests[m.name]?.runtime" class="hint runtime-hint">
              <AppIcon name="info" :size="16" />
              <span>{{ t('modules.runtimeHint') }} <RouterLink to="/settings#terminal">{{ t('modules.pathHelp') }}</RouterLink></span>
            </p>
          </div>
        </CollapseTransition>
      </article>
    </TransitionGroup>
    <div v-if="index && !filtered.length" class="card empty">
      <span class="empty-icon"><AppIcon name="search" :size="28" /></span>
      <p>{{ t('modules.empty') }}</p>
    </div>

    <p v-if="index" class="hint mono foot">
      {{ index.platform }} · {{ t('modules.fetchedAt', { t: new Date(index.fetched_at * 1000).toLocaleString(locale) }) }}
    </p>

    <dialog ref="uninstallDialog" class="dialog" @close="toRemove = null">
      <form method="dialog" class="dialog-body" @submit.prevent="confirmUninstall">
        <div class="dialog-head">
          <span class="tile"><AppIcon name="trash" :size="22" /></span>
          <div>
            <h2>{{ t('modules.confirmUninstall', toRemove ?? { name: '', version: '' }) }}</h2>
            <p>{{ t('modules.confirmUninstallHint') }}</p>
          </div>
        </div>
        <label class="check">
          <input v-model="deleteData" type="checkbox" />
          <span>{{ t('modules.deleteData') }}</span>
        </label>
        <div class="dialog-actions">
          <button type="button" class="btn" @click="uninstallDialog?.close()">{{ t('common.cancel') }}</button>
          <button type="submit" class="btn danger solid"><AppIcon name="trash" />{{ t('common.uninstall') }}</button>
        </div>
      </form>
    </dialog>
  </div>
</template>

<style scoped>
.toolbar {
  display: grid;
  gap: 14px;
}

.search {
  max-width: 440px;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
  gap: 16px;
  align-items: start;
}

.mod {
  display: grid;
  gap: 12px;
  padding: 20px;
}

.mod:hover {
  border-color: var(--line-strong);
  box-shadow: var(--shadow-md);
}

.mod.open {
  border-color: var(--primary);
  box-shadow: 0 0 0 3px var(--primary-soft);
}

.mod-top {
  display: flex;
  align-items: center;
  gap: 14px;
}

.mod-name {
  display: grid;
  flex: 1;
  min-width: 0;
}

.mod-name .hint {
  font-size: 12.5px;
}

.desc {
  color: var(--ink-2);
  min-height: 2lh;
}

.meta {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.toggle {
  margin-left: auto;
}

.chev {
  transition: transform var(--dur-slow) var(--ease);
}

.open .chev {
  transform: rotate(180deg);
}

.versions {
  border-top: 1px solid var(--line);
}

.versions ul {
  list-style: none;
  margin: 0;
  padding: 6px 0 0;
}

.versions li {
  padding: 10px 0;
}

.versions li + li {
  border-top: 1px dashed var(--line);
}

.v-main {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
  min-height: 36px;
}

.v-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
  margin-left: auto;
}

.v-name {
  font-size: 14px;
  font-weight: 600;
}

.companion {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  background: var(--surface-2);
  color: var(--ink-2);
  font-size: 13.5px;
}

.companion b {
  color: var(--ink);
}

.runtime-hint {
  display: flex;
  gap: 8px;
  padding: 10px 12px;
  border-radius: var(--radius-sm);
  background: var(--surface-2);
}

.runtime-hint .icon {
  flex: none;
  margin-top: 2px;
}

.prog {
  display: grid;
  gap: 6px;
  padding-top: 10px;
}

.foot {
  font-size: 12px;
}

.sk {
  pointer-events: none;
}

.sk-tile {
  width: 44px;
  height: 44px;
  border-radius: 12px;
}

.sk-title {
  width: 40%;
  height: 18px;
}

.sk-line {
  height: 14px;
}

.sk-line.short {
  width: 60%;
}
</style>
