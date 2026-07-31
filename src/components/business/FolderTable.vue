<script setup lang="ts">
import { ref } from "vue";
import BaseContextMenu from "@/components/base/BaseContextMenu.vue";
import EmptyState from "@/components/base/EmptyState.vue";
import type { FolderRow, SortDirection, SortField } from "@/types/domain";
import { formatBytes, formatInteger } from "@/utils/format";

defineProps<{
  rows: FolderRow[];
  selectedPath: string | null;
  sortField: SortField;
  sortDirection: SortDirection;
}>();

const emit = defineEmits<{
  select: [row: FolderRow];
  sort: [field: SortField];
  open: [row: FolderRow];
  copy: [row: FolderRow];
}>();

const menu = ref({ open: false, x: 0, y: 0, row: null as FolderRow | null });

const sortMark = (
  field: SortField,
  current: SortField,
  direction: SortDirection,
): string => {
  if (field !== current) return "";
  return direction === "ascending" ? " ↑" : " ↓";
};

const showMenu = (event: MouseEvent, row: FolderRow): void => {
  event.preventDefault();
  emit("select", row);
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
      <table>
        <thead>
          <tr>
            <th class="align-left folder-column">
              <button type="button" @click="$emit('sort', 'name')">
                文件夹{{ sortMark("name", sortField, sortDirection) }}
              </button>
            </th>
            <th>
              <button type="button" @click="$emit('sort', 'allocatedBytes')">
                磁盘占用{{ sortMark("allocatedBytes", sortField, sortDirection) }}
              </button>
            </th>
            <th>
              <button type="button" @click="$emit('sort', 'fileCount')">
                文件数{{ sortMark("fileCount", sortField, sortDirection) }}
              </button>
            </th>
            <th>
              <button type="button" @click="$emit('sort', 'status')">
                状态{{ sortMark("status", sortField, sortDirection) }}
              </button>
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="row in rows"
            :key="row.fullPath"
            :class="{ selected: selectedPath === row.fullPath }"
            tabindex="0"
            @click="$emit('select', row)"
            @dblclick="$emit('open', row)"
            @contextmenu="showMenu($event, row)"
            @keydown.enter="$emit('open', row)"
            @keydown.space.prevent="$emit('select', row)"
          >
            <td class="align-left folder-name" :title="row.fullPath">
              <span class="row-marker" :class="{ linked: row.isLink }" />
              {{ row.name }}
            </td>
            <td>{{ row.measured ? formatBytes(row.allocatedBytes) : "未扫描" }}</td>
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
  overflow: auto;
}

table {
  width: 100%;
  border-collapse: separate;
  border-spacing: 0;
  table-layout: fixed;
}

th {
  position: sticky;
  z-index: 1;
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
  width: 100%;
  height: 100%;
  color: inherit;
  background: transparent;
  text-align: inherit;
}

th:not(.folder-column) {
  width: 145px;
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
