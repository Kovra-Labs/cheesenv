<script setup lang="ts">
import { ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { store, runningCount, dismissToast } from './store';
import AppIcon, { type IconName } from './components/AppIcon.vue';

const { t } = useI18n();
const main = ref<HTMLElement | null>(null);
const nav: { to: string; key: string; icon: IconName }[] = [
  { to: '/', key: 'dashboard', icon: 'dashboard' },
  { to: '/services', key: 'services', icon: 'server' },
  { to: '/modules', key: 'modules', icon: 'box' },
  { to: '/logs', key: 'logs', icon: 'logs' },
  { to: '/settings', key: 'settings', icon: 'settings' },
];

/** Each page starts at the top once the previous one has faded out. */
const resetScroll = () => main.value?.scrollTo({ top: 0 });
</script>

<template>
  <div class="shell">
    <aside class="side">
      <div class="brand">
        <svg viewBox="0 0 512 512" aria-hidden="true">
          <defs>
            <linearGradient id="brand-g" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" stop-color="#42a5f5" />
              <stop offset="1" stop-color="#1976d2" />
            </linearGradient>
          </defs>
          <rect width="512" height="512" rx="128" fill="url(#brand-g)" />
          <g fill="none" stroke="#fff" stroke-width="34" stroke-linejoin="round">
            <path d="M256 104 408 180 256 256 104 180Z" fill="#fff" />
            <path d="M104 256l152 76 152-76" />
            <path d="M104 332l152 76 152-76" />
          </g>
        </svg>
        <div>
          <strong>Stackyard</strong>
          <span>{{ t('app.tagline') }}</span>
        </div>
      </div>

      <nav class="nav" :aria-label="t('app.menu')">
        <RouterLink v-for="item in nav" :key="item.key" :to="item.to" class="nav-link">
          <AppIcon :name="item.icon" :size="20" />
          <span>{{ t(`nav.${item.key}`) }}</span>
          <Transition name="pop">
            <span v-if="item.key === 'services' && runningCount" class="count">{{ runningCount }}</span>
          </Transition>
        </RouterLink>
      </nav>

      <div class="side-foot">
        <div class="status" :class="{ on: runningCount }">
          <span class="pulse" aria-hidden="true" />
          <span>{{ runningCount ? t('app.running', { n: runningCount }) : t('app.idle') }}</span>
        </div>
        <p v-if="store.info" class="mono ver">v{{ store.info.version }} · {{ store.info.platform }}</p>
      </div>
    </aside>

    <main ref="main" class="main">
      <RouterView v-if="store.ready" v-slot="{ Component }">
        <Transition name="page" mode="out-in" @after-leave="resetScroll">
          <component :is="Component" />
        </Transition>
      </RouterView>
      <div v-else class="boot" role="status">
        <span class="loader" aria-hidden="true" />
        <span class="hint">{{ t('common.loading') }}</span>
      </div>
    </main>

    <TransitionGroup tag="div" name="toast" class="toasts" role="status" aria-live="polite">
      <div v-for="toast in store.toasts" :key="toast.id" class="toast" :class="toast.kind">
        <span class="toast-icon"><AppIcon :name="toast.kind === 'error' ? 'alert' : 'check'" :size="18" /></span>
        <span class="toast-text">{{ toast.text }}</span>
        <button class="toast-x" :aria-label="t('common.close')" @click="dismissToast(toast.id)">
          <AppIcon name="x" :size="16" />
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.shell {
  display: grid;
  grid-template-columns: 248px 1fr;
  height: 100%;
}

.side {
  display: flex;
  flex-direction: column;
  gap: 24px;
  padding: 22px 14px 18px;
  border-right: 1px solid var(--line);
  background: var(--surface);
  transition:
    background-color var(--dur-slow) var(--ease),
    border-color var(--dur-slow) var(--ease);
}

.brand {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 8px;
}

.brand svg {
  width: 38px;
  height: 38px;
  flex: none;
  filter: drop-shadow(0 4px 10px rgb(33 150 243 / 0.35));
}

.brand div {
  display: grid;
  line-height: 1.2;
}

.brand strong {
  font-size: 17px;
  letter-spacing: -0.02em;
}

.brand span {
  color: var(--ink-2);
  font-size: 12.5px;
}

.nav {
  display: grid;
  gap: 4px;
}

.nav-link {
  position: relative;
  display: flex;
  align-items: center;
  gap: 12px;
  height: 46px;
  padding: 0 14px;
  border-radius: 12px;
  color: var(--ink-2);
  text-decoration: none;
  font-weight: 600;
  transition:
    background-color var(--dur) var(--ease),
    color var(--dur) var(--ease);
}

.nav-link::before {
  content: '';
  position: absolute;
  left: -14px;
  top: 50%;
  width: 4px;
  height: 22px;
  border-radius: 0 4px 4px 0;
  background: var(--primary);
  transform: translateY(-50%) scaleY(0);
  transition: transform var(--dur-slow) var(--ease);
}

.nav-link:hover {
  background: var(--surface-2);
  color: var(--ink);
}

.nav-link.router-link-exact-active {
  background: var(--primary-soft);
  color: var(--primary-text);
}

.nav-link.router-link-exact-active::before {
  transform: translateY(-50%) scaleY(1);
}

.count {
  margin-left: auto;
  min-width: 24px;
  height: 22px;
  padding: 0 7px;
  border-radius: 999px;
  background: var(--primary);
  color: var(--on-primary);
  font-size: 12px;
  line-height: 22px;
  text-align: center;
}

.pop-enter-active,
.pop-leave-active {
  transition:
    transform var(--dur-slow) var(--ease),
    opacity var(--dur-slow) var(--ease);
}

.pop-enter-from,
.pop-leave-to {
  opacity: 0;
  transform: scale(0.5);
}

.side-foot {
  display: grid;
  gap: 10px;
  margin-top: auto;
}

.status {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 14px;
  border-radius: 12px;
  background: var(--surface-2);
  color: var(--ink-2);
  font-size: 13.5px;
  font-weight: 600;
  transition:
    background-color var(--dur-slow) var(--ease),
    color var(--dur-slow) var(--ease);
}

.status.on {
  background: var(--success-soft);
  color: var(--success);
}

.pulse {
  position: relative;
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: currentColor;
}

.status.on .pulse::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: currentColor;
  animation: ping 1.8s var(--ease) infinite;
}

@keyframes ping {
  to {
    transform: scale(2.6);
    opacity: 0;
  }
}

.ver {
  padding: 0 14px;
  color: var(--ink-3);
  font-size: 12px;
}

.main {
  overflow-y: auto;
  scrollbar-gutter: stable;
}

.boot {
  display: grid;
  place-items: center;
  align-content: center;
  gap: 14px;
  height: 100%;
}

.loader {
  width: 34px;
  height: 34px;
  border: 3px solid var(--primary-soft);
  border-top-color: var(--primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

.page-enter-active,
.page-leave-active {
  transition:
    opacity 200ms var(--ease),
    transform 200ms var(--ease);
}

.page-enter-from {
  opacity: 0;
  transform: translateY(10px);
}

.page-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}

.toasts {
  position: fixed;
  right: 22px;
  bottom: 22px;
  display: grid;
  gap: 10px;
  width: min(400px, calc(100vw - 44px));
  z-index: 20;
}

.toast {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 14px 14px 14px 16px;
  border: 1px solid var(--line);
  border-radius: 14px;
  background: var(--surface);
  box-shadow: var(--shadow-lg);
}

.toast-icon {
  display: grid;
  place-items: center;
  flex: none;
  width: 28px;
  height: 28px;
  border-radius: 8px;
  background: var(--success-soft);
  color: var(--success);
}

.toast.error .toast-icon {
  background: var(--danger-soft);
  color: var(--danger);
}

.toast.error {
  border-color: color-mix(in srgb, var(--danger) 35%, var(--line));
}

.toast-text {
  flex: 1;
  padding-top: 3px;
}

.toast-x {
  display: grid;
  place-items: center;
  flex: none;
  width: 30px;
  height: 30px;
  border: 0;
  border-radius: 8px;
  background: none;
  color: var(--ink-2);
  cursor: pointer;
  transition: background-color var(--dur) var(--ease);
}

.toast-x:hover {
  background: var(--surface-2);
  color: var(--ink);
}

.toast-enter-active,
.toast-leave-active {
  transition:
    opacity var(--dur-slow) var(--ease),
    transform var(--dur-slow) var(--ease);
}

.toast-enter-from {
  opacity: 0;
  transform: translateX(40px);
}

.toast-leave-to {
  opacity: 0;
  transform: translateX(40px) scale(0.96);
}

.toast-leave-active {
  position: absolute;
  right: 0;
  left: 0;
}

.toast-move {
  transition: transform var(--dur-slow) var(--ease);
}
</style>
