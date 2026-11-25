<script setup lang="ts">
// Animates height for content shown with v-if/v-show (CSS can't transition to `auto`).
function enter(el: Element) {
  const e = el as HTMLElement;
  e.style.height = '0';
  e.style.opacity = '0';
  requestAnimationFrame(() => {
    e.style.height = `${e.scrollHeight}px`;
    e.style.opacity = '1';
  });
}
function leave(el: Element) {
  const e = el as HTMLElement;
  e.style.height = `${e.scrollHeight}px`;
  e.style.opacity = '1';
  requestAnimationFrame(() => {
    e.style.height = '0';
    e.style.opacity = '0';
  });
}
function reset(el: Element) {
  const e = el as HTMLElement;
  e.style.height = '';
  e.style.opacity = '';
}
</script>

<template>
  <Transition name="collapse" @enter="enter" @after-enter="reset" @leave="leave" @after-leave="reset">
    <slot />
  </Transition>
</template>

<style>
.collapse-enter-active,
.collapse-leave-active {
  overflow: hidden;
  transition:
    height var(--dur-slow) var(--ease),
    opacity var(--dur-slow) var(--ease);
}
</style>
