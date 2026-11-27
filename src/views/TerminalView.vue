<script setup lang="ts">
import { nextTick, onActivated, onBeforeUnmount, onDeactivated, onMounted, reactive, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRoute, useRouter } from 'vue-router';
import { Channel } from '@tauri-apps/api/core';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import { api, events } from '../api';
import { errorText, toast } from '../store';
import AppIcon from '../components/AppIcon.vue';

// Kept alive by App.vue: shells keep running while other pages are open.
defineOptions({ name: 'TerminalView' });

interface Tab {
  key: number;
  title: string;
  /** Backend session id once the shell has started. */
  session: number | null;
  exited: boolean;
}

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const tabs = reactive<Tab[]>([]);
const active = ref<number | null>(null);
const stage = ref<HTMLElement | null>(null);
/** xterm instances live outside Vue's reactivity. */
const terms = new Map<number, { term: Terminal; fit: FitAddon }>();
/** Each tab's container element (template refs). */
const els = new Map<number, HTMLElement>();
/** Tabs requested while the page is not shown (kept alive, or still entering): opened on activation. */
const pending: (string | null)[] = [];
let isActive = false;
let seq = 0;
let unlisten: UnlistenFn | null = null;
let observer: ResizeObserver | null = null;

function cssVar(name: string, fallback: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

function fitActive() {
  const entry = active.value !== null ? terms.get(active.value) : undefined;
  if (entry && stage.value?.offsetWidth) entry.fit.fit();
}

/** Clipboard shortcuts that do not clash with the shell's Ctrl+C / Ctrl+V. */
function clipboardKeys(term: Terminal) {
  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== 'keydown' || !e.ctrlKey || !e.shiftKey) return true;
    if (e.code === 'KeyC') {
      const text = term.getSelection();
      if (text) navigator.clipboard.writeText(text).catch(() => {});
    } else if (e.code === 'KeyV') {
      navigator.clipboard
        .readText()
        .then((text) => term.paste(text))
        .catch(() => {});
    } else {
      return true;
    }
    e.preventDefault();
    return false;
  });
}

async function openTab(project: string | null) {
  const key = ++seq;
  const tab: Tab = { key, title: project ?? 'www', session: null, exited: false };
  tabs.push(tab);
  active.value = key;
  await nextTick();
  const el = els.get(key);
  if (!el) return;

  const term = new Terminal({
    cursorBlink: true,
    fontFamily: cssVar('--mono', 'monospace'),
    fontSize: 13.5,
    scrollback: 5000,
    theme: {
      background: cssVar('--code-bg', '#0d1524'),
      foreground: cssVar('--code-ink', '#d6e2f0'),
      cursor: cssVar('--primary', '#ecbb01'),
      selectionBackground: 'rgba(236, 187, 1, 0.3)',
    },
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(el);
  clipboardKeys(term);
  terms.set(key, { term, fit });
  fit.fit();

  const output = new Channel<ArrayBuffer>();
  output.onmessage = (buf) => term.write(new Uint8Array(buf));
  try {
    const id = await api.terminalOpen(term.cols, term.rows, project, output);
    const current = tabs.find((x) => x.key === key);
    if (!current) {
      // Closed while the shell was starting.
      api.terminalClose(id).catch(() => {});
      return;
    }
    current.session = id;
    term.onData((data) => api.terminalWrite(id, data).catch(() => {}));
    term.onResize(({ cols, rows }) => api.terminalResize(id, cols, rows).catch(() => {}));
    term.focus();
  } catch (e) {
    term.write(`\x1b[31m${errorText(e)}\x1b[0m\r\n`);
    toast(errorText(e), 'error');
  }
}

function closeTab(key: number) {
  const i = tabs.findIndex((x) => x.key === key);
  if (i < 0) return;
  const [tab] = tabs.splice(i, 1);
  if (tab.session !== null && !tab.exited) api.terminalClose(tab.session).catch(() => {});
  terms.get(key)?.term.dispose();
  terms.delete(key);
  if (active.value === key) active.value = tabs[Math.min(i, tabs.length - 1)]?.key ?? null;
}

function setEl(key: number, el: unknown) {
  if (el instanceof HTMLElement) els.set(key, el);
  else els.delete(key);
}

/** Opens a tab now when the page is visible, otherwise once it is activated. */
async function requestTab(project: string | null) {
  pending.push(project);
  if (isActive) await flushPending();
}

async function flushPending() {
  while (isActive && pending.length) await openTab(pending.shift() ?? null);
}

function select(key: number) {
  active.value = key;
}

watch(active, async (key) => {
  await nextTick();
  fitActive();
  if (key !== null) terms.get(key)?.term.focus();
});

/** `/terminal?project=<name>` (from the Projects page) opens a tab in that project. */
watch(
  () => [route.name, route.query.project] as const,
  ([name, project]) => {
    if (name !== 'terminal' || typeof project !== 'string') return;
    router.replace({ name: 'terminal' });
    requestTab(project);
  },
  { immediate: true },
);

onMounted(async () => {
  unlisten = await events.terminalExit(({ id, code }) => {
    const tab = tabs.find((x) => x.session === id);
    if (!tab) return;
    tab.exited = true;
    terms.get(tab.key)?.term.write(`\r\n\x1b[2m${t('terminal.exited', { code: code ?? '?' })}\x1b[0m\r\n`);
  });
  observer = new ResizeObserver(() => fitActive());
  if (stage.value) observer.observe(stage.value);
});

// Also runs on the first mount (the page is inside KeepAlive).
onActivated(async () => {
  isActive = true;
  await nextTick();
  if (!tabs.length && !pending.length) pending.push(null);
  await flushPending();
  fitActive();
  if (active.value !== null) terms.get(active.value)?.term.focus();
});

onDeactivated(() => {
  isActive = false;
});

onBeforeUnmount(() => {
  unlisten?.();
  observer?.disconnect();
  for (const tab of [...tabs]) closeTab(tab.key);
});
</script>

<template>
  <div class="page term-page">
    <header class="page-head">
      <div>
        <h1>{{ t('terminal.title') }}</h1>
        <p>{{ t('terminal.subtitle') }}</p>
      </div>
      <span class="hint">{{ t('terminal.keys') }}</span>
    </header>

    <div class="tabs" role="tablist" :aria-label="t('terminal.tabs')">
      <div v-for="tab in tabs" :key="tab.key" class="tab" :class="{ active: tab.key === active, exited: tab.exited }">
        <button role="tab" class="tab-btn" :aria-selected="tab.key === active" @click="select(tab.key)">
          <AppIcon name="terminal" :size="16" />
          <span class="mono">{{ tab.title }}</span>
        </button>
        <button class="tab-x" :aria-label="t('terminal.closeTab')" :title="t('terminal.closeTab')" @click="closeTab(tab.key)">
          <AppIcon name="x" :size="14" />
        </button>
      </div>
      <button class="btn sm ghost" @click="openTab(null)"><AppIcon name="terminal" :size="16" />{{ t('terminal.newTab') }}</button>
    </div>

    <div ref="stage" class="stage">
      <div
        v-for="tab in tabs"
        v-show="tab.key === active"
        :key="tab.key"
        :ref="(el) => setEl(tab.key, el)"
        class="term"
        role="tabpanel"
      />
      <div v-if="!tabs.length" class="empty-term">
        <p>{{ t('terminal.empty') }}</p>
        <button class="btn primary" @click="openTab(null)"><AppIcon name="terminal" />{{ t('terminal.newTab') }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.term-page {
  height: 100%;
  grid-template-rows: auto auto minmax(0, 1fr);
  align-content: stretch;
  gap: 16px;
  max-width: none;
  padding-bottom: 24px;
}

.tabs {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.tab {
  display: flex;
  align-items: center;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--surface);
  transition:
    background-color var(--dur) var(--ease),
    border-color var(--dur) var(--ease);
}

.tab.active {
  border-color: var(--primary);
  background: var(--primary-soft);
  color: var(--primary-text);
}

.tab.exited .mono {
  text-decoration: line-through;
  opacity: 0.7;
}

.tab-btn,
.tab-x {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 0;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
}

.tab-btn {
  height: 34px;
  padding: 0 4px 0 12px;
  font-weight: 600;
  font-size: 13.5px;
}

.tab-x {
  justify-content: center;
  width: 28px;
  height: 28px;
  margin-right: 3px;
  border-radius: 7px;
  color: var(--ink-2);
}

.tab-x:hover {
  background: var(--surface-2);
  color: var(--ink);
}

.stage {
  position: relative;
  min-height: 0;
  border-radius: var(--radius-lg);
  background: var(--code-bg);
  overflow: hidden;
  padding: 12px 6px 12px 14px;
}

.term {
  height: 100%;
}

.empty-term {
  display: grid;
  place-items: center;
  align-content: center;
  gap: 14px;
  height: 100%;
  color: var(--code-ink);
}
</style>
