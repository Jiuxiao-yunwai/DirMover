<script setup lang="ts">
import { computed, ref, shallowRef, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import BaseButton from "@/components/base/BaseButton.vue";
import BaseDialog from "@/components/base/BaseDialog.vue";
import TaskProgress from "@/components/business/TaskProgress.vue";
import { mergeTaskState } from "@/utils/taskState";
import MoveTable from "@/components/business/MoveTable.vue";
import { api } from "@/services/api";
import { normalizeAppError } from "@/services/tauri";
import type { BootstrapData, MoveRow, TaskDescriptor, TaskEvent } from "@/types/domain";
import { copyText } from "@/utils/format";

const props = defineProps<{ bootstrap: BootstrapData }>();
const emit = defineEmits<{
  status: [message: string];
  busy: [busy: boolean];
  refreshData: [];
  notify: [message: string, tone?: "info" | "success" | "warning" | "error"];
}>();

const rows = shallowRef<MoveRow[]>(props.bootstrap.moves);
const selected = ref<MoveRow | null>(null);
const activeTask = shallowRef<TaskDescriptor | null>(null);
const confirmRestoreOpen = ref(false);
const pendingAction = ref<"refresh" | "restore" | "recover" | null>(null);
const cancelling = ref(false);
let rowsRevision = 0;

watch(
  () => props.bootstrap.moves,
  (moves) => {
    rowsRevision += 1;
    rows.value = moves;
    if (selected.value) selected.value = moves.find((move) => move.id === selected.value?.id) ?? null;
  },
);

const taskActive = computed(() =>
  ["queued", "running"].includes(activeTask.value?.state ?? ""),
);
const busy = computed(() => pendingAction.value !== null || taskActive.value);
watch(busy, (value) => emit("busy", value), { flush: "sync" });

const refresh = async (): Promise<void> => {
  if (busy.value) return;
  pendingAction.value = "refresh";
  const revision = rowsRevision;
  emit("status", "正在检查迁移记录…");
  try {
    const moves = await api.listMoves();
    if (revision !== rowsRevision) return;
    rows.value = moves;
    if (selected.value) selected.value = moves.find((move) => move.id === selected.value?.id) ?? null;
    emit("status", `已检查 ${rows.value.length} 条迁移记录`);
  } catch (error: unknown) {
    notifyError(error);
  } finally {
    pendingAction.value = null;
  }
};

const requestRestore = (): void => {
  if (busy.value) return;
  if (!selected.value) {
    emit("notify", "请先选择一条迁移记录。", "warning");
    return;
  }
  confirmRestoreOpen.value = true;
};

const restore = async (): Promise<void> => {
  const row = selected.value;
  if (!row || busy.value) return;
  pendingAction.value = "restore";
  try {
    if (!props.bootstrap.isAdministrator) {
      await elevate();
      return;
    }
    confirmRestoreOpen.value = false;
    activeTask.value = await api.startRestore(row.id, onTaskEvent);
  } catch (error: unknown) {
    notifyError(error);
  } finally {
    pendingAction.value = null;
  }
};

const recover = async (): Promise<void> => {
  const row = selected.value;
  if (!row || busy.value) return;
  pendingAction.value = "recover";
  try {
    const message = await api.recoverMove(row.id);
    emit("notify", message, "success");
  } catch (error: unknown) {
    notifyError(error);
  } finally {
    emit("refreshData");
    pendingAction.value = null;
  }
};

const onTaskEvent = (event: TaskEvent): void => {
  activeTask.value = mergeTaskState(activeTask.value ?? undefined, event.task);
  emit("status", event.task.message);
  if (event.type === "progress") return;
  if (event.task.runningAtUtc !== null) emit("refreshData");
  if (event.type === "completed") {
    emit("notify", event.summary, "success");
    selected.value = null;
  } else if (event.type === "failed") {
    emit("notify", event.message, "error");
  } else {
    emit("notify", "还原任务已取消。", "warning");
  }
};

const cancel = async (): Promise<void> => {
  if (!activeTask.value?.canCancel || cancelling.value) return;
  const id = activeTask.value.id;
  cancelling.value = true;
  try {
    const result = await api.cancelTask(id);
    if (activeTask.value?.id !== id) return;
    activeTask.value = mergeTaskState(activeTask.value, result);
    emit("status", activeTask.value.message);
  } catch (error: unknown) { notifyError(error); }
  finally { cancelling.value = false; }
};

const openMove = async (row: MoveRow): Promise<void> => {
  try {
    await api.openPath(row.source);
  } catch {
    await api.openPath(row.destination);
  }
};

const copyMove = async (row: MoveRow): Promise<void> => {
  await copyText(`原位置：${row.source}\n新位置：${row.destination}`);
  emit("notify", "迁移信息已复制。", "success");
};

const openStore = async (): Promise<void> => {
  try {
    await api.openPath(props.bootstrap.dataDirectory);
  } catch (error: unknown) {
    notifyError(error);
  }
};

const openDiagnostics = async (): Promise<void> => {
  try { await api.openDiagnostics(); }
  catch (error: unknown) { notifyError(error); }
};

const elevate = async (): Promise<void> => {
  try {
    if (await api.requestElevation(null)) {
      await getCurrentWindow().close();
    }
  } catch (error: unknown) {
    notifyError(error);
  }
};

const notifyError = (error: unknown): void => {
  emit("notify", normalizeAppError(error).message, "error");
  emit("status", "操作未完成");
};
</script>

<template>
  <section class="view">
    <header class="page-header">
      <h1>迁移记录</h1>
      <span>{{ rows.length }} 条记录</span>
    </header>

    <div class="toolbar">
      <BaseButton :disabled="busy" :loading="pendingAction === 'refresh'" @click="refresh">重新检查</BaseButton>
      <BaseButton
        variant="primary"
        :disabled="busy || !selected || !selected.status.startsWith('有效')"
        @click="requestRestore"
      >
        还原选中项
      </BaseButton>
      <BaseButton
        :disabled="busy || !selected?.recoveryAvailable"
        :loading="pendingAction === 'recover'"
        @click="recover"
      >
        恢复中断操作
      </BaseButton>
      <BaseButton :disabled="busy" @click="openStore">打开记录位置</BaseButton>
      <BaseButton @click="openDiagnostics">诊断日志</BaseButton>
      <BaseButton v-if="taskActive" :disabled="!activeTask?.canCancel || cancelling" @click="cancel">{{ activeTask?.canCancel ? "停止" : activeTask?.cancelRequested ? "正在停止" : "安全收尾中" }}</BaseButton>
    </div>

    <div v-if="activeTask?.state === 'queued' || activeTask?.state === 'running'" class="task-progress">
      <TaskProgress :task="activeTask" />
    </div>

    <section class="content-panel">
      <MoveTable
        :rows="rows"
        :selected-id="selected?.id ?? null"
        @select="selected = $event"
        @open="openMove"
        @copy="copyMove"
      />
    </section>
  </section>

  <BaseDialog
    :open="confirmRestoreOpen"
    title="确认还原"
    description="数据复制和校验完成后，将恢复原目录并清理新位置副本"
    @close="confirmRestoreOpen = false"
  >
    <p class="dialog-text">还原需要原磁盘有足够空间，请先关闭正在使用目录的程序。</p>
    <dl>
      <dt>原位置</dt>
      <dd>{{ selected?.source }}</dd>
      <dt>当前数据</dt>
      <dd>{{ selected?.destination }}</dd>
    </dl>
    <template #footer>
      <BaseButton @click="confirmRestoreOpen = false">取消</BaseButton>
      <BaseButton variant="primary" :disabled="busy" :loading="pendingAction === 'restore'" @click="restore">开始还原</BaseButton>
    </template>
  </BaseDialog>
</template>

<style scoped>
.view {
  display: grid;
  height: 100%;
  min-height: 0;
  grid-template-rows: auto auto auto minmax(0, 1fr);
  gap: var(--space-3);
  padding: var(--space-4) var(--space-5);
}

.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 52px;
}

h1 {
  font-size: var(--font-size-title);
  font-weight: 650;
}

.page-header p,
.page-header > span {
  color: var(--text-secondary);
  font-size: var(--font-size-small);
}

.toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.task-progress {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-medium);
  background: var(--accent-subtle);
}

.task-progress > div {
  display: flex;
  justify-content: space-between;
  color: var(--text-secondary);
  font-size: var(--font-size-small);
}

.content-panel {
  min-height: 0;
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-medium);
  background: var(--surface-primary);
}

.dialog-text {
  color: var(--text-secondary);
}

dl {
  display: grid;
  grid-template-columns: 84px minmax(0, 1fr);
  gap: var(--space-2);
  margin: var(--space-4) 0 0;
}

dt {
  color: var(--text-secondary);
}

dd {
  margin: 0;
  overflow-wrap: anywhere;
  font-family: "Cascadia Mono", Consolas, monospace;
  font-size: 12px;
}
</style>
