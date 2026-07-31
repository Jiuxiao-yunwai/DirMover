<script setup lang="ts">
import { computed, ref, shallowRef, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import BaseButton from "@/components/base/BaseButton.vue";
import BaseDialog from "@/components/base/BaseDialog.vue";
import BaseProgress from "@/components/base/BaseProgress.vue";
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

watch(
  () => props.bootstrap.moves,
  (moves) => {
    rows.value = moves;
    if (selected.value && !moves.some((move) => move.id === selected.value?.id)) {
      selected.value = null;
    }
  },
);

const busy = computed(() => activeTask.value?.state === "running");

const refresh = async (): Promise<void> => {
  emit("busy", true);
  emit("status", "正在检查迁移记录…");
  try {
    rows.value = await api.listMoves();
    emit("status", `已检查 ${rows.value.length} 条迁移记录`);
  } catch (error: unknown) {
    notifyError(error);
  } finally {
    emit("busy", false);
  }
};

const requestRestore = (): void => {
  if (!selected.value) {
    emit("notify", "请先选择一条迁移记录。", "warning");
    return;
  }
  confirmRestoreOpen.value = true;
};

const restore = async (): Promise<void> => {
  const row = selected.value;
  if (!row) return;
  if (!props.bootstrap.isAdministrator) {
    await elevate();
    return;
  }
  confirmRestoreOpen.value = false;
  emit("busy", true);
  try {
    activeTask.value = await api.startRestore(row.id, onTaskEvent);
  } catch (error: unknown) {
    emit("busy", false);
    notifyError(error);
  }
};

const recover = async (): Promise<void> => {
  const row = selected.value;
  if (!row) return;
  emit("busy", true);
  try {
    const message = await api.recoverMove(row.id);
    emit("notify", message, "success");
    emit("refreshData");
    await refresh();
  } catch (error: unknown) {
    notifyError(error);
  } finally {
    emit("busy", false);
  }
};

const onTaskEvent = (event: TaskEvent): void => {
  activeTask.value = event.task;
  emit("status", event.task.message);
  if (event.type === "progress") return;
  emit("busy", false);
  if (event.type === "completed") {
    emit("notify", event.summary, "success");
    selected.value = null;
    emit("refreshData");
    void refresh();
  } else if (event.type === "failed") {
    emit("notify", event.message, "error");
  } else {
    emit("notify", "还原任务已取消。", "warning");
  }
};

const cancel = async (): Promise<void> => {
  if (!activeTask.value) return;
  await api.cancelTask(activeTask.value.id);
  emit("status", "正在安全停止还原…");
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
      <div>
        <h1>迁移记录</h1>
        <p>核对目录联接、处理异常中断，并将数据还原到原磁盘</p>
      </div>
      <span>{{ rows.length }} 条记录</span>
    </header>

    <div class="toolbar">
      <BaseButton :disabled="busy" @click="refresh">重新检查</BaseButton>
      <BaseButton
        variant="primary"
        :disabled="busy || !selected || !selected.status.startsWith('有效')"
        @click="requestRestore"
      >
        还原选中项
      </BaseButton>
      <BaseButton
        :disabled="busy || !selected?.recoveryAvailable"
        @click="recover"
      >
        恢复中断操作
      </BaseButton>
      <BaseButton :disabled="busy" @click="openStore">打开记录位置</BaseButton>
      <BaseButton v-if="busy" @click="cancel">停止</BaseButton>
    </div>

    <div v-if="activeTask?.state === 'running'" class="task-progress">
      <div>
        <span>{{ activeTask.message }}</span>
        <strong>{{ activeTask.percentage }}%</strong>
      </div>
      <BaseProgress :value="activeTask.percentage" />
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
      <BaseButton variant="primary" @click="restore">开始还原</BaseButton>
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
