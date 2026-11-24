<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import { store } from '../store';
import ServiceCard from '../components/ServiceCard.vue';

const { t } = useI18n();
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1>{{ t('services.title') }}</h1>
        <p>{{ t('services.subtitle') }}</p>
      </div>
    </header>
    <section v-if="store.services.length" class="list">
      <ServiceCard v-for="s in store.services" :key="s.id" :service="s" detailed />
    </section>
    <div v-else class="card empty">
      <p>{{ t('services.empty') }}</p>
      <RouterLink class="btn primary" to="/modules">{{ t('dashboard.browse') }}</RouterLink>
    </div>
  </div>
</template>

<style scoped>
.list {
  display: grid;
  gap: 10px;
}
</style>
