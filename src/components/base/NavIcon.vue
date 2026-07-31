<script setup lang="ts">
type NavIconName = "space" | "moves" | "links" | "settings" | "help";

defineProps<{
  name: NavIconName;
}>();
</script>

<template>
  <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <template v-if="name === 'space'">
      <rect x="3.5" y="3.5" width="7" height="7" rx="1.5" />
      <rect x="13.5" y="3.5" width="7" height="7" rx="1.5" />
      <rect x="3.5" y="13.5" width="7" height="7" rx="1.5" />
      <rect x="13.5" y="13.5" width="7" height="7" rx="1.5" />
    </template>
    <template v-else-if="name === 'moves'">
      <path d="M4 7h14m-3.5-3.5L18 7l-3.5 3.5" />
      <path d="M20 17H6m3.5-3.5L6 17l3.5 3.5" />
    </template>
    <template v-else-if="name === 'links'">
      <path d="m9.5 14.5 5-5" />
      <path d="m7.1 16.9-1 1a3.5 3.5 0 0 1-5-5l3-3a3.5 3.5 0 0 1 5 0" />
      <path d="m16.9 7.1 1-1a3.5 3.5 0 1 1 5 5l-3 3a3.5 3.5 0 0 1-5 0" />
    </template>
    <template v-else-if="name === 'settings'">
      <path d="M4 6h5m6 0h5M4 12h9m6 0h1M4 18h2m6 0h8" />
      <circle cx="12" cy="6" r="2" />
      <circle cx="16" cy="12" r="2" />
      <circle cx="9" cy="18" r="2" />
    </template>
    <template v-else>
      <circle cx="12" cy="12" r="9" />
      <path d="M9.6 9a2.6 2.6 0 1 1 3.2 2.5c-.8.3-.8.9-.8 1.5" />
      <path d="M12 17h.01" />
    </template>
  </svg>
</template>

<style scoped>
svg {
  display: block;
  width: 20px;
  height: 20px;
  stroke: currentColor;
  stroke-width: 1.7;
  stroke-linecap: round;
  stroke-linejoin: round;
}
</style>
