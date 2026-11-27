<script setup lang="ts">
import { computed } from 'vue';
import { MODULE_LOGOS } from '../moduleLogos';
import { categoryIcon } from '../ui';
import AppIcon from './AppIcon.vue';

/** A module's logo, or its category icon when it has none. */
const props = defineProps<{ name: string; category: string }>();
const logo = computed(() => MODULE_LOGOS[props.name]);
</script>

<template>
  <span v-if="logo" class="tile logo">
    <img v-if="'src' in logo" :src="logo.src" :alt="name" height="36" />
    <svg v-else viewBox="0 0 24 24" width="26" height="26" role="img" :aria-label="name">
      <path :d="logo.path" :fill="logo.color" />
    </svg>
  </span>
  <span v-else class="tile"><AppIcon :name="categoryIcon(category)" :size="22" /></span>
</template>

<style scoped>
/* Brand colours are made for white, so the logo sits on white in both themes. */
.logo {
  background: #fff;
  box-shadow: inset 0 0 0 1px var(--line);
}

.logo img {
  width: auto;
  height: 36px;
}
</style>
