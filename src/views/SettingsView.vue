<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue';
import { useRoute } from 'vue-router';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { useI18n } from 'vue-i18n';
import { api, events, type Settings } from '../api';
import { applySettings, attempt, store, toast, toastError } from '../store';
import AppIcon, { type IconName } from '../components/AppIcon.vue';

const { t } = useI18n();
const dataDir = ref(store.settings!.data_dir);
const saving = ref(false);
const dataDirDirty = computed(() => dataDir.value.trim() !== store.settings?.data_dir);
const route = useRoute();
const terminalSection = ref<HTMLElement | null>(null);

const isWindows = computed(() => store.info?.platform.startsWith('windows') ?? false);
const binDir = computed(() => store.info?.bin_dir ?? '');
/** Snippets quote the folder for their shell, so paths with spaces or quotes stay intact. */
const pathSnippets = computed(() => {
  const dir = binDir.value;
  if (isWindows.value) {
    const ps = dir.replace(/'/g, "''");
    return [
      {
        label: t('settings.pathWindows'),
        code: `[Environment]::SetEnvironmentVariable('Path', '${ps};' + [Environment]::GetEnvironmentVariable('Path', 'User'), 'User')`,
      },
    ];
  }
  const sh = dir.replace(/(["\\$`])/g, '\\$1');
  const fish = dir.replace(/(['\\])/g, '\\$1');
  return [
    { label: t('settings.pathUnix'), code: `export PATH="${sh}:$PATH"` },
    { label: t('settings.pathFish'), code: `fish_add_path '${fish}'` },
  ];
});

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast(t('services.copied'));
  } catch (e) {
    toastError(e);
  }
}

const languages: { value: Settings['language']; label: string }[] = [
  { value: 'id', label: 'Bahasa Indonesia' },
  { value: 'en', label: 'English' },
];
const themes: { value: Settings['theme']; icon: IconName }[] = [
  { value: 'system', icon: 'monitor' },
  { value: 'light', icon: 'sun' },
  { value: 'dark', icon: 'moon' },
];

/** Language, theme and toggles apply immediately. */
async function patch(update: Partial<Settings>) {
  await attempt(async () => applySettings(await api.updateSettings(update)));
}

const checking = ref(false);
const installing = ref(false);
const progress = ref('');
const progressPct = ref<number | null>(null);
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
  progressPct.value = null;
  try {
    await api.installUpdate(); // the app relaunches on success
  } catch (e) {
    toastError(e);
    store.update = null;
    installing.value = false;
  }
}

onMounted(async () => {
  if (route.hash === '#terminal') {
    await nextTick();
    terminalSection.value?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }
  unlisten = await events.updateProgress((p) => {
    const mb = (n: number) => (n / 1048576).toFixed(1);
    progress.value = p.total ? `${mb(p.downloaded)} / ${mb(p.total)} MB` : `${mb(p.downloaded)} MB`;
    progressPct.value = p.total ? Math.min(100, (p.downloaded / p.total) * 100) : null;
  });
});
onUnmounted(() => unlisten?.());

async function saveDataDir() {
  saving.value = true;
  const ok = await attempt(async () => {
    applySettings(await api.updateSettings({ data_dir: dataDir.value.trim() }));
    store.info = await api.appInfo();
  });
  saving.value = false;
  if (ok) toast(t('settings.saved'));
}
</script>

<template>
  <div class="page narrow">
    <header class="page-head">
      <div>
        <h1>{{ t('settings.title') }}</h1>
        <p>{{ t('settings.subtitle') }}</p>
      </div>
    </header>

    <section class="card group">
      <div class="group-head">
        <span class="tile"><AppIcon name="globe" /></span>
        <div>
          <h2>{{ t('settings.general') }}</h2>
          <p class="hint">{{ t('settings.generalHint') }}</p>
        </div>
      </div>

      <div class="set-row">
        <div class="set-text">
          <strong id="lang-label">{{ t('settings.language') }}</strong>
        </div>
        <div class="seg" role="radiogroup" aria-labelledby="lang-label">
          <label v-for="l in languages" :key="l.value" class="seg-item" :class="{ active: store.settings?.language === l.value }">
            <input
              class="sr-only"
              type="radio"
              name="language"
              :value="l.value"
              :checked="store.settings?.language === l.value"
              @change="patch({ language: l.value })"
            />
            {{ l.label }}
          </label>
        </div>
      </div>

      <div class="set-row">
        <div class="set-text">
          <strong id="theme-label">{{ t('settings.theme') }}</strong>
        </div>
        <div class="seg" role="radiogroup" aria-labelledby="theme-label">
          <label v-for="th in themes" :key="th.value" class="seg-item" :class="{ active: store.settings?.theme === th.value }">
            <input
              class="sr-only"
              type="radio"
              name="theme"
              :value="th.value"
              :checked="store.settings?.theme === th.value"
              @change="patch({ theme: th.value })"
            />
            <AppIcon :name="th.icon" :size="17" />{{ t(`settings.themes.${th.value}`) }}
          </label>
        </div>
      </div>
    </section>

    <section class="card group">
      <div class="group-head">
        <span class="tile"><AppIcon name="server" /></span>
        <div>
          <h2>{{ t('settings.servicesTitle') }}</h2>
          <p class="hint">{{ t('settings.servicesHint') }}</p>
        </div>
      </div>
      <label class="set-row clickable">
        <div class="set-text">
          <strong>{{ t('settings.autoRestartTitle') }}</strong>
          <span class="hint">{{ t('settings.autoRestart') }}</span>
        </div>
        <input
          class="switch"
          type="checkbox"
          :checked="store.settings?.auto_restart"
          @change="patch({ auto_restart: ($event.target as HTMLInputElement).checked })"
        />
      </label>
    </section>

    <form class="card group" @submit.prevent="saveDataDir">
      <div class="group-head">
        <span class="tile"><AppIcon name="drive" /></span>
        <div>
          <h2>{{ t('settings.storage') }}</h2>
          <p class="hint">{{ t('settings.dataDirHint') }}</p>
        </div>
      </div>
      <label class="field">
        <span class="label">{{ t('settings.dataDir') }}</span>
        <div class="row nowrap">
          <input v-model="dataDir" class="input mono grow" required spellcheck="false" />
          <button class="btn icon-only" type="button" :title="t('common.folder')" :aria-label="t('common.folder')" @click="attempt(() => api.openFolder('root'))">
            <AppIcon name="folder" />
          </button>
        </div>
      </label>
      <div class="row end">
        <button v-if="dataDirDirty" class="btn" type="button" @click="dataDir = store.settings!.data_dir">{{ t('common.cancel') }}</button>
        <button class="btn primary" type="submit" :disabled="saving || !dataDirDirty"><AppIcon name="check" />{{ t('common.save') }}</button>
      </div>
    </form>

    <section class="card group">
      <div class="group-head">
        <span class="tile"><AppIcon name="download" /></span>
        <div>
          <h2>{{ t('update.title') }}</h2>
          <p class="hint">{{ t('settings.version', { v: store.info?.version ?? '', p: store.info?.platform ?? '' }) }}</p>
        </div>
        <span class="spacer" />
        <button class="btn" :disabled="checking || installing" @click="checkUpdate">
          <AppIcon name="restart" :class="{ spin: checking }" />{{ checking ? t('update.checking') : t('update.check') }}
        </button>
      </div>

      <Transition name="fade">
        <div v-if="store.update" class="update-box">
          <div class="row">
            <span class="badge info">{{ t('update.newVersion', { v: store.update.version }) }}</span>
          </div>
          <p v-if="store.update.notes" class="notes">{{ store.update.notes }}</p>
          <p class="hint">{{ t('update.note') }}</p>
          <div v-if="installing" class="prog" role="status">
            <div class="bar" :class="{ indeterminate: progressPct === null }">
              <span :style="{ width: `${progressPct ?? 0}%` }" />
            </div>
            <span class="hint mono">{{ t('update.installing', { p: progress }) }}</span>
          </div>
          <div class="row">
            <button class="btn primary lg" :disabled="installing" @click="installUpdate">
              <AppIcon :name="installing ? 'restart' : 'download'" :class="{ spin: installing }" />{{ t('update.install') }}
            </button>
          </div>
        </div>
      </Transition>

      <label class="set-row clickable">
        <div class="set-text">
          <strong>{{ t('update.autoTitle') }}</strong>
          <span class="hint">{{ t('update.auto') }}</span>
        </div>
        <input
          class="switch"
          type="checkbox"
          :checked="store.settings?.check_updates"
          @change="patch({ check_updates: ($event.target as HTMLInputElement).checked })"
        />
      </label>
    </section>

    <section id="terminal" ref="terminalSection" class="card group">
      <div class="group-head">
        <span class="tile"><AppIcon name="terminal" /></span>
        <div>
          <h2>{{ t('settings.terminal') }}</h2>
          <p class="hint">{{ t('settings.terminalHint') }}</p>
        </div>
      </div>
      <div class="field">
        <span class="label">{{ t('settings.binDir') }}</span>
        <div class="code-row">
          <code>{{ binDir }}</code>
          <button class="btn sm ghost icon-only" :title="t('services.copy')" :aria-label="t('services.copy')" @click="copy(binDir)">
            <AppIcon name="copy" :size="16" />
          </button>
        </div>
      </div>
      <div v-for="s in pathSnippets" :key="s.code" class="field">
        <span class="hint">{{ s.label }}</span>
        <div class="code-row">
          <code>{{ s.code }}</code>
          <button class="btn sm ghost icon-only" :title="t('services.copy')" :aria-label="t('services.copy')" @click="copy(s.code)">
            <AppIcon name="copy" :size="16" />
          </button>
        </div>
      </div>
    </section>

    <section v-if="store.info" class="card about">
      <svg viewBox="0 0 512 512" aria-hidden="true">
        <rect width="512" height="512" rx="128" fill="#2196f3" />
        <g fill="none" stroke="#fff" stroke-width="34" stroke-linejoin="round">
          <path d="M256 104 408 180 256 256 104 180Z" fill="#fff" />
          <path d="M104 256l152 76 152-76" />
          <path d="M104 332l152 76 152-76" />
        </g>
      </svg>
      <div>
        <h2>Stackyard</h2>
        <p class="hint">{{ t('settings.version', { v: store.info.version, p: store.info.platform }) }} · MIT</p>
        <p class="hint">{{ t('settings.aboutText') }}</p>
      </div>
    </section>
  </div>
</template>

<style scoped>
.narrow {
  max-width: 860px;
}

.group {
  display: grid;
  gap: 18px;
  padding: 22px 24px;
}

.group-head {
  display: flex;
  align-items: center;
  gap: 14px;
}

.group-head p {
  margin-top: 2px;
}

.set-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  padding-top: 18px;
  border-top: 1px solid var(--line);
}

.set-row.clickable {
  cursor: pointer;
}

.set-text {
  display: grid;
  gap: 2px;
}

.seg {
  display: inline-flex;
  gap: 4px;
  padding: 4px;
  border-radius: 12px;
  background: var(--surface-2);
}

.seg-item {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  height: 38px;
  padding: 0 16px;
  border-radius: 9px;
  color: var(--ink-2);
  font-weight: 600;
  cursor: pointer;
  transition:
    background-color var(--dur) var(--ease),
    color var(--dur) var(--ease),
    box-shadow var(--dur) var(--ease);
}

.seg-item:hover {
  color: var(--ink);
}

.seg-item.active {
  background: var(--surface);
  color: var(--primary-text);
  box-shadow: var(--shadow-md);
}

.seg-item:focus-within {
  box-shadow: 0 0 0 3px var(--ring);
}

.nowrap {
  flex-wrap: nowrap;
}

.grow {
  flex: 1;
}

.end {
  justify-content: flex-end;
}

.code-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 6px 6px 14px;
  border-radius: var(--radius-sm);
  background: var(--code-bg);
  color: var(--code-ink);
}

.code-row code {
  flex: 1;
  min-width: 0;
  overflow-wrap: anywhere;
}

.code-row .btn {
  --btn-ink: var(--code-ink);
  --btn-bg-hover: rgb(255 255 255 / 0.08);
}

.update-box {
  display: grid;
  gap: 12px;
  padding: 16px;
  border-radius: var(--radius);
  background: var(--primary-soft);
}

.notes {
  white-space: pre-wrap;
}

.prog {
  display: grid;
  gap: 6px;
}

.about {
  display: flex;
  align-items: center;
  gap: 18px;
  padding: 22px 24px;
}

.about svg {
  width: 56px;
  height: 56px;
  flex: none;
}

.about > div {
  display: grid;
  gap: 2px;
}
</style>
