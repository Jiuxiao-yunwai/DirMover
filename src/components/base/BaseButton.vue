<script setup lang="ts">
withDefaults(
  defineProps<{
    variant?: "primary" | "secondary" | "subtle" | "danger";
    size?: "small" | "normal";
    loading?: boolean;
    disabled?: boolean;
    type?: "button" | "submit";
  }>(),
  {
    variant: "secondary",
    size: "normal",
    loading: false,
    disabled: false,
    type: "button",
  },
);

defineEmits<{
  click: [event: MouseEvent];
}>();
</script>

<template>
  <button
    class="base-button"
    :class="[`is-${variant}`, `is-${size}`]"
    :type="type"
    :disabled="disabled || loading"
    :aria-busy="loading"
    @click="$emit('click', $event)"
  >
    <span v-if="loading" class="spinner" aria-hidden="true" />
    <slot />
  </button>
</template>

<style scoped>
.base-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 64px;
  height: var(--control-height);
  padding: 0 var(--space-3);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-small);
  color: var(--text-primary);
  background: var(--surface-primary);
  cursor: default;
  user-select: none;
  transition:
    background var(--duration-fast) var(--ease-standard),
    border-color var(--duration-fast) var(--ease-standard);
}

.base-button:hover:not(:disabled) {
  background: var(--surface-hover);
}

.base-button:active:not(:disabled) {
  background: var(--surface-tertiary);
}

.base-button:disabled {
  color: var(--text-disabled);
  opacity: 0.72;
}

.is-primary {
  color: #fff;
  border-color: var(--accent);
  background: var(--accent);
}

.is-primary:hover:not(:disabled) {
  border-color: var(--accent-hover);
  background: var(--accent-hover);
}

.is-primary:active:not(:disabled) {
  border-color: var(--accent-pressed);
  background: var(--accent-pressed);
}

.is-danger {
  color: #fff;
  border-color: var(--danger);
  background: var(--danger);
}

.is-subtle {
  min-width: auto;
  border-color: transparent;
  background: transparent;
}

.is-small {
  height: var(--control-height-small);
  padding-inline: var(--space-2);
  font-size: var(--font-size-small);
}

.spinner {
  width: 13px;
  height: 13px;
  margin-right: var(--space-2);
  border: 2px solid currentColor;
  border-right-color: transparent;
  border-radius: 50%;
  animation: spin 700ms linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
