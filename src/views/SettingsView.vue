<script setup lang="ts">
import { computed, ref } from "vue";
import BaseButton from "@/components/base/BaseButton.vue";
import { api } from "@/services/api";
import { normalizeAppError } from "@/services/tauri";
import type { AppSettings, BootstrapData } from "@/types/domain";
import { formatBytes } from "@/utils/format";

const props = defineProps<{ bootstrap: BootstrapData }>();
const emit = defineEmits<{
  status: [message: string];
  busy: [busy: boolean];
  refreshData: [];
  notify: [message: string, tone?: "info" | "success" | "warning" | "error"];
}>();

const busy = ref(false);
const hasOnlySystemDrive = computed(
  () => props.bootstrap.drives.filter((drive) => drive.fixed).length <= 1,
);

const updateSettings = async (
  transform: (settings: AppSettings) => AppSettings,
  message: string,
): Promise<void> => {
  busy.value = true;
  emit("busy", true);
  try {
    await api.saveSettings(transform(structuredClone(props.bootstrap.settings)));
    emit("notify", message, "success");
    emit("refreshData");
  } catch (error: unknown) {
    emit("notify", normalizeAppError(error).message, "error");
  } finally {
    busy.value = false;
    emit("busy", false);
  }
};

const chooseManagedRoot = async (): Promise<void> => {
  const selected = await api.chooseDirectory("选择统一托管根目录");
  if (!selected) return;
  await updateSettings(
    (settings) => ({ ...settings, managedRoot: selected }),
    "统一托管位置已更新。",
  );
};

const chooseCategoryRoot = async (key: string, label: string): Promise<void> => {
  const selected = await api.chooseDirectory(`选择 ${label} 的托管位置`);
  if (!selected) return;
  await updateSettings(
    (settings) => ({
      ...settings,
      categoryRoots: { ...settings.categoryRoots, [key]: selected },
    }),
    `${label} 的独立位置已更新。`,
  );
};

const useAutomatic = async (): Promise<void> => {
  await updateSettings(
    (settings) => ({ ...settings, managedRoot: null }),
    "已恢复自动选择统一根目录。",
  );
};

const clearCategoryRoots = async (): Promise<void> => {
  await updateSettings(
    (settings) => ({ ...settings, categoryRoots: {} }),
    "已清除所有分类独立位置。",
  );
};

const openManagedRoot = async (): Promise<void> => {
  try {
    await api.openManagedRoot();
  } catch (error: unknown) {
    emit("notify", normalizeAppError(error).message, "error");
  }
};
</script>

<template>
  <section class="view">
    <header class="page-header">
      <div>
        <h1>设置</h1>
        <p>配置统一托管位置，或为不同来源单独指定固定 NTFS 分区</p>
      </div>
    </header>

    <div v-if="hasOnlySystemDrive" class="warning-strip">
      当前只检测到一个固定 NTFS 分区，默认托管将回退到 C 盘；迁移同分区目录时仍需另选其他分区。
    </div>

    <section class="settings-section">
      <header>
        <div>
          <h2>托管位置</h2>
          <p>
            {{
              bootstrap.settings.managedRoot
                ? "使用手动设置的统一根目录"
                : "优先选择系统盘同一物理磁盘上的最后一个 NTFS 分区"
            }}
          </p>
        </div>
        <div class="actions">
          <BaseButton :disabled="busy" @click="chooseManagedRoot">
            更改统一根目录
          </BaseButton>
          <BaseButton :disabled="busy" @click="useAutomatic">恢复自动选择</BaseButton>
          <BaseButton :disabled="busy" @click="openManagedRoot">打开目录</BaseButton>
        </div>
      </header>

      <div class="drive-strip">
        <div v-for="drive in bootstrap.drives" :key="drive.root">
          <strong>{{ drive.root }}</strong>
          <span>{{ drive.fileSystem }} · 可用 {{ formatBytes(drive.freeBytes) }}</span>
        </div>
      </div>

      <div class="category-list">
        <div
          v-for="location in bootstrap.managedLocations"
          :key="location.key"
          class="category-row"
        >
          <div>
            <strong>{{ location.label }}</strong>
            <span>{{ location.overridden ? "独立设置" : "跟随统一根目录" }}</span>
          </div>
          <code :title="location.path ?? '不可用'">
            {{ location.path ?? "不可用（没有其他固定 NTFS 分区）" }}
          </code>
          <BaseButton
            size="small"
            :disabled="busy"
            @click="chooseCategoryRoot(location.key, location.label)"
          >
            更改
          </BaseButton>
        </div>
      </div>

      <footer>
        <BaseButton
          :disabled="
            busy || Object.keys(bootstrap.settings.categoryRoots).length === 0
          "
          @click="clearCategoryRoots"
        >
          清除分类独立设置
        </BaseButton>
        <span>设置继续保存在 {{ bootstrap.dataDirectory }}</span>
      </footer>
    </section>
  </section>
</template>

<style scoped>
.view {
  height: 100%;
  overflow: auto;
  padding: var(--space-4) var(--space-5) var(--space-5);
}

.page-header {
  display: flex;
  align-items: center;
  min-height: 52px;
}

h1 {
  font-size: var(--font-size-title);
  font-weight: 650;
}

.page-header p,
.settings-section header p,
.settings-section footer span {
  color: var(--text-secondary);
  font-size: var(--font-size-small);
}

.warning-strip {
  margin-bottom: var(--space-3);
  padding: var(--space-3);
  border: 1px solid color-mix(in srgb, var(--warning) 40%, var(--border-subtle));
  border-radius: var(--radius-medium);
  color: var(--warning);
  background: var(--warning-subtle);
  font-size: var(--font-size-small);
}

.settings-section {
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-medium);
  background: var(--surface-primary);
}

.settings-section > header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-4);
  border-bottom: 1px solid var(--border-subtle);
}

h2 {
  font-size: var(--font-size-heading);
  font-weight: 600;
}

.actions {
  display: flex;
  gap: var(--space-2);
}

.drive-strip {
  display: flex;
  gap: var(--space-5);
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--border-subtle);
  background: var(--surface-secondary);
}

.drive-strip div {
  display: grid;
}

.drive-strip span {
  color: var(--text-secondary);
  font-size: 11px;
}

.category-list {
  padding: 0 var(--space-4);
}

.category-row {
  display: grid;
  min-height: 58px;
  grid-template-columns: 160px minmax(0, 1fr) auto;
  align-items: center;
  gap: var(--space-3);
  border-bottom: 1px solid var(--border-subtle);
}

.category-row > div {
  display: grid;
}

.category-row span {
  color: var(--text-secondary);
  font-size: 11px;
}

code {
  overflow: hidden;
  color: var(--text-secondary);
  font-family: var(--font-family);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.settings-section > footer {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  background: var(--surface-secondary);
}
</style>
