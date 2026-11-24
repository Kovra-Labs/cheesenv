import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type ServiceStatus = 'stopped' | 'starting' | 'running' | 'stopping' | 'crashed';

export interface AppInfo {
  version: string;
  platform: string;
  data_dir: string;
}

export interface Settings {
  language: 'id' | 'en';
  theme: 'system' | 'light' | 'dark';
  data_dir: string;
  registry_url: string;
  auto_restart: boolean;
  check_updates: boolean;
}

export type SettingsUpdate = Partial<Pick<Settings, 'language' | 'theme' | 'data_dir' | 'registry_url' | 'auto_restart' | 'check_updates'>>;

export interface ModuleView {
  name: string;
  category: string;
  description: Record<string, string>;
  latest: string;
  license: string;
  homepage: string;
  installed: string[];
  update_available: boolean;
}

export interface IndexView {
  modules: ModuleView[];
  fetched_at: number;
  stale: boolean;
  platform: string;
}

export interface VersionView {
  version: string;
  available: boolean;
  installed: boolean;
}

export interface ManifestView {
  name: string;
  category: string;
  has_service: boolean;
  default_port: number | null;
  versions: VersionView[];
}

export interface ServiceInfo {
  id: string;
  name: string;
  version: string;
  category: string;
  status: ServiceStatus;
  pid: number | null;
  started_at: number | null;
  recent_crashes: number;
  port: number | null;
  autostart: boolean;
}

export interface ServiceSnapshot {
  id: string;
  status: ServiceStatus;
  pid: number | null;
  started_at: number | null;
  recent_crashes: number;
}

export interface InstallProgress {
  name: string;
  version: string;
  stage: 'download' | 'done' | 'failed';
  downloaded: number;
  total: number | null;
}

export interface UpdateInfo {
  version: string;
  current_version: string;
  notes: string | null;
  date: string | null;
}

export interface UpdateProgress {
  downloaded: number;
  total: number | null;
}

export interface PortCheck {
  port: number;
  free: boolean;
  suggestion: number | null;
}

export interface LogFile {
  name: string;
  size: number;
  modified: number;
}

export interface LogChunk {
  content: string;
  offset: number;
  reset: boolean;
}

/** Error shape produced by the Rust core (see src-tauri/src/error.rs). */
export interface AppError {
  code: string;
  port?: number;
  suggestion?: number | null;
  detail?: string;
}

export interface BulkFailure {
  id: string;
  error: AppError;
}

export function isAppError(e: unknown): e is AppError {
  return typeof e === 'object' && e !== null && typeof (e as AppError).code === 'string';
}

export const api = {
  appInfo: () => invoke<AppInfo>('get_app_info'),
  settings: () => invoke<Settings>('get_settings'),
  updateSettings: (update: SettingsUpdate) => invoke<Settings>('update_settings', { update }),

  registryIndex: (force = false) => invoke<IndexView>('registry_index', { force }),
  registryModule: (name: string) => invoke<ManifestView>('registry_module', { name }),
  installModule: (name: string, version: string) => invoke<void>('install_module', { name, version }),
  uninstallModule: (name: string, version: string, deleteData: boolean) =>
    invoke<void>('uninstall_module', { name, version, deleteData }),

  listServices: () => invoke<ServiceInfo[]>('list_services'),
  startService: (id: string) => invoke<void>('start_service', { id }),
  stopService: (id: string) => invoke<void>('stop_service', { id }),
  restartService: (id: string) => invoke<void>('restart_service', { id }),
  startAll: () => invoke<BulkFailure[]>('start_all'),
  stopAll: () => invoke<void>('stop_all'),
  setServiceOptions: (id: string, opts: { port?: number; autostart?: boolean }) =>
    invoke<ServiceInfo>('set_service_options', { id, port: opts.port ?? null, autostart: opts.autostart ?? null }),
  checkPort: (port: number) => invoke<PortCheck>('check_port', { port }),

  listLogs: () => invoke<LogFile[]>('list_logs'),
  readLog: (name: string, offset: number | null) => invoke<LogChunk>('read_log', { name, offset }),
  clearLog: (name: string) => invoke<void>('clear_log', { name }),

  openServiceUrl: (id: string) => invoke<void>('open_service_url', { id }),
  checkUpdate: () => invoke<UpdateInfo | null>('check_update'),
  installUpdate: () => invoke<void>('install_update'),

  openFolder: (kind: 'root' | 'www' | 'logs' | 'config' | 'module', id?: string) =>
    invoke<void>('open_folder', { kind, id: id ?? null }),
};

export const events = {
  serviceStatus: (cb: (s: ServiceSnapshot) => void): Promise<UnlistenFn> =>
    listen<ServiceSnapshot>('service-status', (e) => cb(e.payload)),
  servicesChanged: (cb: () => void): Promise<UnlistenFn> => listen('services-changed', () => cb()),
  installProgress: (cb: (p: InstallProgress) => void): Promise<UnlistenFn> =>
    listen<InstallProgress>('install-progress', (e) => cb(e.payload)),
  updateProgress: (cb: (p: UpdateProgress) => void): Promise<UnlistenFn> =>
    listen<UpdateProgress>('update-progress', (e) => cb(e.payload)),
};
