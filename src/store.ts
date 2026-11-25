import { computed, reactive } from 'vue';
import { api, events, isAppError, type AppInfo, type InstallProgress, type ServiceInfo, type Settings, type UpdateInfo } from './api';
import { i18n } from './i18n';

export interface Toast {
  id: number;
  kind: 'info' | 'error';
  text: string;
}

export const store = reactive({
  ready: false,
  info: null as AppInfo | null,
  settings: null as Settings | null,
  services: [] as ServiceInfo[],
  toasts: [] as Toast[],
  update: null as UpdateInfo | null,
  /** A built-in panel being downloaded/updated in the background, if any. */
  panelSetup: null as InstallProgress | null,
});

let toastSeq = 0;

const toastTimers = new Map<number, number>();

/** Shows a toast; an identical one already on screen is kept (timer reset) instead of stacking. */
export function toast(text: string, kind: Toast['kind'] = 'info') {
  const existing = store.toasts.find((t) => t.text === text && t.kind === kind);
  const id = existing?.id ?? ++toastSeq;
  if (!existing) store.toasts.push({ id, kind, text });
  window.clearTimeout(toastTimers.get(id));
  toastTimers.set(id, window.setTimeout(() => dismissToast(id), kind === 'error' ? 7000 : 3500));
}

export function dismissToast(id: number) {
  window.clearTimeout(toastTimers.get(id));
  toastTimers.delete(id);
  const i = store.toasts.findIndex((t) => t.id === id);
  if (i >= 0) store.toasts.splice(i, 1);
}

/** Translates a core error code into a user-facing message. */
export function errorText(e: unknown): string {
  const t = i18n.global.t;
  if (isAppError(e)) {
    const key = `errors.${e.code}`;
    return i18n.global.te(key) ? t(key, { port: e.port ?? '' }) : t('errors.INTERNAL');
  }
  console.error(e);
  return t('errors.INTERNAL');
}

export function toastError(e: unknown) {
  toast(errorText(e), 'error');
}

/** Runs an action, reporting failures as a toast. Returns false on error. */
export async function attempt(fn: () => Promise<unknown>): Promise<boolean> {
  try {
    await fn();
    return true;
  } catch (e) {
    toastError(e);
    return false;
  }
}

export function applySettings(s: Settings) {
  store.settings = s;
  i18n.global.locale.value = s.language;
  document.documentElement.lang = s.language;
  document.documentElement.dataset.theme = s.theme;
}

export async function reloadServices() {
  store.services = await api.listServices();
}

/** Services that get a card of their own; built-in panels are shown inside their database's card. */
export const cardServices = computed(() => store.services.filter((s) => !s.panel_for.length));

export const runningCount = computed(() => store.services.filter((s) => s.status === 'running').length);

export async function initStore() {
  const [info, settings] = await Promise.all([api.appInfo(), api.settings()]);
  store.info = info;
  applySettings(settings);
  await reloadServices();

  await events.serviceStatus((snap) => {
    const svc = store.services.find((s) => s.id === snap.id);
    if (svc) Object.assign(svc, snap);
  });
  await events.panelsSetup((p) => {
    store.panelSetup = p.stage === 'done' || p.stage === 'failed' ? null : p;
  });
  await events.servicesChanged(async () => {
    store.info = await api.appInfo();
    await reloadServices();
  });
  store.ready = true;

  // Silent background check; failures (offline, no release yet) are ignored.
  if (settings.check_updates) {
    api
      .checkUpdate()
      .then((u) => {
        store.update = u;
        if (u) toast(i18n.global.t('update.available', { v: u.version }));
      })
      .catch(() => {});
  }
}
