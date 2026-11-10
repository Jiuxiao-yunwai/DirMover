<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowReactive, shallowRef, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import BaseButton from "@/components/base/BaseButton.vue";
import BaseDialog from "@/components/base/BaseDialog.vue";
import TaskProgress from "@/components/business/TaskProgress.vue";
import FolderTable from "@/components/business/FolderTable.vue";
import MigrationDestinationDialog from "@/components/business/MigrationDestinationDialog.vue";
import { api } from "@/services/api";
import { createScanSessionCache } from "@/services/scanSessionCache";
import { normalizeAppError } from "@/services/tauri";
import { formatElapsedTime, mergeTaskState, taskElapsedMs } from "@/utils/taskState";
import { sortFolderRows } from "@/utils/folderSort";
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

type MigrationQueueEntry = {
  plan: MigrationPlan;
  task: TaskDescriptor;
};

const props = withDefaults(defineProps<{ bootstrap: BootstrapData; dataRevision?: number }>(), {
  dataRevision: 0,
});
const emit = defineEmits<{
  status: [message: string];
  busy: [busy: boolean];
  refreshData: [];
  notify: [message: string, tone?: "info" | "success" | "warning" | "error"];
}>();

const root = ref(props.bootstrap.startupScanRoot ?? props.bootstrap.defaultScanRoot);
const rows = shallowRef<FolderRow[]>([]);
const selectedPaths = ref<string[]>([]);
const selectedRowCache = shallowReactive(new Map<string, FolderRow>());
const previewPage = ref(1);
const loadingPage = ref(false);
let listRequestId = 0;
const pageSize = 500;
const scanPage = shallowRef<ScanPage | null>(null);
const scanTask = shallowRef<TaskDescriptor | null>(null);
const migrationEntries = shallowRef<MigrationQueueEntry[]>([]);
const pendingMigrationEvents = new Map<string, TaskEvent>();
const migrationQueueCollapsed = ref(false);
const sortField = ref<SortField>("name");
const sortDirection = ref<SortDirection>("ascending");
const loadingPreview = ref(false);
const startingScan = ref(false);
const cancellingTasks = shallowReactive(new Set<string>());
const planningMigration = ref(false);
const submittingMigration = ref(false);
const elevationOpen = ref(false);
const migrationOpen = ref(false);
const migrationMode = ref<"managed" | "custom">("managed");
const managedPlans = shallowRef<MigrationPlan[]>([]);
const customPlans = shallowRef<MigrationPlan[]>([]);
const pendingPlans = shallowRef<MigrationPlan[]>([]);
const confirmMigrationOpen = ref(false);
const scanCache = createScanSessionCache();

const isScanBusy = computed(
  () => startingScan.value || loadingPreview.value || loadingPage.value || ["queued", "running"].includes(scanTask.value?.state ?? ""),
);
const selectedRows = computed(() => selectedPaths.value
  .map((path) => selectedRowCache.get(path))
  .filter((row): row is FolderRow => row !== undefined));
const activeMigrationCount = computed(
  () => migrationEntries.value.filter(({ task }) => ["queued", "running"].includes(task.state)).length,
);
const folderCount = computed(() => scanPage.value?.totalItems ?? rows.value.length);
const currentPage = computed(() => scanPage.value?.page ?? previewPage.value);
const pageCount = computed(() => Math.max(1, Math.ceil(folderCount.value / pageSize)));
const visibleRows = computed(() => scanPage.value
  ? rows.value
  : rows.value.slice((previewPage.value - 1) * pageSize, previewPage.value * pageSize));
const totalLogical = computed(() => scanPage.value ? formatBytes(scanPage.value.totalBytes) : "未扫描");
const totalAllocated = computed(() => scanPage.value ? formatBytes(scanPage.value.totalAllocatedBytes) : "未扫描");
watch(() => props.dataRevision, () => {
  scanCache.clear();
  scanPage.value = null;
  previewPage.value = 1;
  void loadPreview(true);
});

const currentPreset = computed(() => {
  const match = props.bootstrap.scanPresets.find(
    (preset) => preset.path.toLowerCase() === root.value.toLowerCase(),
  );
  return match?.key ?? "custom";
});

onMounted(async () => {
  await loadPreview();
  if (props.bootstrap.startupScanRoot && props.bootstrap.isAdministrator) await startScan(false);
});

onBeforeUnmount(() => {
  listRequestId += 1;
});

const chooseRoot = async (): Promise<void> => {
  const selectedRoot = await api.chooseDirectory("选择要分析的目录", root.value);
  if (!selectedRoot) return;
  root.value = selectedRoot;
  await loadPreview();
};

const changePreset = async (event: Event): Promise<void> => {
  const key = (event.target as HTMLSelectElement).value;
  if (key === "custom") return chooseRoot();
  const preset = props.bootstrap.scanPresets.find((item) => item.key === key);
  if (!preset) return;
  root.value = preset.path;
  await loadPreview();
};

const clearSelection = (): void => {
  selectedPaths.value = [];
  selectedRowCache.clear();
};

const loadPreview = async (preserveSelection = false): Promise<void> => {
  if (!root.value) return;
  const requestId = ++listRequestId;
  const requestedRoot = root.value;
  loadingPage.value = false;
  loadingPreview.value = false;
  previewPage.value = 1;
  if (!preserveSelection) clearSelection();
  const cached = scanCache.get(requestedRoot);
  if (cached) {
    sortField.value = cached.sortField;
    sortDirection.value = cached.sortDirection;
    scanPage.value = cached.page;
    rows.value = cached.page.items;
    emit("busy", false);
    emit("status", `已恢复扫描缓存（${cached.page.engine}）：${formatInteger(cached.page.totalItems)} 个文件夹`);
    return;
  }
  loadingPreview.value = true;
  emit("busy", true);
  emit("status", `正在列出 ${root.value} 的一级文件夹…`);
  scanPage.value = null;
  try {
    const preview = await api.preview(requestedRoot);
    if (requestId !== listRequestId) return;
    rows.value = sortFolderRows(
      preview,
      sortField.value,
      sortDirection.value,
    );
    if (preserveSelection) {
      const fresh = new Map(preview.filter((row) => !row.isLink).map((row) => [row.fullPath, row]));
      selectedPaths.value = selectedPaths.value.filter((path) => fresh.has(path));
      selectedRowCache.clear();
      for (const path of selectedPaths.value) selectedRowCache.set(path, fresh.get(path)!);
    }
    emit("status", `已列出 ${formatInteger(rows.value.length)} 个一级文件夹`);
  } catch (error: unknown) {
    if (requestId !== listRequestId) return;
    notifyError(error);
    rows.value = [];
  } finally {
    if (requestId === listRequestId) {
      loadingPreview.value = false;
      emit("busy", scanTask.value?.state === "running");
    }
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
  if (isScanBusy.value) return;
  startingScan.value = true;
  emit("busy", true);
  emit("status", "正在准备扫描…");
  try {
    scanTask.value = await api.startScan(root.value, force, onScanEvent);
  } catch (error: unknown) {
    notifyError(error);
  } finally {
    startingScan.value = false;
    emit("busy", isScanBusy.value);
  }
};

const onScanEvent = (event: TaskEvent): void => {
  scanTask.value = mergeTaskState(scanTask.value ?? undefined, event.task);
  emit("status", event.task.message);
  if (event.type === "progress") return;
  emit("busy", false);
  if (event.type === "completed" && event.task.resultId) {
    void loadScanPage(event.task.resultId);
  } else if (event.type === "failed") {
    emit("notify", event.message, "error");
  } else if (event.type === "cancelled") {
    emit("notify", "扫描已取消。", "warning");
  }
};

const loadScanPage = async (resultId: string, requestedPage = 1): Promise<void> => {
  const requestId = ++listRequestId;
  const requestedSort = sortField.value;
  const requestedDirection = sortDirection.value;
  loadingPreview.value = false;
  loadingPage.value = true;
  try {
    const page = await api.scanPage(resultId, {
      page: requestedPage,
      pageSize,
      sortField: requestedSort,
      sortDirection: requestedDirection,
    });
    if (requestId !== listRequestId) return;
    if (scanPage.value?.resultId !== resultId) clearSelection();
    scanPage.value = page;
    rows.value = page.items;
    const selected = new Set(selectedPaths.value);
    for (const row of page.items) {
      if (selected.has(row.fullPath)) selectedRowCache.set(row.fullPath, row);
    }
    scanCache.set(page.root, { page, sortField: requestedSort, sortDirection: requestedDirection });
    emit("status", `扫描完成（${page.engine}）：${formatInteger(page.totalItems)} 个文件夹`);
  } catch (error: unknown) {
    if (requestId === listRequestId) notifyError(error);
  } finally {
    if (requestId === listRequestId) loadingPage.value = false;
  }
};

const changePage = async (page: number): Promise<void> => {
  if (isScanBusy.value) return;
  const target = Math.max(1, Math.min(pageCount.value, page));
  if (scanPage.value) await loadScanPage(scanPage.value.resultId, target);
  else previewPage.value = target;
};

const cancelScan = async (): Promise<void> => {
  if (!scanTask.value?.canCancel || cancellingTasks.has(scanTask.value.id)) return;
  const id = scanTask.value.id;
  cancellingTasks.add(id);
  try {
    const result = await api.cancelTask(id);
    if (scanTask.value?.id === id) scanTask.value = mergeTaskState(scanTask.value, result);
    emit("status", scanTask.value?.message ?? result.message);
  } catch (error: unknown) {
    notifyError(error);
  } finally {
    cancellingTasks.delete(id);
  }
};

const toggleSelection = (row: FolderRow): void => {
  if (selectedPaths.value.includes(row.fullPath)) {
    selectedRowCache.delete(row.fullPath);
    selectedPaths.value = selectedPaths.value.filter((path) => path !== row.fullPath);
  } else {
    selectedRowCache.set(row.fullPath, row);
    selectedPaths.value = [...selectedPaths.value, row.fullPath];
  }
};

const toggleAllVisible = (): void => {
  const visible = visibleRows.value.map((row) => row.fullPath);
  const selected = new Set(selectedPaths.value);
  const allSelected = visible.length > 0 && visible.every((path) => selected.has(path));
  for (const row of visibleRows.value) {
    if (allSelected) selectedRowCache.delete(row.fullPath);
    else selectedRowCache.set(row.fullPath, row);
  }
  const visiblePaths = new Set(visible);
  selectedPaths.value = allSelected
    ? selectedPaths.value.filter((path) => !visiblePaths.has(path))
    : [...new Set([...selectedPaths.value, ...visible])];
};

const changeSort = async (field: SortField): Promise<void> => {
  if (sortField.value === field) {
    sortDirection.value = sortDirection.value === "ascending" ? "descending" : "ascending";
  } else {
    sortField.value = field;
    sortDirection.value = field === "name" ? "ascending" : "descending";
  }
  if (scanPage.value) await loadScanPage(scanPage.value.resultId);
  else {
    rows.value = sortFolderRows(rows.value, sortField.value, sortDirection.value);
    previewPage.value = 1;
  }
};

const openFolder = async (row: FolderRow): Promise<void> => {
  try { await api.openPath(row.fullPath); } catch (error: unknown) { notifyError(error); }
};

const copyFolder = async (row: FolderRow): Promise<void> => {
  await copyText(row.fullPath);
  emit("notify", "完整路径已复制。", "success");
};

const prepareMigration = async (): Promise<void> => {
  const selected = selectedRows.value;
  if (!selected.length) {
    emit("notify", "请至少选择一个文件夹。", "warning");
    return;
  }
  const unsafe = selected.filter((row) => row.isLink || row.linkCount > 0);
  if (unsafe.length) {
    emit("notify", `${unsafe.length} 个目录本身或内部含有链接，安全检查未通过。`, "warning");
    return;
  }
  const activeSources = new Set(
    migrationEntries.value
      .filter(({ task }) => ["queued", "running"].includes(task.state))
      .map(({ plan }) => plan.source.toLowerCase()),
  );
  const duplicate = selected.find((row) => activeSources.has(row.fullPath.toLowerCase()));
  if (duplicate) {
    emit("notify", `${duplicate.name} 已在迁移队列中。`, "warning");
    return;
  }
  if (!props.bootstrap.isAdministrator) {
    elevationOpen.value = true;
    return;
  }
  planningMigration.value = true;
  try {
    managedPlans.value = await Promise.all(selected.map((row) => api.planMigration(row.fullPath, null)));
    customPlans.value = [];
    migrationMode.value = managedPlans.value.every((plan) => plan.available) ? "managed" : "custom";
    migrationOpen.value = true;
  } catch (error: unknown) {
    notifyError(error);
  } finally {
    planningMigration.value = false;
  }
};

const chooseCustomDestination = async (): Promise<void> => {
  const customRoot = await api.chooseDirectory("选择本次批量迁移的托管根目录");
  if (!customRoot || !managedPlans.value.length) return;
  planningMigration.value = true;
  try {
    customPlans.value = await Promise.all(
      managedPlans.value.map((plan) => api.planMigration(plan.source, customRoot)),
    );
    migrationMode.value = "custom";
  } catch (error: unknown) {
    notifyError(error);
  } finally {
    planningMigration.value = false;
  }
};

const confirmMigration = (plans: MigrationPlan[]): void => {
  pendingPlans.value = plans;
  confirmMigrationOpen.value = true;
};

const executeMigration = async (): Promise<void> => {
  const plans = pendingPlans.value;
  if (!plans.length || plans.some((plan) => !plan.destination)) return;
  submittingMigration.value = true;
  try {
    const tasks = await api.startMigrations(
      plans.map((plan) => ({ source: plan.source, destination: plan.destination! })),
      onMigrationEvent,
    );
    migrationEntries.value = [
      ...migrationEntries.value,
      ...tasks.map((task, index) => ({
        plan: plans[index],
        task,
      })),
    ];
    for (const task of tasks) {
      const pending = pendingMigrationEvents.get(task.id);
      if (pending) {
        pendingMigrationEvents.delete(task.id);
        onMigrationEvent(pending);
      }
    }
    selectedPaths.value = selectedPaths.value.filter(
      (path) => !plans.some((plan) => plan.source === path),
    );
    for (const plan of plans) selectedRowCache.delete(plan.source);
    migrationOpen.value = false;
    confirmMigrationOpen.value = false;
    emit("status", `${tasks.length} 个迁移任务已加入后台队列。`);
    emit("notify", `已加入后台队列：${tasks.length} 项。`, "success");
  } catch (error: unknown) {
    notifyError(error);
  } finally {
    submittingMigration.value = false;
  }
};

const onMigrationEvent = (event: TaskEvent): void => {
  const index = migrationEntries.value.findIndex(({ task }) => task.id === event.task.id);
  if (index < 0) {
    pendingMigrationEvents.set(event.task.id, event);
    return;
  }
  const entry = migrationEntries.value[index];
  const updated = [...migrationEntries.value];
  updated[index] = { ...entry, task: mergeTaskState(entry.task, event.task) };
  migrationEntries.value = updated;
  emit("status", event.task.message);
  if (event.type === "progress") return;
  if (event.task.runningAtUtc !== null) emit("refreshData");
  if (event.type === "completed") {
    scanCache.clear();
    rows.value = rows.value.filter((row) => row.fullPath !== entry.plan.source);
    selectedPaths.value = selectedPaths.value.filter((path) => path !== entry.plan.source);
    selectedRowCache.delete(entry.plan.source);
    emit("notify", event.summary, "success");
  } else if (event.type === "failed") {
    emit("notify", event.message, "error");
  } else {
    emit("notify", `已取消：${entry.plan.source}`, "warning");
  }
};

const cancelMigration = async (task: TaskDescriptor): Promise<void> => {
  if (!task.canCancel || cancellingTasks.has(task.id)) return;
  cancellingTasks.add(task.id);
  try {
    const cancelled = await api.cancelTask(task.id);
    const index = migrationEntries.value.findIndex(
      ({ task: entryTask }) => entryTask.id === task.id,
    );
    if (index >= 0) {
      const updated = [...migrationEntries.value];
      updated[index] = { ...updated[index], task: mergeTaskState(updated[index].task, cancelled) };
      migrationEntries.value = updated;
    }
    emit("status", index >= 0 ? migrationEntries.value[index].task.message : cancelled.message);
  } catch (error: unknown) {
    notifyError(error);
  } finally {
    cancellingTasks.delete(task.id);
  }
};

const taskStateLabel = (task: TaskDescriptor): string => ({
  queued: "排队中",
  running: "迁移中",
  completed: "已完成",
  failed: "失败",
  cancelled: "已取消",
})[task.state];

const entryTimeLabel = (entry: MigrationQueueEntry): string => {
  if (entry.task.state === "queued") return "等待前方任务完成";
  const elapsed = taskElapsedMs(entry.task);
  if (entry.task.state === "completed" && elapsed !== null) return `用时 ${formatElapsedTime(elapsed)}`;
  return entry.task.message;
};

const queueSummaryLabel = computed(() => {
  const completed = migrationEntries.value.filter(({ task }) => task.state === "completed").length;
  const failed = migrationEntries.value.filter(({ task }) => task.state === "failed").length;
  const cancelled = migrationEntries.value.filter(({ task }) => task.state === "cancelled").length;
  return `${completed} 项完成 · ${failed} 项失败 · ${cancelled} 项取消`;
});

const elevate = async (): Promise<void> => {
  try {
    const launched = await api.requestElevation(root.value);
    elevationOpen.value = false;
    if (launched) await getCurrentWindow().close();
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
      <h1>空间分析</h1>
    </header>

    <div class="toolbar">
      <select :value="currentPreset" :disabled="isScanBusy" @change="changePreset">
        <option
          v-for="preset in bootstrap.scanPresets"
          :key="preset.key"
          :value="preset.key"
        >
          {{ preset.label }}
        </option>
        <option value="custom">自选目录…</option>
      </select>
      <div class="path-picker" :class="{ disabled: isScanBusy }">
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
          :disabled="isScanBusy"
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
        :disabled="isScanBusy"
        @click="requestScan(false)"
      >
        扫描
      </BaseButton>
      <BaseButton :disabled="isScanBusy" @click="requestScan(true)">
        重新扫描
      </BaseButton>
      <BaseButton
        v-if="scanTask?.state === 'running'"
        :disabled="!scanTask.canCancel || cancellingTasks.has(scanTask.id)"
        @click="cancelScan"
      >
        {{ scanTask.cancelRequested ? "正在停止" : "停止" }}
      </BaseButton>
      <BaseButton
        variant="primary"
        :loading="planningMigration"
        :disabled="isScanBusy || planningMigration || selectedRows.length === 0"
        @click="prepareMigration"
      >
        迁移选中项（{{ selectedRows.length }}）
      </BaseButton>
    </div>

    <div class="activity-stack">
      <div v-if="scanTask?.state === 'running'" class="task-progress">
        <TaskProgress :task="scanTask" />
      </div>

      <section
        v-if="migrationEntries.length"
        class="migration-queue"
        :class="{ collapsed: migrationQueueCollapsed }"
      >
        <header>
          <button
            type="button"
            class="queue-toggle"
            :aria-expanded="!migrationQueueCollapsed"
            @click="migrationQueueCollapsed = !migrationQueueCollapsed"
          >
            <span class="queue-chevron" :class="{ collapsed: migrationQueueCollapsed }" />
            <strong>后台迁移队列</strong>
          </button>
          <span class="queue-summary" title="任务按队列顺序执行">
            {{ activeMigrationCount }} 项进行中或等待中 · {{ queueSummaryLabel }}
          </span>
        </header>
        <div v-show="!migrationQueueCollapsed" class="queue-list">
          <article v-for="entry in migrationEntries" :key="entry.task.id">
            <div class="queue-paths">
              <strong :title="entry.plan.source">{{ entry.plan.source }}</strong>
              <span :title="entry.plan.destination ?? ''">→ {{ entry.plan.destination }}</span>
            </div>
            <div class="queue-state" :class="entry.task.state">
              <TaskProgress v-if="entry.task.state === 'running'" :task="entry.task" />
              <template v-else>
                <div><span>{{ taskStateLabel(entry.task) }}</span></div>
                <small>{{ entryTimeLabel(entry) }}</small>
              </template>
            </div>
            <BaseButton
              v-if="entry.task.state === 'queued' || entry.task.state === 'running'"
              :disabled="!entry.task.canCancel || cancellingTasks.has(entry.task.id)"
              size="small"
              @click="cancelMigration(entry.task)"
            >
              {{ entry.task.canCancel ? "取消" : entry.task.cancelRequested ? "正在停止" : "安全收尾中" }}
            </BaseButton>
          </article>
        </div>
      </section>
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
        <strong>目录明细</strong>
        <span v-if="loadingPreview || loadingPage">正在读取…</span>
      </header>
      <FolderTable
        :rows="visibleRows"
        :selected-paths="selectedPaths"
        :sort-field="sortField"
        :sort-direction="sortDirection"
        :total-allocated-bytes="scanPage?.totalAllocatedBytes ?? 0"
        @toggle="toggleSelection"
        @toggle-all="toggleAllVisible"
        @sort="changeSort"
        @open="openFolder"
        @copy="copyFolder"
      />
      <nav v-if="pageCount > 1" class="folder-pagination" aria-label="目录分页">
        <span>第 {{ currentPage }} / {{ pageCount }} 页 · 共 {{ formatInteger(folderCount) }} 项</span>
        <div>
          <BaseButton size="small" :disabled="isScanBusy || currentPage === 1" @click="changePage(1)">首页</BaseButton>
          <BaseButton size="small" :disabled="isScanBusy || currentPage === 1" @click="changePage(currentPage - 1)">上一页</BaseButton>
          <BaseButton size="small" :disabled="isScanBusy || currentPage === pageCount" @click="changePage(currentPage + 1)">下一页</BaseButton>
          <BaseButton size="small" :disabled="isScanBusy || currentPage === pageCount" @click="changePage(pageCount)">末页</BaseButton>
        </div>
      </nav>
    </section>
  </section>

  <MigrationDestinationDialog
    :open="migrationOpen"
    :sources="managedPlans.map((plan) => plan.source)"
    :mode="migrationMode"
    :managed-plans="managedPlans"
    :custom-plans="customPlans"
    :busy="planningMigration || submittingMigration"
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
      <p>迁移前请关闭正在使用这些目录的程序和服务。任务将按下列顺序进入后台队列。</p>
      <div class="confirmation-list">
        <dl v-for="plan in pendingPlans" :key="plan.source">
          <dt>原位置</dt>
          <dd>{{ plan.source }}</dd>
          <dt>新位置</dt>
          <dd>{{ plan.destination }}</dd>
        </dl>
      </div>
    </div>
    <template #footer>
      <BaseButton :disabled="submittingMigration" @click="confirmMigrationOpen = false">取消</BaseButton>
      <BaseButton variant="primary" :loading="submittingMigration" @click="executeMigration">
        加入后台队列（{{ pendingPlans.length }}）
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
.folder-pagination {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--space-3);
  padding: var(--space-3);
  border-top: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  font-size: var(--font-size-small);
}

.folder-pagination > div {
  display: flex;
  gap: var(--space-2);
}
.view {
  display: grid;
  height: 100%;
  min-width: 0;
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

.activity-stack:empty {
  display: none;
}

.activity-stack {
  display: grid;
  min-height: 0;
  gap: var(--space-2);
}

.migration-queue {
  display: grid;
  max-height: 178px;
  grid-template-rows: 36px minmax(0, 1fr);
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-medium);
  background: var(--surface-primary);
}

.migration-queue.collapsed {
  max-height: 36px;
  grid-template-rows: 36px;
}

.migration-queue > header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 var(--space-3);
  border-bottom: 1px solid var(--border-subtle);
  font-size: var(--font-size-small);
}

.queue-summary {
  color: var(--text-secondary);
}

.migration-queue.collapsed > header {
  border-bottom-color: transparent;
}

.queue-toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--text-primary);
  background: transparent;
}

.queue-chevron {
  width: 7px;
  height: 7px;
  transform: rotate(45deg);
  border-right: 1.5px solid currentColor;
  border-bottom: 1.5px solid currentColor;
  color: var(--text-secondary);
  transition: transform 120ms ease;
}

.queue-chevron.collapsed {
  transform: rotate(-45deg);
}

.queue-list {
  overflow: auto;
}

.queue-list article {
  display: grid;
  min-height: 52px;
  grid-template-columns: minmax(0, 1fr) minmax(180px, 220px) auto;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border-subtle);
}

.queue-paths {
  display: grid;
  min-width: 0;
  gap: 2px;
}

.queue-paths strong,
.queue-paths span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.queue-paths strong {
  font-size: 12px;
  font-weight: 600;
}

.queue-paths span {
  color: var(--text-secondary);
  font-size: 11px;
}

.queue-state {
  display: grid;
  gap: 2px;
  color: var(--text-secondary);
  font-size: 12px;
}

.queue-state > div {
  display: flex;
  justify-content: space-between;
}

.queue-state small {
  overflow: hidden;
  color: var(--text-secondary);
  font-size: 10px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.queue-state.running,
.queue-state.completed {
  color: var(--success);
}

.queue-state.failed,
.queue-state.cancelled {
  color: var(--warning);
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
  min-width: 0;
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

.confirmation-list {
  display: grid;
  max-height: 300px;
  gap: var(--space-3);
  overflow: auto;
}

.confirmation-list dl {
  padding: var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
  background: var(--surface-secondary);
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
