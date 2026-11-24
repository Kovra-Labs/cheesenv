<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { api, type LogFile } from '../api';
import { attempt, toastError } from '../store';

const MAX_LINES = 5000;
const POLL_MS = 1000;

const { t } = useI18n();
const files = ref<LogFile[]>([]);
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
  if (l.startsWith('[stackyard')) return 'meta';
  return '';
}

async function loadFiles() {
  files.value = await api.listLogs();
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
      viewer.value?.scrollTo({ top: viewer.value.scrollHeight });
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
        <button class="btn" @click="loadFiles">{{ t('common.refresh') }}</button>
        <button class="btn" @click="attempt(() => api.openFolder('logs'))">{{ t('common.folder') }}</button>
      </div>
    </header>

    <div v-if="!files.length" class="card empty">{{ t('logs.empty') }}</div>
    <div v-else class="logs">
      <nav class="card files" :aria-label="t('logs.title')">
        <button
          v-for="f in files"
          :key="f.name"
          class="file mono"
          :class="{ active: f.name === selected }"
          :aria-current="f.name === selected"
          @click="selected = f.name"
        >
          {{ f.name }}
        </button>
      </nav>

      <section class="card view">
        <div class="row view-bar">
          <label class="sr-only" for="log-filter">{{ t('logs.filter') }}</label>
          <input id="log-filter" v-model="filter" class="input" type="search" :placeholder="t('logs.filter')" />
          <label class="check"><input v-model="follow" type="checkbox" /> {{ t('logs.follow') }}</label>
          <button class="btn small danger" @click="clearDialog?.showModal()">{{ t('logs.clear') }}</button>
        </div>
        <pre ref="viewer" class="out mono" tabindex="0"><template v-for="(l, i) in shown" :key="i"><span :class="lineClass(l)">{{ l }}</span>
</template><span v-if="!shown.length" class="hint">{{ t('logs.noLines') }}</span></pre>
      </section>
    </div>

    <dialog ref="clearDialog" class="card dialog">
      <form class="dialog-body" @submit.prevent="clearLog">
        <h2>{{ t('logs.confirmClear', { name: selected ?? '' }) }}</h2>
        <div class="row end">
          <button type="button" class="btn" @click="clearDialog?.close()">{{ t('common.cancel') }}</button>
          <button type="submit" class="btn primary">{{ t('logs.clear') }}</button>
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
}

.logs {
  display: grid;
  grid-template-columns: 220px 1fr;
  gap: 12px;
  min-height: 0;
}

.files {
  display: grid;
  align-content: start;
  gap: 2px;
  padding: 6px;
  overflow-y: auto;
}

.file {
  padding: 6px 8px;
  border: 0;
  border-radius: 6px;
  background: none;
  color: var(--ink-2);
  text-align: left;
  cursor: pointer;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file:hover {
  background: var(--surface-2);
}

.file.active {
  background: var(--accent-soft);
  color: var(--accent);
}

.view {
  display: grid;
  grid-template-rows: auto 1fr;
  min-height: 0;
  overflow: hidden;
}

.view-bar {
  padding: 8px;
  border-bottom: 1px solid var(--line);
}

.view-bar .input {
  flex: 1;
}

.out {
  margin: 0;
  padding: 10px 12px;
  overflow: auto;
  min-height: 260px;
  white-space: pre-wrap;
  word-break: break-all;
  line-height: 1.55;
}

.err {
  color: var(--danger);
}

.warn {
  color: var(--warn);
}

.meta {
  color: var(--accent);
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
