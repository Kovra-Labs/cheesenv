<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRouter } from 'vue-router';
import { api, type ProjectsView } from '../api';
import { attempt, errorText, reloadServices, toast } from '../store';
import AppIcon from '../components/AppIcon.vue';

const { t } = useI18n();
const router = useRouter();
const view = ref<ProjectsView | null>(null);
const loading = ref(false);
const syncingHosts = ref(false);
const showManual = ref(false);
const createDialog = ref<HTMLDialogElement | null>(null);
const newName = ref('');
const creating = ref(false);

const enablingHttps = ref(false);
const browserHint = ref(false);
const grantingPorts = ref<string | null>(null);

const LABEL = /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/;
const nameValid = computed(() => LABEL.test(newName.value));

const servers = computed(() => view.value?.servers ?? []);
const readyServers = computed(() => servers.value.filter((s) => s.vhosts));
const outdated = computed(() => servers.value.filter((s) => !s.vhosts));
/** The server `open_project` uses: the first running one with vhost support. */
const serving = computed(() => readyServers.value.find((s) => s.running) ?? null);
const hasDomains = computed(() => view.value?.projects.some((p) => p.domain) ?? false);
const phps = computed(() => view.value?.php ?? []);
/** One choice per release line (the newest of each). */
const phpLines = computed(() => phps.value.filter((p, i, all) => all.findIndex((x) => x.line === p.line) === i));
/** PHP versions projects use that are stopped while a web server serves them. */
const stoppedPhp = computed(() => {
  if (!serving.value) return [];
  const used = new Set(view.value?.projects.map((p) => p.php));
  return phps.value.filter((p) => used.has(p.id) && !p.running);
});
/** Linux nginx still on 8080 because it may not bind ports 80/443. */
const lowPortServers = computed(() => readyServers.value.filter((s) => s.low_ports === false));
const tlsReady = computed(() => !!view.value?.tls.enabled && view.value.tls.trusted);

async function load() {
  loading.value = true;
  try {
    view.value = await api.syncProjects();
  } catch (e) {
    toast(errorText(e), 'error');
  } finally {
    loading.value = false;
  }
}

async function syncHosts() {
  syncingHosts.value = true;
  try {
    await api.syncHosts();
    toast(t('projects.hostsSynced'));
    await load();
  } catch (e) {
    toast(errorText(e), 'error');
  } finally {
    syncingHosts.value = false;
  }
}

function openCreate() {
  newName.value = '';
  createDialog.value?.showModal();
}

async function create() {
  if (!nameValid.value || creating.value) return;
  creating.value = true;
  try {
    view.value = await api.createProject(newName.value);
    createDialog.value?.close();
    toast(t('projects.created', { name: newName.value }));
    // The user just asked for this domain: register it right away (one admin prompt).
    if (!view.value.hosts_synced) await syncHosts();
  } catch (e) {
    toast(errorText(e), 'error');
  } finally {
    creating.value = false;
  }
}

async function setPhp(name: string, line: string | null) {
  try {
    view.value = await api.setProjectPhp(name, line);
    const php = view.value.projects.find((p) => p.name === name)?.php;
    const version = phps.value.find((p) => p.id === php)?.version ?? '';
    toast(t('projects.phpChanged', { name, version }));
    await reloadServices();
  } catch (e) {
    toast(errorText(e), 'error');
    await load();
  }
}

async function enableHttps() {
  enablingHttps.value = true;
  try {
    const trust = await api.enableHttps();
    browserHint.value = trust.tool_missing;
    toast(trust.imported ? t('projects.httpsBrowsersDone', { count: trust.imported }) : t('projects.httpsEnabled'));
    await load();
  } catch (e) {
    toast(errorText(e), 'error');
    await load();
  } finally {
    enablingHttps.value = false;
  }
}

async function enableLowPorts(id: string, name: string) {
  grantingPorts.value = id;
  try {
    await api.enableLowPorts(id);
    toast(t('projects.lowPortsDone', { name }));
    await reloadServices();
    await load();
  } catch (e) {
    toast(errorText(e), 'error');
  } finally {
    grantingPorts.value = null;
  }
}

async function startServer(id: string) {
  if (await attempt(() => api.startService(id))) {
    await reloadServices();
    await load();
  }
}

const openTerminal = (name: string) => router.push({ name: 'terminal', query: { project: name } });

const copyLines = () =>
  attempt(async () => {
    await navigator.clipboard.writeText((view.value?.hosts_lines ?? []).join('\n') + '\n');
    toast(t('services.copied'));
  });

onMounted(load);
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1>{{ t('projects.title') }}</h1>
        <p>{{ t('projects.subtitle') }}</p>
      </div>
      <div class="row">
        <button class="btn lg" @click="attempt(() => api.openFolder('www'))">
          <AppIcon name="folder" />{{ t('dashboard.openWww') }}
        </button>
        <button class="btn lg icon-only" :disabled="loading" :title="t('common.refresh')" :aria-label="t('common.refresh')" @click="load">
          <AppIcon name="restart" :class="{ spin: loading }" />
        </button>
        <button class="btn lg primary" @click="openCreate"><AppIcon name="folder" />{{ t('projects.new') }}</button>
      </div>
    </header>

    <template v-if="view">
      <div v-if="!servers.length" class="alert info">
        <AppIcon name="info" />
        <span class="spacer">{{ t('projects.noServer') }}</span>
        <RouterLink class="btn sm" to="/modules">{{ t('dashboard.browse') }}</RouterLink>
      </div>
      <div v-for="s in outdated" :key="s.id" class="alert">
        <AppIcon name="alert" />
        <span class="spacer">{{ t('projects.oldTemplate', { name: s.name, version: s.version }) }}</span>
        <RouterLink class="btn sm" to="/modules">{{ t('dashboard.browse') }}</RouterLink>
      </div>
      <div v-if="readyServers.length && !serving" class="alert info">
        <AppIcon name="info" />
        <span class="spacer">{{ t('projects.notRunning') }}</span>
        <button v-for="s in readyServers" :key="s.id" class="btn sm" @click="startServer(s.id)">
          <AppIcon name="play" />{{ t('projects.startServer', { name: s.name }) }}
        </button>
      </div>
      <div v-for="p in stoppedPhp" :key="p.id" class="alert info">
        <AppIcon name="info" />
        <span class="spacer">{{ t('projects.phpStopped', { version: p.version }) }}</span>
        <button class="btn sm" @click="startServer(p.id)"><AppIcon name="play" />{{ t('projects.startPhp', { version: p.version }) }}</button>
      </div>
      <div v-for="s in lowPortServers" :key="`low-${s.id}`" class="alert info">
        <AppIcon name="info" />
        <span class="spacer">{{ t('projects.lowPorts', { name: s.name, port: s.port }) }}</span>
        <button class="btn sm" :disabled="grantingPorts !== null" @click="enableLowPorts(s.id, s.name)">
          <AppIcon :name="grantingPorts === s.id ? 'restart' : 'shield'" :class="{ spin: grantingPorts === s.id }" />
          {{ grantingPorts === s.id ? t('projects.hostsSyncing') : t('projects.lowPortsEnable') }}
        </button>
      </div>
      <div v-if="servers.length && !phps.length" class="alert info">
        <AppIcon name="info" />
        <span>{{ t('projects.noPhp') }}</span>
      </div>

      <div v-if="hasDomains && !view.hosts_synced" class="card hosts">
        <div class="row">
          <span class="tile warn"><AppIcon name="shield" /></span>
          <p class="spacer">{{ t('projects.hostsMissing') }}</p>
          <button class="btn" :aria-expanded="showManual" @click="showManual = !showManual">{{ t('projects.hostsManual') }}</button>
          <button class="btn primary" :disabled="syncingHosts" @click="syncHosts">
            <AppIcon :name="syncingHosts ? 'restart' : 'shield'" :class="{ spin: syncingHosts }" />
            {{ syncingHosts ? t('projects.hostsSyncing') : t('projects.hostsSync') }}
          </button>
        </div>
        <div v-if="showManual" class="manual">
          <p class="hint">{{ t('projects.hostsManualHint', { file: view.hosts_file }) }}</p>
          <div class="code-row">
            <pre class="mono">{{ view.hosts_lines.join('\n') }}</pre>
            <button class="btn sm" @click="copyLines"><AppIcon name="copy" />{{ t('services.copy') }}</button>
          </div>
        </div>
      </div>

      <div v-if="readyServers.length && !tlsReady" class="card tls">
        <div class="row">
          <span class="tile https"><AppIcon name="lock" /></span>
          <div class="spacer">
            <strong>{{ t('projects.httpsTitle') }}</strong>
            <p class="hint">{{ view.tls.enabled ? t('projects.httpsUntrusted') : t('projects.httpsOff') }}</p>
          </div>
          <button class="btn primary" :disabled="enablingHttps" @click="enableHttps">
            <AppIcon :name="enablingHttps ? 'restart' : 'shield'" :class="{ spin: enablingHttps }" />
            {{ enablingHttps ? t('projects.hostsSyncing') : view.tls.enabled ? t('projects.httpsTrust') : t('projects.httpsEnable') }}
          </button>
        </div>
      </div>
      <div v-if="browserHint" class="alert">
        <AppIcon name="alert" />
        <span class="spacer">{{ t('projects.httpsBrowsers') }}</span>
      </div>

      <p v-if="serving" class="hint">
        {{
          serving.https_port
            ? t('projects.servedByHttps', { name: serving.name, port: serving.port, https: serving.https_port })
            : t('projects.servedBy', { name: serving.name, port: serving.port })
        }}
      </p>

      <TransitionGroup v-if="view.projects.length" tag="section" name="list" class="list">
        <article v-for="p in view.projects" :key="p.name" class="card project">
          <span class="tile"><AppIcon name="globe" :size="22" /></span>
          <div class="project-info">
            <div class="title">
              <h3>{{ p.domain ?? p.name }}</h3>
              <span v-if="p.doc_root !== p.path" class="badge info mono" :title="t('projects.publicRootHint')">
                {{ t('projects.publicRoot') }}
              </span>
            </div>
            <p v-if="p.domain" class="hint mono path">{{ p.path }}</p>
            <p v-else class="bad-name">{{ t('projects.invalidName') }}</p>
            <label v-if="p.domain && phps.length" class="php-pick">
              <span class="label">{{ t('projects.php') }}</span>
              <select
                class="select"
                :value="p.php_missing ? '' : (p.php_line ?? '')"
                @change="setPhp(p.name, ($event.target as HTMLSelectElement).value || null)"
              >
                <option value="">{{ t('projects.phpDefault', { version: phps[0].version }) }}</option>
                <option v-for="php in phpLines" :key="php.line" :value="php.line">PHP {{ php.version }}</option>
              </select>
            </label>
            <p v-if="p.php_missing" class="bad-name">{{ t('projects.phpMissing', { line: p.php_line }) }}</p>
          </div>
          <div class="row">
            <button class="btn" @click="openTerminal(p.name)"><AppIcon name="terminal" />{{ t('nav.terminal') }}</button>
            <button class="btn" @click="attempt(() => api.openFolder('project', p.name))">
              <AppIcon name="folder" />{{ t('common.folder') }}
            </button>
            <button
              class="btn primary"
              :disabled="!p.domain || !serving"
              :title="!serving ? t('projects.notRunning') : undefined"
              @click="attempt(() => api.openProject(p.name))"
            >
              <AppIcon name="external" />{{ t('projects.openSite') }}
            </button>
          </div>
        </article>
      </TransitionGroup>
      <div v-else class="card empty">
        <span class="empty-icon"><AppIcon name="folder" :size="30" /></span>
        <strong>{{ t('projects.empty') }}</strong>
        <p>{{ t('projects.emptyHint') }}</p>
        <button class="btn lg primary" @click="openCreate"><AppIcon name="folder" />{{ t('projects.new') }}</button>
      </div>
    </template>
    <div v-else-if="loading" class="list" aria-hidden="true">
      <div v-for="i in 3" :key="i" class="skeleton" style="height: 84px" />
    </div>

    <dialog ref="createDialog" class="dialog" :aria-label="t('projects.newTitle')">
      <form method="dialog" class="dialog-body" @submit.prevent="create">
        <div class="dialog-head">
          <span class="tile new"><AppIcon name="folder" :size="22" /></span>
          <div>
            <h2>{{ t('projects.newTitle') }}</h2>
            <p>{{ t('projects.newHint', { domain: `${nameValid ? newName : 'my-app'}.test` }) }}</p>
          </div>
        </div>
        <label class="field">
          <span class="label">{{ t('projects.name') }}</span>
          <input
            v-model.trim="newName"
            class="input mono"
            autocomplete="off"
            spellcheck="false"
            maxlength="63"
            required
            :aria-invalid="newName.length > 0 && !nameValid"
            aria-describedby="project-name-hint"
            @input="newName = newName.toLowerCase()"
          />
          <span id="project-name-hint" class="hint" :class="{ invalid: newName.length > 0 && !nameValid }">
            {{ t('projects.namePattern') }}
          </span>
        </label>
        <div class="dialog-actions">
          <button type="button" class="btn" @click="createDialog?.close()">{{ t('common.cancel') }}</button>
          <button type="submit" class="btn primary" :disabled="!nameValid || creating">
            <AppIcon :name="creating ? 'restart' : 'check'" :class="{ spin: creating }" />{{ t('projects.create') }}
          </button>
        </div>
      </form>
    </dialog>
  </div>
</template>

<style scoped>
.list {
  display: grid;
  gap: 14px;
}

.project {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 18px 20px;
}

.project:hover {
  box-shadow: var(--shadow-md);
}

.project-info {
  display: grid;
  gap: 4px;
  flex: 1;
  min-width: 0;
}

.title {
  display: flex;
  align-items: center;
  gap: 10px;
}

.path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.bad-name {
  color: var(--warn);
  font-size: 13.5px;
}

.php-pick {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 4px;
}

.php-pick .label {
  color: var(--ink-2);
  font-size: 13px;
  font-weight: 600;
}

.php-pick .select {
  height: 32px;
  padding: 0 8px;
  font-size: 13px;
}

.tls {
  padding: 16px 18px;
}

.tile.https {
  background: var(--primary-soft);
  color: var(--primary-text);
}

.hosts {
  display: grid;
  gap: 14px;
  padding: 16px 18px;
  border-color: color-mix(in srgb, var(--warn) 35%, var(--line));
}

.tile.warn {
  background: var(--warn-soft);
  color: var(--warn);
}

.dialog-head .tile.new {
  background: var(--primary-soft);
  color: var(--primary-text);
}

.manual {
  display: grid;
  gap: 8px;
}

.code-row {
  display: flex;
  align-items: flex-start;
  gap: 10px;
}

.code-row pre {
  flex: 1;
  margin: 0;
  padding: 12px 14px;
  border-radius: var(--radius);
  background: var(--code-bg);
  color: var(--code-ink);
  font-size: 13px;
  overflow-x: auto;
}

.hint.invalid {
  color: var(--danger);
}

@media (max-width: 900px) {
  .project {
    flex-wrap: wrap;
  }
}
</style>
