<script setup lang="ts">
import { ref } from "vue";

const ready = ref(true);
</script>

<template>
  <main class="bootstrap">
    <h1>DirMover</h1>
    <p v-if="ready">Tauri 2 重构骨架已就绪。</p>
  </main>
</template>

<style scoped>
.bootstrap {
  display: grid;
  min-height: 100vh;
  place-content: center;
  gap: var(--space-2);
  color: var(--text-primary);
  background: var(--surface-app);
  text-align: center;
}

h1,
p {
  margin: 0;
}
</style>
