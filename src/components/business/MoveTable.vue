<script setup lang="ts">
import { ref } from "vue";
import BaseContextMenu from "@/components/base/BaseContextMenu.vue";
import EmptyState from "@/components/base/EmptyState.vue";
import type { MoveRow } from "@/types/domain";
import { formatDate } from "@/utils/format";

defineProps<{
  rows: MoveRow[];
  selectedId: string | null;
}>();

const emit = defineEmits<{
  select: [row: MoveRow];
  open: [row: MoveRow];
  copy: [row: MoveRow];
}>();

const menu = ref({ open: false, x: 0, y: 0, row: null as MoveRow | null });
const showMenu = (event: MouseEvent, row: MoveRow): void => {
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
            <th class="status-column">状态</th>
            <th>原位置</th>
            <th>新位置</th>
            <th class="time-column">迁移时间</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="row in rows"
            :key="row.id"
            :class="{ selected: selectedId === row.id }"
            tabindex="0"
            @click="$emit('select', row)"
            @dblclick="$emit('open', row)"
            @contextmenu="showMenu($event, row)"
            @keydown.enter="$emit('open', row)"
          >
            <td>
              <span
                class="status"
                :class="{
                  good: row.status.startsWith('有效'),
                  warning: row.recoveryAvailable,
                }"
              >
                {{ row.status }}
              </span>
            </td>
            <td :title="row.source">{{ row.source }}</td>
            <td :title="row.destination">{{ row.destination }}</td>
            <td>{{ formatDate(row.createdAtUtc) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
    <EmptyState
      v-else
      title="没有迁移记录"
      description="完成一次目录迁移后，这里会持续核对原位置、目标位置和目录联接。"
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
      复制迁移信息
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

.status-column {
  width: 170px;
}

.time-column {
  width: 160px;
}

.status.good {
  color: var(--success);
}

.status.warning {
  color: var(--warning);
}
</style>
