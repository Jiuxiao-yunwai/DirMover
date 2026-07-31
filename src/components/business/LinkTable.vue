<script setup lang="ts">
import { ref } from "vue";
import BaseContextMenu from "@/components/base/BaseContextMenu.vue";
import EmptyState from "@/components/base/EmptyState.vue";
import type { LinkRow } from "@/types/domain";
import { formatDate } from "@/utils/format";

defineProps<{
  rows: LinkRow[];
  selectedPath: string | null;
}>();

const emit = defineEmits<{
  select: [row: LinkRow];
  open: [row: LinkRow];
  copy: [row: LinkRow];
}>();
const menu = ref({ open: false, x: 0, y: 0, row: null as LinkRow | null });
const showMenu = (event: MouseEvent, row: LinkRow): void => {
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
    <div v-if="rows.length" class="table-scroll">
      <table>
        <thead>
          <tr>
            <th class="kind-column">类型</th>
            <th>链接位置</th>
            <th>指向位置</th>
            <th class="state-column">状态</th>
            <th class="time-column">检测时间</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="row in rows"
            :key="`${row.scanRoot}:${row.path}`"
            data-app-context-menu
            :class="{ selected: selectedPath === row.path }"
            tabindex="0"
            @click="$emit('select', row)"
            @dblclick="$emit('open', row)"
            @contextmenu="showMenu($event, row)"
            @keydown.enter="$emit('open', row)"
          >
            <td>{{ row.kind }}</td>
            <td :title="row.path">{{ row.path }}</td>
            <td :title="row.target">{{ row.target }}</td>
            <td :class="{ valid: row.state === '有效', invalid: row.state !== '有效' }">
              {{ row.state }}
            </td>
            <td>{{ formatDate(row.detectedAtUtc) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
    <EmptyState
      v-else
      title="没有链接记录"
      description="选择目录后检测其直接子项；检测过程不会进入链接目标。"
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
      复制链接信息
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
  top: 0;
  height: 34px;
  padding: 0 var(--space-3);
  border-bottom: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  background: var(--surface-secondary);
  font-size: var(--font-size-small);
  font-weight: 600;
  text-align: left;
}

td {
  height: 34px;
  padding: 0 var(--space-3);
  overflow: hidden;
  border-bottom: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  text-overflow: ellipsis;
  white-space: nowrap;
}

tr:hover {
  background: var(--surface-hover);
}

tr.selected {
  background: var(--surface-selected);
}

.kind-column,
.state-column {
  width: 104px;
}

.time-column {
  width: 154px;
}

.valid {
  color: var(--success);
}

.invalid {
  color: var(--danger);
}
</style>
