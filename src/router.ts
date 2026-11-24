import { createRouter, createWebHashHistory } from 'vue-router';

// Views are split per route so only the visible page is parsed at startup.
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', name: 'dashboard', component: () => import('./views/DashboardView.vue') },
    { path: '/services', name: 'services', component: () => import('./views/ServicesView.vue') },
    { path: '/modules', name: 'modules', component: () => import('./views/ModulesView.vue') },
    { path: '/logs', name: 'logs', component: () => import('./views/LogsView.vue') },
    { path: '/settings', name: 'settings', component: () => import('./views/SettingsView.vue') },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
});
