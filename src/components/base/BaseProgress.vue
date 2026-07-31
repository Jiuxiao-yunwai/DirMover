<script setup lang="ts">
withDefaults(
  defineProps<{
    value?: number;
    indeterminate?: boolean;
  }>(),
  {
    value: 0,
    indeterminate: false,
  },
);
</script>

<template>
  <div
    class="progress-track"
    role="progressbar"
    :aria-valuenow="indeterminate ? undefined : Math.round(value)"
    aria-valuemin="0"
    aria-valuemax="100"
  >
    <div
      class="progress-value"
      :class="{ indeterminate }"
      :style="indeterminate ? undefined : { width: `${Math.max(0, Math.min(100, value))}%` }"
    />
  </div>
</template>

<style scoped>
.progress-track {
  position: relative;
  height: 4px;
  overflow: hidden;
  border-radius: 2px;
  background: var(--surface-tertiary);
}

.progress-value {
  height: 100%;
  border-radius: inherit;
  background: var(--accent);
  transition: width var(--duration-normal) var(--ease-standard);
}

.indeterminate {
  position: absolute;
  width: 36%;
  animation: progress 1.1s ease-in-out infinite;
}

@keyframes progress {
  from {
    transform: translateX(-120%);
  }
  to {
    transform: translateX(360%);
  }
}
</style>
