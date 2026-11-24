<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import { store, runningCount, dismissToast } from './store';

const { t } = useI18n();
const nav = [
  { to: '/', key: 'dashboard', icon: 'M3 3h7v7H3zM14 3h7v4h-7zM14 10h7v11h-7zM3 13h7v8H3z' },
  { to: '/services', key: 'services', icon: 'M4 5h16v5H4zM4 14h16v5H4zM7 7.5h.01M7 16.5h.01' },
  { to: '/modules', key: 'modules', icon: 'M12 3l8 4.5v9L12 21l-8-4.5v-9zM12 12l8-4.5M12 12v9M12 12L4 7.5' },
  { to: '/logs', key: 'logs', icon: 'M5 4h14v16H5zM8 8h8M8 12h8M8 16h5' },
  { to: '/settings', key: 'settings', icon: 'M12 15a3 3 0 100-6 3 3 0 000 6zM19 12l2-1-1-3-2 .2-1.5-1.5L16.7 4l-3-1-1 2h-1.4l-1-2-3 1 .2 2.7L6 8.2 4 8l-1 3 2 1v1.4l-2 1 1 3 2-.2 1.5 1.5-.2 2.3 3 1 1-2h1.4l1 2 3-1-.2-2.3 1.5-1.5 2 .2 1-3-2-1z' },
];
</script>

<template>
  <div class="shell">
    <aside class="side">
      <div class="brand">
        <svg viewBox="0 0 512 512" aria-hidden="true">
          <rect width="512" height="512" rx="112" fill="var(--accent)" />
          <g fill="none" stroke="var(--accent-ink)" stroke-width="34" stroke-linejoin="round">
            <path d="M256 104 408 180 256 256 104 180Z" fill="var(--accent-ink)" />
            <path d="M104 256l152 76 152-76" />
            <path d="M104 332l152 76 152-76" />
          </g>
        </svg>
        <span>Stackyard</span>
      </div>
      <nav :aria-label="t('nav.dashboard')">
        <RouterLink v-for="item in nav" :key="item.key" :to="item.to" class="nav-link">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path :d="item.icon" /></svg>
          <span>{{ t(`nav.${item.key}`) }}</span>
          <span v-if="item.key === 'services' && runningCount" class="count">{{ runningCount }}</span>
        </RouterLink>
      </nav>
      <p v-if="store.info" class="side-foot mono">v{{ store.info.version }}</p>
    </aside>

    <main class="main">
      <RouterView v-if="store.ready" />
      <p v-else class="page hint">{{ t('common.loading') }}</p>
    </main>

    <div class="toasts" role="status" aria-live="polite">
      <div v-for="toast in store.toasts" :key="toast.id" class="toast" :class="toast.kind">
        <span>{{ toast.text }}</span>
        <button class="toast-x" :aria-label="t('common.close')" @click="dismissToast(toast.id)">×</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.shell {
  display: grid;
  grid-template-columns: 208px 1fr;
  height: 100%;
}

.side {
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 18px 12px;
  border-right: 1px solid var(--line);
  background: var(--surface);
}

.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 8px;
  font-weight: 700;
  font-size: 16px;
  letter-spacing: -0.01em;
}

.brand svg {
  width: 28px;
  height: 28px;
}

nav {
  display: grid;
  gap: 2px;
}

.nav-link {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: 8px;
  color: var(--ink-2);
  text-decoration: none;
  font-weight: 500;
}

.nav-link svg {
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.7;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.nav-link:hover {
  background: var(--surface-2);
  color: var(--ink);
}

.nav-link.router-link-exact-active {
  background: var(--accent-soft);
  color: var(--accent);
}

.count {
  margin-left: auto;
  min-width: 20px;
  padding: 0 6px;
  border-radius: 999px;
  background: var(--accent);
  color: var(--accent-ink);
  font-size: 11.5px;
  text-align: center;
}

.side-foot {
  margin-top: auto;
  padding: 0 10px;
  color: var(--ink-2);
}

.main {
  overflow-y: auto;
}

.toasts {
  position: fixed;
  right: 18px;
  bottom: 18px;
  display: grid;
  gap: 8px;
  max-width: 380px;
  z-index: 10;
}

.toast {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 10px 12px;
  border-radius: 10px;
  border: 1px solid var(--line);
  background: var(--surface);
  box-shadow: 0 6px 24px rgb(0 0 0 / 0.12);
}

.toast.error {
  border-color: var(--danger);
  background: var(--danger-soft);
}

.toast-x {
  margin-left: auto;
  border: 0;
  background: none;
  color: inherit;
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
}
</style>
