<script setup lang="ts">
import { computed, onMounted, ref, shallowRef } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import BaseButton from "@/components/base/BaseButton.vue";
import BaseDialog from "@/components/base/BaseDialog.vue";
import BaseProgress from "@/components/base/BaseProgress.vue";
import FolderTable from "@/components/business/FolderTable.vue";
import MigrationDestinationDialog from "@/components/business/MigrationDestinationDialog.vue";
import { api } from "@/services/api";
import { normalizeAppError } from "@/services/tauri";
import type {
  BootstrapData,
  FolderRow,
  MigrationPlan,
  ScanPage,
  SortDirection,
  SortField,
  TaskDescriptor,
  TaskEvent,
} from "@/types/domain";
import { copyText, formatBytes, formatInteger } from "@/utils/format";

const props = defineProps<{
  bootstrap: BootstrapData;
}>();

const emit = defineEmits<{
  status: [message: string];
  busy: [busy: boolean];
  refreshData: [];
  notify: [message: string, tone?: "info" | "success" | "warning" | "error"];
}>();

const root = ref(props.bootstrap.startupScanRoot ?? props.bootstrap.defaultScanRoot);
const rows = shallowRef<FolderRow[]>([]);
const selected = ref<FolderRow | null>(null);
const scanPage = shallowRef<ScanPage | null>(null);
const activeTask = shallowRef<TaskDescriptor | null>(null);
const sortField = ref<SortField>("allocatedBytes");
const sortDirection = ref<SortDirection>("descending");
const loadingPreview = ref(false);
const elevationOpen = ref(false);
const migrationOpen = ref(false);
const migrationMode = ref<"managed" | "custom">("managed");
const managedPlan = shallowRef<MigrationPlan | null>(null);
const customPlan = shallowRef<MigrationPlan | null>(null);
const pendingPlan = shallowRef<MigrationPlan | null>(null);
const confirmMigrationOpen = ref(false);

const isBusy = computed(
  () => loadingPreview.value || activeTask.value?.state === "running",
);
const selectedPath = computed(() => selected.value?.fullPath ?? null);
const folderCount = computed(() => scanPage.value?.totalItems ?? rows.value.length);
const totalLogical = computed(() =>
  scanPage.value ? formatBytes(scanPage.value.totalBytes) : "未扫描",
);
const totalAllocated = computed(() =>
  scanPage.value ? formatBytes(scanPage.value.totalAllocatedBytes) : "未扫描",
);
const engineLabel = computed(() => scanPage.value?.engine ?? "目录预览");

const currentPreset = computed(() => {
  const match = props.bootstrap.scanPresets.find(
    (preset) => preset.path.toLowerCase() === root.value.toLowerCase(),
  );
  return match?.key ?? "custom";
});

onMounted(async () => {
  await loadPreview();
  if (props.bootstrap.startupScanRoot && props.bootstrap.isAdministrator) {
    await startScan(false);
  }
});

const chooseRoot = async (): Promise<void> => {
  const selectedRoot = await api.chooseDirectory("选择要分析的目录", root.value);
  if (!selectedRoot) return;
  root.value = selectedRoot;
  await loadPreview();
};

const changePreset = async (event: Event): Promise<void> => {
  const key = (event.target as HTMLSelectElement).value;
  if (key === "custom") {
    await chooseRoot();
    return;
  }
  const preset = props.bootstrap.scanPresets.find((item) => item.key === key);
  if (!preset) return;
  root.value = preset.path;
  await loadPreview();
};

const loadPreview = async (): Promise<void> => {
  if (!root.value) return;
  loadingPreview.value = true;
  emit("busy", true);
  emit("status", `正在列出 ${root.value} 的一级文件夹…`);
  selected.value = null;
  scanPage.value = null;
  try {
    rows.value = await api.preview(root.value);
    emit("status", `已列出 ${formatInteger(rows.value.length)} 个一级文件夹`);
  } catch (error: unknown) {
    notifyError(error);
    rows.value = [];
  } finally {
    loadingPreview.value = false;
    emit("busy", false);
  }
};

const requestScan = async (force: boolean): Promise<void> => {
  if (!props.bootstrap.isAdministrator) {
    elevationOpen.value = true;
    return;
  }
  await startScan(force);
};

const startScan = async (force: boolean): Promise<void> => {
  emit("busy", true);
  emit("status", "正在准备扫描…");
  try {
    activeTask.value = await api.startScan(root.value, force, onTaskEvent);
  } catch (error: unknown) {
    emit("busy", false);
    notifyError(error);
  }
};

const onTaskEvent = (event: TaskEvent): void => {
  activeTask.value = event.task;
  emit("status", event.task.message);
  if (event.type === "progress") return;
  emit("busy", false);
  if (event.type === "completed") {
    if (event.task.kind === "scan" && event.task.resultId) {
      void loadScanPage(event.task.resultId);
    } else if (event.task.kind === "migration") {
      migrationOpen.value = false;
      confirmMigrationOpen.value = false;
      emit("notify", event.summary, "success");
      emit("refreshData");
      void startScan(true);
    }
  } else if (event.type === "failed") {
    emit("notify", event.message, "error");
  } else {
    emit("notify", "任务已取消。", "warning");
  }
};

const loadScanPage = async (resultId: string): Promise<void> => {
  try {
    scanPage.value = await api.scanPage(resultId, {
      page: 1,
      pageSize: 500,
      sortField: sortField.value,
      sortDirection: sortDirection.value,
    });
    rows.value = scanPage.value.items;
    selected.value = null;
    emit(
      "status",
      `扫描完成（${scanPage.value.engine}）：${formatInteger(scanPage.value.totalItems)} 个文件夹`,
    );
  } catch (error: unknown) {
    notifyError(error);
  }
};

const cancelTask = async (): Promise<void> => {
  if (!activeTask.value) return;
  try {
    await api.cancelTask(activeTask.value.id);
    emit("status", "正在安全停止任务…");
  } catch (error: unknown) {
    notifyError(error);
  }
};

const changeSort = async (field: SortField): Promise<void> => {
  if (sortField.value === field) {
    sortDirection.value =
      sortDirection.value === "ascending" ? "descending" : "ascending";
  } else {
    sortField.value = field;
    sortDirection.value = field === "name" ? "ascending" : "descending";
  }
  if (scanPage.value) {
    await loadScanPage(scanPage.value.resultId);
  } else {
    rows.value = [...rows.value].sort((left, right) =>
      left.name.localeCompare(right.name, "zh-CN"),
    );
  }
};

const openFolder = async (row: FolderRow): Promise<void> => {
  try {
    await api.openPath(row.fullPath);
  } catch (error: unknown) {
    notifyError(error);
  }
};

const copyFolder = async (row: FolderRow): Promise<void> => {
  await copyText(row.fullPath);
  emit("notify", "完整路径已复制。", "success");
};

const prepareMigration = async (): Promise<void> => {
  const row = selected.value;
  if (!row) {
    emit("notify", "请先选择一个文件夹。", "warning");
    return;
  }
  if (!row.measured) {
    emit("notify", "请先完成扫描，再迁移该目录。", "warning");
    return;
  }
  if (row.isLink || row.linkCount > 0) {
    emit("notify", "目录本身或内部含有链接，安全检查未通过。", "warning");
    return;
  }
  if (!props.bootstrap.isAdministrator) {
    elevationOpen.value = true;
    return;
  }
  try {
    managedPlan.value = await api.planMigration(row.fullPath, null);
    customPlan.value = null;
    migrationMode.value = managedPlan.value.available ? "managed" : "custom";
    migrationOpen.value = true;
  } catch (error: unknown) {
    notifyError(error);
  }
};

const chooseCustomDestination = async (): Promise<void> => {
  const customRoot = await api.chooseDirectory("选择本次迁移的托管根目录");
  if (!customRoot || !selected.value) return;
  try {
    customPlan.value = await api.planMigration(
      selected.value.fullPath,
      customRoot,
    );
    migrationMode.value = "custom";
  } catch (error: unknown) {
    notifyError(error);
  }
};

const confirmMigration = (plan: MigrationPlan): void => {
  pendingPlan.value = plan;
  confirmMigrationOpen.value = true;
};

const executeMigration = async (): Promise<void> => {
  const plan = pendingPlan.value;
  if (!plan?.destination) return;
  confirmMigrationOpen.value = false;
  emit("busy", true);
  emit("status", "正在准备迁移…");
  try {
    activeTask.value = await api.startMigration(
      plan.source,
      plan.destination,
      onTaskEvent,
    );
  } catch (error: unknown) {
    emit("busy", false);
    notifyError(error);
  }
};

const elevate = async (): Promise<void> => {
  try {
    const launched = await api.requestElevation(root.value);
    elevationOpen.value = false;
    if (launched) {
      await getCurrentWindow().close();
    }
  } catch (error: unknown) {
    notifyError(error);
  }
};

const notifyError = (error: unknown): void => {
  const normalized = normalizeAppError(error);
  emit("notify", normalized.message, "error");
  emit("status", "操作未完成");
};
</script>

<template>
  <section class="view">
    <header class="page-header">
      <div>
        <h1>空间分析</h1>
        <p>查看一级目录占用，并将选中的目录安全迁移到其他 NTFS 分区</p>
      </div>
      <span class="engine-badge">{{ engineLabel }}</span>
    </header>

    <div class="toolbar">
      <select :value="currentPreset" :disabled="isBusy" @change="changePreset">
        <option
          v-for="preset in bootstrap.scanPresets"
          :key="preset.key"
          :value="preset.key"
        >
          {{ preset.label }}
        </option>
        <option value="custom">自选目录…</option>
      </select>
      <div class="path-picker" :class="{ disabled: isBusy }">
        <input
          class="path-display"
          type="text"
          :value="root"
          :title="root"
          readonly
          aria-label="当前分析目录"
        />
        <button
          type="button"
          title="选择分析目录"
          aria-label="选择分析目录"
          :disabled="isBusy"
          @click="chooseRoot"
        >
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path
              d="M2.75 5.25A1.75 1.75 0 0 1 4.5 3.5h3.1l1.45 1.6h6.45a1.75 1.75 0 0 1 1.75 1.75v7.4A1.75 1.75 0 0 1 15.5 16H4.5a1.75 1.75 0 0 1-1.75-1.75v-9Z"
            />
          </svg>
        </button>
      </div>
      <BaseButton
        variant="primary"
        :disabled="isBusy"
        @click="requestScan(false)"
      >
        扫描
      </BaseButton>
      <BaseButton :disabled="isBusy" @click="requestScan(true)">
        重新扫描
      </BaseButton>
      <BaseButton
        v-if="isBusy && activeTask?.state === 'running'"
        @click="cancelTask"
      >
        停止
      </BaseButton>
      <BaseButton
        variant="primary"
        :disabled="isBusy || !selected"
        @click="prepareMigration"
      >
        迁移选中项
      </BaseButton>
    </div>

    <div v-if="activeTask?.state === 'running'" class="task-progress">
      <div>
        <span>{{ activeTask.message }}</span>
        <strong>{{ activeTask.percentage }}%</strong>
      </div>
      <BaseProgress :value="activeTask.percentage" />
    </div>

    <div class="summary-strip">
      <div>
        <span>一级文件夹</span>
        <strong>{{ formatInteger(folderCount) }}</strong>
      </div>
      <div>
        <span>逻辑大小</span>
        <strong>{{ totalLogical }}</strong>
      </div>
      <div>
        <span>磁盘占用</span>
        <strong class="accent">{{ totalAllocated }}</strong>
      </div>
      <div v-if="scanPage">
        <span>文件总数</span>
        <strong>{{ formatInteger(scanPage.totalFiles) }}</strong>
      </div>
    </div>

    <section class="content-panel">
      <header>
        <div>
          <strong>目录明细</strong>
          <span>双击打开，右键可复制路径</span>
        </div>
        <span v-if="loadingPreview">正在读取…</span>
      </header>
      <FolderTable
        :rows="rows"
        :selected-path="selectedPath"
        :sort-field="sortField"
        :sort-direction="sortDirection"
        :total-allocated-bytes="scanPage?.totalAllocatedBytes ?? 0"
        @select="selected = $event"
        @sort="changeSort"
        @open="openFolder"
        @copy="copyFolder"
      />
    </section>
  </section>

  <MigrationDestinationDialog
    :open="migrationOpen"
    :source="selected?.fullPath ?? ''"
    :mode="migrationMode"
    :managed-plan="managedPlan"
    :custom-plan="customPlan"
    :busy="isBusy"
    @close="migrationOpen = false"
    @update:mode="migrationMode = $event"
    @choose-custom="chooseCustomDestination"
    @confirm="confirmMigration"
  />

  <BaseDialog
    :open="confirmMigrationOpen"
    title="确认迁移"
    description="复制和校验完成后，原位置会变成目录联接"
    @close="confirmMigrationOpen = false"
  >
    <div class="confirmation">
      <p>迁移前请关闭正在使用此目录的程序和服务。</p>
      <dl>
        <dt>原位置</dt>
        <dd>{{ pendingPlan?.source }}</dd>
        <dt>新位置</dt>
        <dd>{{ pendingPlan?.destination }}</dd>
      </dl>
    </div>
    <template #footer>
      <BaseButton @click="confirmMigrationOpen = false">取消</BaseButton>
      <BaseButton variant="primary" @click="executeMigration">
        开始迁移
      </BaseButton>
    </template>
  </BaseDialog>

  <BaseDialog
    :open="elevationOpen"
    title="需要管理员权限"
    description="读取 NTFS MFT 和迁移受保护目录需要提升权限"
    @close="elevationOpen = false"
  >
    <p class="dialog-text">
      DirMover 将以管理员权限重新启动，并自动恢复当前扫描位置。确认 UAC 后即可继续。
    </p>
    <template #footer>
      <BaseButton @click="elevationOpen = false">取消</BaseButton>
      <BaseButton variant="primary" @click="elevate">重新启动</BaseButton>
    </template>
  </BaseDialog>
</template>

<style scoped>
.view {
  display: grid;
  height: 100%;
  min-height: 0;
  grid-template-rows: auto auto auto auto minmax(0, 1fr);
  gap: var(--space-3);
  padding: var(--space-4) var(--space-5);
}

.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 52px;
}

.page-header h1 {
  font-size: var(--font-size-title);
  font-weight: 650;
}

.page-header p {
  margin-top: 2px;
  color: var(--text-secondary);
  font-size: var(--font-size-small);
}

.engine-badge {
  padding: 3px var(--space-2);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
  color: var(--text-secondary);
  background: var(--surface-secondary);
  font-size: 11px;
}

.toolbar {
  display: grid;
  grid-template-columns: 168px minmax(180px, 1fr) auto auto auto auto;
  gap: var(--space-2);
  align-items: center;
}

select,
.path-picker {
  height: var(--control-height);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-small);
  color: var(--text-primary);
  background: var(--surface-primary);
}

select {
  padding: 0 var(--space-2);
}

.path-picker {
  display: grid;
  min-width: 0;
  grid-template-columns: minmax(0, 1fr) var(--control-height);
  overflow: hidden;
}

.path-display {
  min-width: 0;
  border: 0;
  outline: 0;
  overflow: hidden;
  padding: 0 var(--space-3);
  color: var(--text-primary);
  background: transparent;
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.path-picker > button {
  display: grid;
  width: var(--control-height);
  height: 100%;
  place-items: center;
  border-left: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  background: var(--surface-secondary);
}

.path-picker > button:hover:not(:disabled) {
  color: var(--accent);
  background: var(--surface-hover);
}

.path-picker > button:disabled {
  color: var(--text-disabled);
}

.path-picker svg {
  width: 17px;
  height: 17px;
  fill: none;
  stroke: currentColor;
  stroke-linecap: round;
  stroke-linejoin: round;
  stroke-width: 1.5;
}

.path-picker.disabled {
  opacity: 0.7;
}

.task-progress {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1px solid color-mix(in srgb, var(--accent) 38%, var(--border-subtle));
  border-radius: var(--radius-medium);
  background: var(--accent-subtle);
}

.task-progress > div {
  display: flex;
  justify-content: space-between;
  color: var(--text-secondary);
  font-size: var(--font-size-small);
}

.task-progress strong {
  color: var(--accent);
  font-variant-numeric: tabular-nums;
}

.summary-strip {
  display: flex;
  min-height: 58px;
  border-block: 1px solid var(--border-subtle);
  background: var(--surface-primary);
}

.summary-strip > div {
  display: grid;
  min-width: 155px;
  align-content: center;
  padding: 0 var(--space-4);
  border-right: 1px solid var(--border-subtle);
}

.summary-strip span {
  color: var(--text-secondary);
  font-size: 11px;
}

.summary-strip strong {
  margin-top: 1px;
  font-size: 16px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.summary-strip .accent {
  color: var(--accent);
}

.content-panel {
  display: grid;
  min-height: 0;
  grid-template-rows: 42px minmax(0, 1fr);
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-medium);
  background: var(--surface-primary);
}

.content-panel > header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 var(--space-3);
  border-bottom: 1px solid var(--border-subtle);
}

.content-panel > header div {
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
}

.content-panel > header strong {
  font-size: 13px;
  font-weight: 600;
}

.content-panel > header span {
  color: var(--text-secondary);
  font-size: 11px;
}

.confirmation p,
.dialog-text {
  color: var(--text-secondary);
}

dl {
  display: grid;
  grid-template-columns: 68px minmax(0, 1fr);
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
