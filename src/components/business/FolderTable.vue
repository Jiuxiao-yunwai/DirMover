<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";
import BaseContextMenu from "@/components/base/BaseContextMenu.vue";
import EmptyState from "@/components/base/EmptyState.vue";
import type { FolderRow, SortDirection, SortField } from "@/types/domain";
import { normalizeColumnRatios, resizeColumnPair } from "@/utils/columnSizing";
import { formatBytes, formatInteger } from "@/utils/format";

const props = defineProps<{
  rows: FolderRow[];
  selectedPaths: string[];
  sortField: SortField;
  sortDirection: SortDirection;
  totalAllocatedBytes: number;
}>();

const emit = defineEmits<{
  toggle: [row: FolderRow];
  toggleAll: [];
  sort: [field: SortField];
  open: [row: FolderRow];
  copy: [row: FolderRow];
}>();

const menu = ref({ open: false, x: 0, y: 0, row: null as FolderRow | null });
const table = ref<HTMLTableElement | null>(null);
const columnRatios = ref<number[] | null>(null);
const resizingColumn = ref<number | null>(null);
const minimumColumnWidths = [36, 160, 104, 88, 112];
let stopPointerResize: (() => void) | null = null;
const selectedSet = computed(() => new Set(props.selectedPaths));
const allSelected = computed(
  () => props.rows.length > 0 && props.rows.every((row) => selectedSet.value.has(row.fullPath)),
);
const someSelected = computed(
  () => !allSelected.value && props.rows.some((row) => selectedSet.value.has(row.fullPath)),
);

const allocatedPercentage = (row: FolderRow): number => {
  if (!row.measured || props.totalAllocatedBytes <= 0) return 0;
  return Math.min(100, (row.allocatedBytes / props.totalAllocatedBytes) * 100);
};

const percentageLabel = (row: FolderRow): string => {
  const percentage = allocatedPercentage(row);
  if (percentage <= 0) return "0%";
  if (percentage < 0.1) return "<0.1%";
  return `${percentage.toFixed(1)}%`;
};

const sortClass = computed(() =>
  props.sortDirection === "ascending" ? "ascending" : "descending",
);
const columnStyle = (index: number): { width: string } | undefined => {
  const ratio = columnRatios.value?.[index];
  return ratio ? { width: `${ratio * 100}%` } : undefined;
};

const measureColumnWidths = (): number[] => {
  const headers = table.value?.querySelectorAll("thead th");
  return minimumColumnWidths.map((minimum, index) =>
    Math.max(minimum, headers?.[index]?.getBoundingClientRect().width ?? minimum),
  );
};

const resizedWidths = (initial: number[], index: number, requestedDelta: number): number[] => {
  return resizeColumnPair(initial, minimumColumnWidths, index, requestedDelta);
};

const startResize = (event: PointerEvent, index: number): void => {
  event.preventDefault();
  event.stopPropagation();
  stopPointerResize?.();
  const initial = measureColumnWidths();
  const startX = event.clientX;
  columnRatios.value = normalizeColumnRatios(initial);
  resizingColumn.value = index;
  const previousCursor = document.body.style.cursor;
  const previousUserSelect = document.body.style.userSelect;
  document.body.style.cursor = "col-resize";
  document.body.style.userSelect = "none";

  const move = (moveEvent: PointerEvent): void => {
    columnRatios.value = normalizeColumnRatios(
      resizedWidths(initial, index, moveEvent.clientX - startX),
    );
  };
  const stop = (): void => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", stop);
    window.removeEventListener("pointercancel", stop);
    document.body.style.cursor = previousCursor;
    document.body.style.userSelect = previousUserSelect;
    resizingColumn.value = null;
    stopPointerResize = null;
  };
  stopPointerResize = stop;
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", stop);
  window.addEventListener("pointercancel", stop);
};

const resizeWithKeyboard = (index: number, delta: number): void => {
  const initial = measureColumnWidths();
  columnRatios.value = normalizeColumnRatios(resizedWidths(initial, index, delta));
};

const resetColumnWidths = (): void => {
  stopPointerResize?.();
  columnRatios.value = null;
};

onBeforeUnmount(() => {
  stopPointerResize?.();
});

const showMenu = (event: MouseEvent, row: FolderRow): void => {
  event.preventDefault();
  if (!selectedSet.value.has(row.fullPath)) emit("toggle", row);
  menu.value = {
    open: true,
    x: Math.min(event.clientX, window.innerWidth - 205),
    y: Math.min(event.clientY, window.innerHeight - 92),
    row,
  };
};

const invokeMenu = (action: "open" | "copy"): void => {
  const row = menu.value.row;
  menu.value.open = false;
  if (row) emit(action, row);
};
</script>

<template>
  <div class="table-shell">
    <div v-if="rows.length > 0" class="table-scroll">
      <table ref="table" :class="{ resizing: resizingColumn !== null }">
        <colgroup>
          <col class="selection-column-width" :style="columnStyle(0)" />
          <col class="folder-column-width" :style="columnStyle(1)" />
          <col class="usage-column-width" :style="columnStyle(2)" />
          <col class="count-column-width" :style="columnStyle(3)" />
          <col class="status-column-width" :style="columnStyle(4)" />
        </colgroup>
        <thead>
          <tr>
            <th class="selection-column">
              <input
                type="checkbox"
                aria-label="选择当前列表中的全部文件夹"
                :checked="allSelected"
                :indeterminate="someSelected"
                @change="$emit('toggleAll')"
              />
              <span
                class="resize-handle"
                role="separator"
                tabindex="0"
                aria-label="调整选择列宽度"
                title="拖动调整列宽；双击恢复默认"
                @pointerdown="startResize($event, 0)"
                @keydown.left.prevent="resizeWithKeyboard(0, -12)"
                @keydown.right.prevent="resizeWithKeyboard(0, 12)"
                @dblclick.stop="resetColumnWidths"
              />
            </th>
            <th
              class="align-left folder-column"
              :aria-sort="
                sortField === 'name'
                  ? sortDirection === 'ascending'
                    ? 'ascending'
                    : 'descending'
                  : 'none'
              "
            >
              <button type="button" @click="$emit('sort', 'name')">
                <span>文件夹</span>
                <span
                  v-if="sortField === 'name'"
                  class="sort-indicator"
                  :class="sortClass"
                  aria-hidden="true"
                />
              </button>
              <span
                class="resize-handle"
                role="separator"
                tabindex="0"
                aria-label="调整文件夹列宽度"
                title="拖动调整列宽；双击恢复默认"
                @pointerdown="startResize($event, 1)"
                @keydown.left.prevent="resizeWithKeyboard(1, -12)"
                @keydown.right.prevent="resizeWithKeyboard(1, 12)"
                @dblclick.stop="resetColumnWidths"
              />
            </th>
            <th
              :aria-sort="
                sortField === 'allocatedBytes'
                  ? sortDirection === 'ascending'
                    ? 'ascending'
                    : 'descending'
                  : 'none'
              "
            >
              <button type="button" @click="$emit('sort', 'allocatedBytes')">
                <span>磁盘占用</span>
                <span
                  v-if="sortField === 'allocatedBytes'"
                  class="sort-indicator"
                  :class="sortClass"
                  aria-hidden="true"
                />
              </button>
              <span
                class="resize-handle"
                role="separator"
                tabindex="0"
                aria-label="调整磁盘占用列宽度"
                title="拖动调整列宽；双击恢复默认"
                @pointerdown="startResize($event, 2)"
                @keydown.left.prevent="resizeWithKeyboard(2, -12)"
                @keydown.right.prevent="resizeWithKeyboard(2, 12)"
                @dblclick.stop="resetColumnWidths"
              />
            </th>
            <th
              :aria-sort="
                sortField === 'fileCount'
                  ? sortDirection === 'ascending'
                    ? 'ascending'
                    : 'descending'
                  : 'none'
              "
            >
              <button type="button" @click="$emit('sort', 'fileCount')">
                <span>文件数</span>
                <span
                  v-if="sortField === 'fileCount'"
                  class="sort-indicator"
                  :class="sortClass"
                  aria-hidden="true"
                />
              </button>
              <span
                class="resize-handle"
                role="separator"
                tabindex="0"
                aria-label="调整文件数列宽度"
                title="拖动调整列宽；双击恢复默认"
                @pointerdown="startResize($event, 3)"
                @keydown.left.prevent="resizeWithKeyboard(3, -12)"
                @keydown.right.prevent="resizeWithKeyboard(3, 12)"
                @dblclick.stop="resetColumnWidths"
              />
            </th>
            <th
              :aria-sort="
                sortField === 'status'
                  ? sortDirection === 'ascending'
                    ? 'ascending'
                    : 'descending'
                  : 'none'
              "
            >
              <button type="button" @click="$emit('sort', 'status')">
                <span>状态</span>
                <span
                  v-if="sortField === 'status'"
                  class="sort-indicator"
                  :class="sortClass"
                  aria-hidden="true"
                />
              </button>
              <span
                class="resize-handle"
                role="separator"
                tabindex="0"
                aria-label="调整状态列宽度"
                title="拖动调整列宽；双击恢复默认"
                @pointerdown="startResize($event, 4)"
                @keydown.left.prevent="resizeWithKeyboard(4, -12)"
                @keydown.right.prevent="resizeWithKeyboard(4, 12)"
                @dblclick.stop="resetColumnWidths"
              />
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="row in rows"
            :key="row.fullPath"
            data-app-context-menu
            :class="{ selected: selectedSet.has(row.fullPath) }"
            tabindex="0"
            @click="$emit('toggle', row)"
            @dblclick="$emit('open', row)"
            @contextmenu="showMenu($event, row)"
            @keydown.enter="$emit('open', row)"
            @keydown.space.prevent="$emit('toggle', row)"
          >
            <td class="selection-cell">
              <input
                type="checkbox"
                :checked="selectedSet.has(row.fullPath)"
                :aria-label="`选择 ${row.name}`"
                @click.stop="$emit('toggle', row)"
              />
            </td>
            <td class="align-left folder-name" :title="row.fullPath">
              <span class="row-marker" :class="{ linked: row.isLink }" />
              {{ row.name }}
            </td>
            <td
              class="usage-cell"
              :title="
                row.measured
                  ? `${formatBytes(row.allocatedBytes)}，占当前扫描总量 ${percentageLabel(row)}`
                  : '未扫描'
              "
            >
              <span
                v-if="row.measured"
                class="usage-bar"
                :style="{ width: `${allocatedPercentage(row)}%` }"
              />
              <span class="usage-value">
                {{ row.measured ? formatBytes(row.allocatedBytes) : "未扫描" }}
              </span>
            </td>
            <td>{{ row.measured ? formatInteger(row.fileCount) : "—" }}</td>
            <td :title="row.linkTarget ?? row.note">
              <span
                class="status"
                :class="{
                  warning: row.errorCount > 0,
                  linked: row.isLink,
                  muted: !row.measured,
                }"
              >
                {{
                  row.isLink
                    ? "目录链接"
                    : !row.measured
                      ? "未扫描"
                      : row.errorCount > 0
                        ? `${formatInteger(row.errorCount)} 项不可读`
                        : "已扫描"
                }}
              </span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <EmptyState
      v-else
      title="没有可显示的文件夹"
      description="选择一个目录后会立即列出一级文件夹；扫描只在你主动开始时运行。"
    />
  </div>

  <BaseContextMenu
    :open="menu.open"
    :x="menu.x"
    :y="menu.y"
    @close="menu.open = false"
  >
    <button type="button" role="menuitem" @click="invokeMenu('open')">
      在文件资源管理器中打开
    </button>
    <button type="button" role="menuitem" @click="invokeMenu('copy')">
      复制完整路径
    </button>
  </BaseContextMenu>
</template>

<style scoped>
.table-shell,
.table-scroll {
  min-height: 0;
  height: 100%;
}

.table-scroll {
  overflow-x: hidden;
  overflow-y: auto;
}

table {
  width: 100%;
  border-collapse: separate;
  border-spacing: 0;
  table-layout: fixed;
}

th {
  position: sticky;
  z-index: 10;
  top: 0;
  height: 34px;
  padding: 0 var(--space-3);
  border-bottom: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  background: var(--surface-secondary);
  font-size: var(--font-size-small);
  font-weight: 600;
  text-align: right;
}

th button {
  display: inline-flex;
  width: 100%;
  height: 100%;
  align-items: center;
  justify-content: flex-end;
  gap: 6px;
  white-space: nowrap;
  color: inherit;
  background: transparent;
  text-align: inherit;
}

.resize-handle {
  position: absolute;
  z-index: 12;
  top: 0;
  right: -5px;
  width: 10px;
  height: 100%;
  cursor: col-resize;
  touch-action: none;
}

.resize-handle::after {
  position: absolute;
  top: 7px;
  right: 4px;
  bottom: 7px;
  width: 2px;
  border-radius: 1px;
  background: transparent;
  content: "";
}

.resize-handle:hover::after,
.resize-handle:focus-visible::after,
.resizing .resize-handle::after {
  background: var(--accent);
}

.resize-handle:focus-visible {
  outline: 1px solid var(--accent);
  outline-offset: -2px;
}

.selection-column-width {
  width: 42px;
}

.selection-column,
.selection-cell {
  padding: 0;
  text-align: center;
}

.selection-column input,
.selection-cell input {
  width: 15px;
  height: 15px;
  accent-color: var(--accent);
}

.folder-column button {
  justify-content: flex-start;
}

.folder-column-width {
  width: auto;
}

.usage-column-width {
  width: 12ch;
}

.count-column-width {
  width: 14%;
}

.status-column-width {
  width: 18%;
}

.sort-indicator {
  width: 0;
  height: 0;
  border-right: 4px solid transparent;
  border-left: 4px solid transparent;
}

.sort-indicator.ascending {
  border-bottom: 6px solid currentColor;
}

.sort-indicator.descending {
  border-top: 6px solid currentColor;
}

td {
  height: 32px;
  padding: 0 var(--space-3);
  overflow: hidden;
  border-bottom: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
  text-align: right;
  text-overflow: ellipsis;
  white-space: nowrap;
}

th:not(:last-child),
td:not(:last-child) {
  border-right: 1px solid color-mix(in srgb, var(--border-subtle) 72%, transparent);
}

tbody tr {
  background: var(--surface-primary);
}

tbody tr:hover {
  background: var(--surface-hover);
}

tbody tr.selected {
  background: var(--surface-selected);
}

tbody tr:focus-visible {
  position: relative;
  z-index: 2;
  outline-offset: -2px;
}

.align-left {
  text-align: left;
}

.folder-name {
  color: var(--text-primary);
}

.usage-cell {
  position: relative;
}

.usage-bar {
  position: absolute;
  inset: 3px auto 3px 0;
  min-width: 2px;
  border-radius: 0 3px 3px 0;
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  pointer-events: none;
}

.usage-value {
  position: relative;
  z-index: 1;
}

.row-marker {
  display: inline-block;
  width: 6px;
  height: 6px;
  margin-right: var(--space-2);
  border-radius: 50%;
  background: var(--border-strong);
}

.row-marker.linked {
  background: var(--warning);
}

.status {
  color: var(--success);
}

.status.muted {
  color: var(--text-disabled);
}

.status.warning,
.status.linked {
  color: var(--warning);
}
</style>
