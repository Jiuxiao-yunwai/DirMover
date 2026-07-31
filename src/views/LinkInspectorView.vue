<script setup lang="ts">
import { ref, shallowRef, watch } from "vue";
import BaseButton from "@/components/base/BaseButton.vue";
import LinkTable from "@/components/business/LinkTable.vue";
import { api } from "@/services/api";
import { normalizeAppError } from "@/services/tauri";
import type { BootstrapData, LinkRow } from "@/types/domain";
import { copyText } from "@/utils/format";

const props = defineProps<{ bootstrap: BootstrapData }>();
const emit = defineEmits<{
  status: [message: string];
  busy: [busy: boolean];
  refreshData: [];
  notify: [message: string, tone?: "info" | "success" | "warning" | "error"];
}>();

const rows = shallowRef<LinkRow[]>(props.bootstrap.links);
const selected = ref<LinkRow | null>(null);
const busy = ref(false);

watch(
  () => props.bootstrap.links,
  (links) => {
    rows.value = links;
  },
);

const detect = async (): Promise<void> => {
  const root = await api.chooseDirectory("选择要检测链接的目录");
  if (!root) return;
  busy.value = true;
  emit("busy", true);
  emit("status", `正在检测 ${root} 的直接子项…`);
  try {
    const found = await api.detectLinks(root);
    emit("notify", `检测完成：找到 ${found.length} 个链接。`, "success");
    emit("refreshData");
  } catch (error: unknown) {
    emit("notify", normalizeAppError(error).message, "error");
  } finally {
    busy.value = false;
    emit("busy", false);
  }
};

const openLink = async (row: LinkRow): Promise<void> => {
  try {
    await api.openPath(row.path, true);
  } catch (error: unknown) {
    emit("notify", normalizeAppError(error).message, "error");
  }
};

const copyLink = async (row: LinkRow): Promise<void> => {
  await copyText(`链接：${row.path}\n目标：${row.target}`);
  emit("notify", "链接信息已复制。", "success");
};
</script>

<template>
  <section class="view">
    <header class="page-header">
      <h1>链接检测</h1>
      <span>{{ rows.length }} 条记录</span>
    </header>
    <div class="toolbar">
      <BaseButton variant="primary" :loading="busy" @click="detect">
        选择目录并检测
      </BaseButton>
    </div>
    <section class="content-panel">
      <LinkTable
        :rows="rows"
        :selected-path="selected?.path ?? null"
        @select="selected = $event"
        @open="openLink"
        @copy="copyLink"
      />
    </section>
  </section>
</template>

<style scoped>
.view {
  display: grid;
  height: 100%;
  min-height: 0;
  grid-template-rows: auto auto minmax(0, 1fr);
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
.page-header > span,
.toolbar span {
  color: var(--text-secondary);
  font-size: var(--font-size-small);
}

.toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.content-panel {
  min-height: 0;
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-medium);
  background: var(--surface-primary);
}
</style>
