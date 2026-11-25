import { i18n } from './i18n';
import type { IconName } from './components/AppIcon.vue';

const CATEGORY_ICONS: Record<string, IconName> = {
  'web-server': 'globe',
  runtime: 'code',
  database: 'database',
  cache: 'zap',
  search: 'search',
  tools: 'terminal',
};

export function categoryIcon(category: string): IconName {
  return CATEGORY_ICONS[category] ?? 'box';
}

export function fmtBytes(n: number) {
  if (n >= 1048576) return `${(n / 1048576).toFixed(1)} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

export function categoryLabel(category: string) {
  const key = `category.${category}`;
  return i18n.global.te(key) ? i18n.global.t(key) : category;
}
