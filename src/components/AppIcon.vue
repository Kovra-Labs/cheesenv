<script lang="ts">
// 24×24 stroke icons drawn inline (no icon font or library to load).
const PATHS = {
  dashboard: 'M3 3h7v9H3zM14 3h7v5h-7zM14 12h7v9h-7zM3 16h7v5H3z',
  server: 'M3 4h18v7H3zM3 13h18v7H3zM7 7.5h.01M7 16.5h.01',
  box: 'M21 8l-9-5-9 5v8l9 5 9-5zM3.3 7.5L12 12l8.7-4.5M12 12v9',
  logs: 'M14 3H6a2 2 0 00-2 2v14a2 2 0 002 2h12a2 2 0 002-2V9zM14 3v6h6M8 13h8M8 17h5',
  settings: 'M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6',
  play: 'M7 4.5l12 7.5-12 7.5z',
  // An outlined square read as a missing glyph at button size; stop uses an X.
  stop: 'M6 6l12 12M18 6L6 18',
  restart: 'M3 12a9 9 0 0115.5-6.2L21 8M21 3v5h-5M21 12a9 9 0 01-15.5 6.2L3 16M3 21v-5h5',
  external: 'M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 01-1 1H5a1 1 0 01-1-1V7a1 1 0 011-1h5',
  key: 'M11.5 15.5a4 4 0 11-8 0 4 4 0 018 0zM10.3 12.7L20 3M16.5 6.5l3 3M14 9l2.5 2.5',
  folder: 'M3 6a1 1 0 011-1h5l2 2h9a1 1 0 011 1v10a1 1 0 01-1 1H4a1 1 0 01-1-1z',
  search: 'M17 11a6 6 0 11-12 0 6 6 0 0112 0zM21 21l-5.2-5.2',
  download: 'M12 3v12M7 10l5 5 5-5M4 21h16',
  trash: 'M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3',
  check: 'M5 12.5l4.5 4.5L19 7',
  x: 'M6 6l12 12M18 6L6 18',
  alert: 'M12 9v4M12 17h.01M10.3 3.9L2.4 18a2 2 0 001.7 3h15.8a2 2 0 001.7-3L13.7 3.9a2 2 0 00-3.4 0z',
  info: 'M21 12a9 9 0 11-18 0 9 9 0 0118 0zM12 11v5M12 8h.01',
  sun: 'M16 12a4 4 0 11-8 0 4 4 0 018 0zM12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4',
  moon: 'M20 14.5A8 8 0 019.5 4 8 8 0 1020 14.5z',
  monitor: 'M3 4h18v12H3zM8 20h8M12 16v4',
  globe: 'M21 12a9 9 0 11-18 0 9 9 0 0118 0zM3 12h18M12 3a14 14 0 010 18M12 3a14 14 0 000 18',
  database: 'M20 6c0 1.7-3.6 3-8 3S4 7.7 4 6s3.6-3 8-3 8 1.3 8 3zM4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3',
  code: 'M8 7l-5 5 5 5M16 7l5 5-5 5M14 4l-4 16',
  terminal: 'M4 5h16v14H4zM8 10l3 2-3 2M13 15h3',
  zap: 'M13 2L4 14h7l-1 8 9-12h-7z',
  chevron: 'M6 9l6 6 6-6',
  copy: 'M9 9h11v11H9zM5 15H4V4h11v1',
  eye: 'M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12zM15 12a3 3 0 11-6 0 3 3 0 016 0z',
  eyeOff:
    'M3 3l18 18M10.6 5.1A10 10 0 0112 5c6.5 0 10 7 10 7a17 17 0 01-3.2 4M6.6 6.6C3.9 8.3 2 12 2 12s3.5 7 10 7a10 10 0 005.4-1.6M9.9 9.9a3 3 0 004.2 4.2',
  activity: 'M3 12h4l3-8 4 16 3-8h4',
  arrowRight: 'M5 12h14M13 6l6 6-6 6',
  cloud: 'M7 18a5 5 0 01-.6-10A6 6 0 0118 9a4.5 4.5 0 01-.5 9z',
  drive: 'M3 13h18v6H3zM3 13l3-8h12l3 8M7 16h.01',
  shield: 'M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z',
  power: 'M12 3v8M6.4 6.4a8 8 0 1011.2 0',
} as const;

export type IconName = keyof typeof PATHS;
</script>

<script setup lang="ts">
defineProps<{ name: IconName; size?: number }>();
</script>

<template>
  <svg
    class="icon"
    viewBox="0 0 24 24"
    :width="size ?? 18"
    :height="size ?? 18"
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    focusable="false"
  >
    <path :d="PATHS[name]" />
  </svg>
</template>
