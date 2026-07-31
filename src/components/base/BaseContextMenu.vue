<script setup lang="ts">
import { onBeforeUnmount, watch } from "vue";

const props = defineProps<{
  open: boolean;
  x: number;
  y: number;
}>();

const emit = defineEmits<{
  close: [];
}>();

const close = (): void => emit("close");
watch(
  () => props.open,
  (open) => {
    if (open) {
      window.addEventListener("pointerdown", close, { once: true });
      window.addEventListener("blur", close, { once: true });
    }
  },
);
onBeforeUnmount(() => {
  window.removeEventListener("pointerdown", close);
  window.removeEventListener("blur", close);
});
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="context-menu"
      role="menu"
      :style="{ left: `${x}px`, top: `${y}px` }"
      @pointerdown.stop
    >
      <slot />
    </div>
  </Teleport>
</template>

<style scoped>
.context-menu {
  position: fixed;
  z-index: 1100;
  min-width: 188px;
  padding: var(--space-1);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-medium);
  background: var(--surface-primary);
  box-shadow: var(--shadow-flyout);
}

.context-menu :deep(button) {
  display: block;
  width: 100%;
  height: 30px;
  padding: 0 var(--space-3);
  border-radius: var(--radius-small);
  color: var(--text-primary);
  background: transparent;
  text-align: left;
}

.context-menu :deep(button:hover) {
  background: var(--surface-hover);
}
</style>
