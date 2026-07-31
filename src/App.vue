<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef } from "vue";
import BaseButton from "@/components/base/BaseButton.vue";
import BaseProgress from "@/components/base/BaseProgress.vue";
import AppLayout, { type ViewKey } from "@/layouts/AppLayout.vue";
import { useTheme } from "@/composables/useTheme";
import { api } from "@/services/api";
import { normalizeAppError } from "@/services/tauri";
import type { BootstrapData } from "@/types/domain";
import LinkInspectorView from "@/views/LinkInspectorView.vue";
import MovesView from "@/views/MovesView.vue";
import SettingsView from "@/views/SettingsView.vue";
import SpaceAnalysisView from "@/views/SpaceAnalysisView.vue";

type ToastTone = "info" | "success" | "warning" | "error";

const bootstrap = shallowRef<BootstrapData | null>(null);
const bootstrapError = ref("");
const loading = ref(true);
const activeView = ref<ViewKey>("space");
const status = ref("正在启动…");
const busy = ref(false);
const toast = ref<{ message: string; tone: ToastTone } | null>(null);
const { preference: theme } = useTheme();
let toastTimer: ReturnType<typeof setTimeout> | null = null;

onMounted(loadBootstrap);
onMounted(() => {
  document.addEventListener("contextmenu", suppressNativeContextMenu);
});
onBeforeUnmount(() => {
  if (toastTimer) clearTimeout(toastTimer);
  document.removeEventListener("contextmenu", suppressNativeContextMenu);
});

async function loadBootstrap(): Promise<void> {
  loading.value = true;
  bootstrapError.value = "";
  try {
    bootstrap.value = await api.bootstrap();
    status.value = "就绪";
  } catch (error: unknown) {
    bootstrapError.value = normalizeAppError(error).message;
    status.value = "启动失败";
  } finally {
    loading.value = false;
  }
}

async function refreshBootstrap(): Promise<void> {
  try {
    bootstrap.value = await api.bootstrap();
  } catch (error: unknown) {
    notify(normalizeAppError(error).message, "error");
  }
}

function notify(message: string, tone: ToastTone = "info"): void {
  toast.value = { message, tone };
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toast.value = null;
  }, tone === "error" ? 6500 : 4000);
}

function suppressNativeContextMenu(event: MouseEvent): void {
  const target = event.target;
  if (!(target instanceof Element) || !target.closest("[data-app-context-menu]")) {
    event.preventDefault();
  }
}
</script>

<template>
  <div v-if="loading" class="startup">
    <div class="startup-mark">D</div>
    <strong>正在启动 DirMover</strong>
    <BaseProgress indeterminate />
  </div>

  <div v-else-if="bootstrapError || !bootstrap" class="startup error">
    <div class="startup-mark">!</div>
    <strong>DirMover 无法启动</strong>
    <p>{{ bootstrapError }}</p>
    <BaseButton variant="primary" @click="loadBootstrap">重试</BaseButton>
  </div>

  <AppLayout
    v-else
    :active-view="activeView"
    :is-administrator="bootstrap.isAdministrator"
    :theme="theme"
    :status="status"
    :busy="busy"
    @navigate="activeView = $event"
    @update:theme="theme = $event"
  >
    <SpaceAnalysisView
      v-if="activeView === 'space'"
      :bootstrap="bootstrap"
      @status="status = $event"
      @busy="busy = $event"
      @refresh-data="refreshBootstrap"
      @notify="notify"
    />
    <MovesView
      v-else-if="activeView === 'moves'"
      :bootstrap="bootstrap"
      @status="status = $event"
      @busy="busy = $event"
      @refresh-data="refreshBootstrap"
      @notify="notify"
    />
    <LinkInspectorView
      v-else-if="activeView === 'links'"
      :bootstrap="bootstrap"
      @status="status = $event"
      @busy="busy = $event"
      @refresh-data="refreshBootstrap"
      @notify="notify"
    />
    <SettingsView
      v-else
      :bootstrap="bootstrap"
      @status="status = $event"
      @busy="busy = $event"
      @refresh-data="refreshBootstrap"
      @notify="notify"
    />
  </AppLayout>

  <Transition name="toast">
    <div v-if="toast" class="toast" :class="toast.tone" role="status">
      <span>{{ toast.message }}</span>
      <button type="button" aria-label="关闭通知" @click="toast = null">×</button>
    </div>
  </Transition>
</template>

<style scoped>
.startup {
  display: grid;
  width: 100vw;
  height: 100vh;
  place-content: center;
  justify-items: center;
  gap: var(--space-3);
  color: var(--text-primary);
  background: var(--surface-app);
}

.startup :deep(.progress-track) {
  width: 220px;
}

.startup-mark {
  display: grid;
  width: 44px;
  height: 44px;
  place-items: center;
  border-radius: var(--radius-large);
  color: #fff;
  background: var(--accent);
  font-size: 20px;
  font-weight: 700;
}

.startup.error {
  text-align: center;
}

.startup.error .startup-mark {
  background: var(--danger);
}

.startup p {
  max-width: 480px;
  color: var(--text-secondary);
}

.toast {
  position: fixed;
  z-index: 1200;
  right: var(--space-4);
  bottom: calc(var(--status-height) + var(--space-4));
  display: flex;
  max-width: 460px;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3);
  border: 1px solid var(--border-strong);
  border-left: 3px solid var(--accent);
  border-radius: var(--radius-medium);
  color: var(--text-primary);
  background: var(--surface-primary);
  box-shadow: var(--shadow-flyout);
}

.toast.success {
  border-left-color: var(--success);
}

.toast.warning {
  border-left-color: var(--warning);
}

.toast.error {
  border-left-color: var(--danger);
}

.toast button {
  width: 24px;
  height: 24px;
  border-radius: var(--radius-small);
  color: var(--text-secondary);
  background: transparent;
  font-size: 18px;
}

.toast button:hover {
  background: var(--surface-hover);
}

.toast-enter-active,
.toast-leave-active {
  transition:
    opacity var(--duration-normal) var(--ease-standard),
    transform var(--duration-normal) var(--ease-standard);
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(8px);
}
</style>
