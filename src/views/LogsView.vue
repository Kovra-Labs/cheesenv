<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { api, type LogFile } from '../api';
import { attempt, toast, toastError } from '../store';
import { fmtBytes } from '../ui';
import AppIcon from '../components/AppIcon.vue';

const MAX_LINES = 5000;
const POLL_MS = 1000;

const { t, locale } = useI18n();
const files = ref<LogFile[]>([]);
const loaded = ref(false);
const selected = ref<string | null>(null);
const lines = ref<string[]>([]);
const filter = ref('');
const follow = ref(true);
const viewer = ref<HTMLElement | null>(null);
const clearDialog = ref<HTMLDialogElement | null>(null);

let offset: number | null = null;
let partial = '';
let timer: number | undefined;

const shown = computed(() => {
  const q = filter.value.trim().toLowerCase();
  return q ? lines.value.filter((l) => l.toLowerCase().includes(q)) : lines.value;
});

function lineClass(l: string) {
  if (/\b(error|fatal|emerg|crit|alert)\b/i.test(l)) return 'err';
  if (/\b(warn|warning|notice)\b/i.test(l)) return 'warn';
  if (l.startsWith('[cheesenv')) return 'meta';
  return '';
}

const fmtTime = (secs: number) =>
  new Date(secs * 1000).toLocaleString(locale.value, { dateStyle: 'short', timeStyle: 'short' });

async function loadFiles() {
  files.value = await api.listLogs();
  loaded.value = true;
  if (!selected.value && files.value.length) selected.value = files.value[0].name;
}

async function poll() {
  const name = selected.value;
  if (!name) return;
  try {
    const chunk = await api.readLog(name, offset);
    if (name !== selected.value) return;
    if (chunk.reset) {
      lines.value = [];
      partial = '';
    }
    offset = chunk.offset;
    if (!chunk.content) return;
    const parts = (partial + chunk.content).split('\n');
    partial = parts.pop() ?? '';
    lines.value.push(...parts);
    if (lines.value.length > MAX_LINES) lines.value.splice(0, lines.value.length - MAX_LINES);
    if (follow.value) {
      await nextTick();
      viewer.value?.scrollTo({ top: viewer.value.scrollHeight, behavior: 'smooth' });
    }
  } catch (e) {
    toastError(e);
    stop();
  }
}

function stop() {
  window.clearInterval(timer);
  timer = undefined;
}

function startPolling() {
  stop();
  offset = null;
  partial = '';
  lines.value = [];
  poll();
  timer = window.setInterval(poll, POLL_MS);
}

async function copyShown() {
  try {
    await navigator.clipboard.writeText(shown.value.join('\n'));
    toast(t('services.copied'));
  } catch (e) {
    toastError(e);
  }
}

async function clearLog() {
  clearDialog.value?.close();
  if (selected.value && (await attempt(() => api.clearLog(selected.value!)))) startPolling();
}

watch(selected, startPolling);
onMounted(() => loadFiles().catch(toastError));
onUnmounted(stop);
</script>

<template>
  <div class="page logs-page">
    <header class="page-head">
      <div>
        <h1>{{ t('logs.title') }}</h1>
        <p>{{ t('logs.subtitle') }}</p>
      </div>
      <div class="row">
        <button class="btn" @click="loadFiles().catch(toastError)"><AppIcon name="restart" />{{ t('common.refresh') }}</button>
        <button class="btn" @click="attempt(() => api.openFolder('logs'))"><AppIcon name="folder" />{{ t('common.folder') }}</button>
      </div>
    </header>

    <div v-if="loaded && !files.length" class="card empty">
      <span class="empty-icon"><AppIcon name="logs" :size="28" /></span>
      <strong>{{ t('logs.empty') }}</strong>
      <p>{{ t('logs.emptyHint') }}</p>
    </div>
    <div v-else class="logs">
      <nav class="card files" :aria-label="t('logs.title')">
        <button
          v-for="f in files"
          :key="f.name"
          class="file"
          :class="{ active: f.name === selected }"
          :aria-current="f.name === selected"
          :title="f.name"
          @click="selected = f.name"
        >
          <AppIcon name="logs" :size="18" />
          <span class="file-text">
            <span class="mono name">{{ f.name }}</span>
            <small>{{ fmtBytes(f.size) }} · {{ fmtTime(f.modified) }}</small>
          </span>
        </button>
      </nav>

      <section class="card view">
        <div class="view-bar">
          <label class="input-icon grow">
            <span class="sr-only">{{ t('logs.filter') }}</span>
            <AppIcon name="search" />
            <input v-model="filter" class="input" type="search" :placeholder="t('logs.filter')" />
          </label>
          <label class="check">
            <input v-model="follow" class="switch" type="checkbox" />
            <span>{{ t('logs.follow') }}</span>
          </label>
          <button class="btn icon-only" :title="t('logs.copy')" :aria-label="t('logs.copy')" :disabled="!shown.length" @click="copyShown">
            <AppIcon name="copy" />
          </button>
          <button class="btn danger" :disabled="!selected" @click="clearDialog?.showModal()">
            <AppIcon name="trash" />{{ t('logs.clear') }}
          </button>
        </div>
        <pre ref="viewer" class="out mono" tabindex="0"><template v-for="(l, i) in shown" :key="i"><span :class="lineClass(l)">{{ l }}</span>
</template><span v-if="!shown.length" class="none">{{ t('logs.noLines') }}</span></pre>
        <div class="view-foot hint mono">
          <span>{{ selected }}</span>
          <span>{{ t('logs.lines', { n: shown.length }) }}</span>
        </div>
      </section>
    </div>

    <dialog ref="clearDialog" class="dialog">
      <form class="dialog-body" @submit.prevent="clearLog">
        <div class="dialog-head">
          <span class="tile"><AppIcon name="trash" :size="22" /></span>
          <div>
            <h2>{{ t('logs.confirmClear', { name: selected ?? '' }) }}</h2>
            <p>{{ t('logs.confirmClearHint') }}</p>
          </div>
        </div>
        <div class="dialog-actions">
          <button type="button" class="btn" @click="clearDialog?.close()">{{ t('common.cancel') }}</button>
          <button type="submit" class="btn danger solid"><AppIcon name="trash" />{{ t('logs.clear') }}</button>
        </div>
      </form>
    </dialog>
  </div>
</template>

<style scoped>
.logs-page {
  max-width: none;
  height: 100%;
  grid-template-rows: auto 1fr;
  align-content: stretch;
}

.logs {
  display: grid;
  grid-template-columns: 260px 1fr;
  gap: 16px;
  min-height: 0;
}

.files {
  display: grid;
  align-content: start;
  gap: 4px;
  padding: 8px;
  overflow-y: auto;
}

.file {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 52px;
  padding: 8px 12px;
  border: 0;
  border-radius: 10px;
  background: none;
  color: var(--ink-2);
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition:
    background-color var(--dur) var(--ease),
    color var(--dur) var(--ease);
}

.file:hover {
  background: var(--surface-2);
  color: var(--ink);
}

.file.active {
  background: var(--primary-soft);
  color: var(--primary-text);
}

.file-text {
  display: grid;
  min-width: 0;
}

.name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}

.file small {
  color: var(--ink-3);
  font-size: 12px;
}

.view {
  display: grid;
  grid-template-rows: auto 1fr auto;
  min-height: 0;
  overflow: hidden;
}

.view-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px;
  border-bottom: 1px solid var(--line);
}

.grow {
  flex: 1;
}

.out {
  margin: 0;
  padding: 14px 16px;
  overflow: auto;
  min-height: 280px;
  background: var(--code-bg);
  color: var(--code-ink);
  white-space: pre-wrap;
  word-break: break-all;
  line-height: 1.65;
}

.out .err {
  color: #f87171;
}

.out .warn {
  color: #fb923c;
}

.out .meta {
  color: #f5cd3d;
}

.none {
  color: #7f8ea6;
}

.view-foot {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 14px;
  border-top: 1px solid var(--line);
  font-size: 12px;
}
</style>
