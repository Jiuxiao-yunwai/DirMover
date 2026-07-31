<script setup lang="ts">
defineProps<{
  title: string;
  description: string;
}>();
</script>

<template>
  <div class="empty-state">
    <div class="empty-mark" aria-hidden="true">—</div>
    <strong>{{ title }}</strong>
    <p>{{ description }}</p>
    <slot />
  </div>
</template>

<style scoped>
.empty-state {
  display: grid;
  min-height: 220px;
  place-items: center;
  align-content: center;
  gap: var(--space-2);
  padding: var(--space-5);
  color: var(--text-secondary);
  text-align: center;
}

.empty-mark {
  display: grid;
  width: 36px;
  height: 36px;
  place-items: center;
  border: 1px solid var(--border-subtle);
  border-radius: 50%;
  background: var(--surface-secondary);
}

strong {
  color: var(--text-primary);
  font-weight: 600;
}

p {
  max-width: 420px;
  font-size: var(--font-size-small);
}
</style>
