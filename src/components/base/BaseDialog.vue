<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from "vue";

const props = withDefaults(
  defineProps<{
    open: boolean;
    title: string;
    description?: string;
    width?: string;
    closeOnBackdrop?: boolean;
  }>(),
  {
    description: "",
    width: "520px",
    closeOnBackdrop: true,
  },
);

const emit = defineEmits<{
  close: [];
}>();
const dialog = ref<HTMLElement | null>(null);

const close = (): void => emit("close");
const onKeydown = (event: KeyboardEvent): void => {
  if (event.key === "Escape" && props.open) {
    event.preventDefault();
    close();
  }
};

watch(
  () => props.open,
  async (open) => {
    if (open) {
      document.addEventListener("keydown", onKeydown);
      await nextTick();
      dialog.value?.focus();
    } else {
      document.removeEventListener("keydown", onKeydown);
    }
  },
  { immediate: true },
);

onBeforeUnmount(() => document.removeEventListener("keydown", onKeydown));
</script>

<template>
  <Teleport to="body">
    <Transition name="fade">
      <div
        v-if="open"
        class="dialog-overlay"
        @mousedown.self="closeOnBackdrop && close()"
      >
        <section
          ref="dialog"
          class="dialog"
          role="dialog"
          aria-modal="true"
          :aria-label="title"
          :style="{ width }"
          tabindex="-1"
        >
          <header class="dialog-header">
            <div>
              <h2>{{ title }}</h2>
              <p v-if="description">{{ description }}</p>
            </div>
            <button class="close-button" type="button" aria-label="关闭" @click="close">
              ×
            </button>
          </header>
          <div class="dialog-content">
            <slot />
          </div>
          <footer v-if="$slots.footer" class="dialog-footer">
            <slot name="footer" />
          </footer>
        </section>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.dialog-overlay {
  position: fixed;
  z-index: 1000;
  inset: 0;
  display: grid;
  place-items: center;
  padding: var(--space-5);
  background: var(--overlay);
}

.dialog {
  max-width: min(100%, 720px);
  max-height: min(88vh, 760px);
  overflow: hidden;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-large);
  color: var(--text-primary);
  background: var(--surface-primary);
  box-shadow: var(--shadow-flyout);
}

.dialog:focus {
  outline: none;
}

.dialog-header,
.dialog-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-4);
}

.dialog-header {
  border-bottom: 1px solid var(--border-subtle);
}

.dialog-footer {
  justify-content: flex-end;
  border-top: 1px solid var(--border-subtle);
}

.dialog-header h2 {
  font-size: var(--font-size-title);
  font-weight: 600;
}

.dialog-header p {
  margin-top: var(--space-1);
  color: var(--text-secondary);
  font-size: var(--font-size-small);
}

.dialog-content {
  overflow: auto;
  padding: var(--space-4);
}

.close-button {
  width: var(--control-height-small);
  height: var(--control-height-small);
  border-radius: var(--radius-small);
  color: var(--text-secondary);
  background: transparent;
  font-size: 20px;
}

.close-button:hover {
  color: var(--text-primary);
  background: var(--surface-hover);
}
</style>
