import { Channel, invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type ServiceStatus = 'stopped' | 'starting' | 'running' | 'stopping' | 'crashed';

export interface AppInfo {
  version: string;
  platform: string;
  data_dir: string;
  /** Folder with the default runtimes' commands; users add it to PATH. */
  bin_dir: string;
}

export interface Settings {
  language: 'id' | 'en';
  theme: 'system' | 'light' | 'dark';
  accent: 'cheese' | 'blue' | 'teal' | 'violet';
  data_dir: string;
  auto_restart: boolean;
  check_updates: boolean;
  launch_at_login: boolean;
}

export type SettingsUpdate = Partial<
  Pick<Settings, 'language' | 'theme' | 'accent' | 'data_dir' | 'auto_restart' | 'check_updates' | 'launch_at_login'>
>;

export interface ModuleView {
  name: string;
  category: string;
  description: Record<string, string>;
  latest: string;
  license: string;
  homepage: string;
  installed: string[];
  update_available: boolean;
  /** Modules installed along with it (e.g. `phpmyadmin` for MySQL). */
  companions?: string[];
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
  /** Default version of a runtime (its commands are on PATH). */
  default: boolean;
}

export interface ManifestView {
  name: string;
  category: string;
  has_service: boolean;
  default_port: number | null;
  /** Node.js, Python, …: versions can be made the default on PATH. */
  runtime: boolean;
  /** Modules installed along with it (e.g. `phpmyadmin` for MySQL). */
  companions: string[];
  versions: VersionView[];
}

export interface InstallOutcome {
  /** Companions installed along with the module. */
  companions: string[];
  /** Companions that failed: `"<name>: <reason>"`. */
  companion_errors: string[];
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
  credentials_user: string | null;
  initialized: boolean;
  /** Modules this service is a web panel for (e.g. `mysql`, `mariadb` for phpMyAdmin). */
  panel_for: string[];
}

export interface Credentials {
  user: string;
  password: string;
  host: string;
  port: number | null;
  storage: 'keychain' | 'file';
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
  stage: 'download' | 'setup' | 'done' | 'failed';
  downloaded: number;
  total: number | null;
}

export interface UpdateInfo {
  version: string;
  current_version: string;
  notes: string | null;
  /** The same notes per language code (latest.json `notes_i18n`). */
  notes_i18n: Record<string, string>;
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

export interface Project {
  name: string;
  /** `<name>.test`, or null when the folder name is not a valid domain label. */
  domain: string | null;
  path: string;
  /** Folder served as web root (`public/` when present). */
  doc_root: string;
  /** PHP service id handling its `.php` files. */
  php: string | null;
  /** Chosen PHP release line (`8.3`); null = the newest PHP. */
  php_line: string | null;
  /** The chosen line is not installed (the newest PHP serves it meanwhile). */
  php_missing: boolean;
}

export interface WebServer {
  id: string;
  name: string;
  version: string;
  port: number | null;
  /** HTTPS port of the projects (nginx with HTTPS enabled). */
  https_port: number | null;
  running: boolean;
  /** Its config includes CheesEnv's vhosts. */
  vhosts: boolean;
  /** Linux nginx: may use ports 80/443; null when not applicable. */
  low_ports: boolean | null;
}

export interface PhpVersion {
  id: string;
  version: string;
  line: string;
  port: number | null;
  running: boolean;
}

export interface BrowserTrust {
  databases: number;
  imported: number;
  /** Browser profiles exist but NSS certutil is not installed (Linux). */
  tool_missing: boolean;
}

export interface ProjectsView {
  projects: Project[];
  servers: WebServer[];
  /** Installed PHP versions, newest (the default) first. */
  php: PhpVersion[];
  tls: { enabled: boolean; trusted: boolean };
  hosts_synced: boolean;
  hosts_file: string;
  /** Lines to add to the hosts file by hand. */
  hosts_lines: string[];
}

export interface TerminalExit {
  id: number;
  code: number | null;
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
  installModule: (name: string, version: string) => invoke<InstallOutcome>('install_module', { name, version }),
  uninstallModule: (name: string, version: string, deleteData: boolean) =>
    invoke<void>('uninstall_module', { name, version, deleteData }),
  setRuntime: (name: string, version: string | null) => invoke<void>('set_runtime', { name, version }),

  listServices: () => invoke<ServiceInfo[]>('list_services'),
  startService: (id: string) => invoke<void>('start_service', { id }),
  stopService: (id: string) => invoke<void>('stop_service', { id }),
  restartService: (id: string) => invoke<void>('restart_service', { id }),
  startAll: () => invoke<BulkFailure[]>('start_all'),
  stopAll: () => invoke<void>('stop_all'),
  setServiceOptions: (id: string, opts: { port?: number; autostart?: boolean }) =>
    invoke<ServiceInfo>('set_service_options', { id, port: opts.port ?? null, autostart: opts.autostart ?? null }),
  getCredentials: (id: string) => invoke<Credentials>('get_credentials', { id }),
  checkPort: (port: number) => invoke<PortCheck>('check_port', { port }),

  listLogs: () => invoke<LogFile[]>('list_logs'),
  readLog: (name: string, offset: number | null) => invoke<LogChunk>('read_log', { name, offset }),
  clearLog: (name: string) => invoke<void>('clear_log', { name }),

  openServiceUrl: (id: string) => invoke<void>('open_service_url', { id }),
  openPanel: (id: string) => invoke<void>('open_panel', { id }),
  checkUpdate: () => invoke<UpdateInfo | null>('check_update'),
  installUpdate: () => invoke<void>('install_update'),

  openFolder: (kind: 'root' | 'www' | 'logs' | 'config' | 'module' | 'project', id?: string) =>
    invoke<void>('open_folder', { kind, id: id ?? null }),

  /** Regenerates the web servers' vhosts from www/ and returns the projects. */
  syncProjects: () => invoke<ProjectsView>('sync_projects'),
  createProject: (name: string) => invoke<ProjectsView>('create_project', { name }),
  /** Writes the `.test` domains to the hosts file (administrator prompt). */
  syncHosts: () => invoke<void>('sync_hosts'),
  openProject: (name: string) => invoke<void>('open_project', { name }),
  /** PHP release line for a project (`8.3`), or null for the newest. */
  setProjectPhp: (name: string, line: string | null) => invoke<ProjectsView>('set_project_php', { name, line }),
  /** Creates and trusts the local CA (administrator prompt), then serves HTTPS. */
  enableHttps: () => invoke<BrowserTrust>('enable_https'),
  /** Linux: lets nginx use ports 80/443 (administrator prompt). */
  enableLowPorts: (id: string) => invoke<void>('enable_low_ports', { id }),

  /** Starts a shell; its output arrives as raw bytes on `onOutput`. */
  terminalOpen: (cols: number, rows: number, project: string | null, onOutput: Channel<ArrayBuffer>) =>
    invoke<number>('terminal_open', { cols, rows, project, onOutput }),
  terminalWrite: (id: number, data: string) => invoke<void>('terminal_write', { id, data }),
  terminalResize: (id: number, cols: number, rows: number) => invoke<void>('terminal_resize', { id, cols, rows }),
  terminalClose: (id: number) => invoke<void>('terminal_close', { id }),
};

export const events = {
  serviceStatus: (cb: (s: ServiceSnapshot) => void): Promise<UnlistenFn> =>
    listen<ServiceSnapshot>('service-status', (e) => cb(e.payload)),
  servicesChanged: (cb: () => void): Promise<UnlistenFn> => listen('services-changed', () => cb()),
  installProgress: (cb: (p: InstallProgress) => void): Promise<UnlistenFn> =>
    listen<InstallProgress>('install-progress', (e) => cb(e.payload)),
  /** Built-in panels (phpMyAdmin, pgAdmin) being installed/updated in the background. */
  panelsSetup: (cb: (p: InstallProgress) => void): Promise<UnlistenFn> =>
    listen<InstallProgress>('panels-setup', (e) => cb(e.payload)),
  terminalExit: (cb: (e: TerminalExit) => void): Promise<UnlistenFn> =>
    listen<TerminalExit>('terminal-exit', (e) => cb(e.payload)),
  updateProgress: (cb: (p: UpdateProgress) => void): Promise<UnlistenFn> =>
    listen<UpdateProgress>('update-progress', (e) => cb(e.payload)),
};
