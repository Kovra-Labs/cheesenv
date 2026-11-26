<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import { segments, type ChangeKind, type Release } from '../changelog';

defineProps<{ releases: Release[] }>();
const { t, locale } = useI18n();

const tone: Record<ChangeKind, string> = {
  added: 'ok',
  changed: 'info',
  fixed: 'warn',
  removed: 'bad',
  security: 'bad',
  deprecated: 'warn',
  other: 'info',
};

function fmtDate(date: string | null) {
  if (!date || !/^\d{4}-\d{2}-\d{2}$/.test(date)) return date;
  return new Intl.DateTimeFormat(locale.value, { year: 'numeric', month: 'long', day: 'numeric' }).format(new Date(`${date}T00:00:00`));
}
</script>

<template>
  <div class="changelog">
    <article v-for="r in releases" :key="r.version" class="release">
      <header>
        <h3 class="mono">v{{ r.version }}</h3>
        <time v-if="r.date" class="hint" :datetime="r.date">{{ fmtDate(r.date) }}</time>
      </header>
      <section v-for="(g, i) in r.groups" :key="i" class="group">
        <span class="badge" :class="tone[g.kind]">{{ t(`changelog.kind.${g.kind}`) }}</span>
        <ul>
          <li v-for="(item, j) in g.items" :key="j">
            <template v-for="(s, k) in segments(item)" :key="k">
              <code v-if="s.code">{{ s.text }}</code>
              <template v-else>{{ s.text.replace(/\*\*/g, '') }}</template>
            </template>
          </li>
        </ul>
      </section>
    </article>
  </div>
</template>

<style scoped>
.changelog {
  display: grid;
  gap: 18px;
}

.release {
  display: grid;
  gap: 10px;
}

.release + .release {
  padding-top: 16px;
  border-top: 1px solid var(--line);
}

header {
  display: flex;
  align-items: baseline;
  gap: 10px;
}

h3 {
  font-size: 15px;
}

.group {
  display: grid;
  gap: 6px;
  justify-items: start;
}

ul {
  display: grid;
  gap: 4px;
  margin: 0;
  padding-left: 20px;
  color: var(--ink-2);
}

code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--code-bg);
  color: var(--code-ink);
  font-size: 0.9em;
}
</style>
