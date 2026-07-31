<script setup lang="ts">
import BaseButton from "@/components/base/BaseButton.vue";
import BaseDialog from "@/components/base/BaseDialog.vue";
import type { MigrationPlan } from "@/types/domain";

defineProps<{
  open: boolean;
  source: string;
  mode: "managed" | "custom";
  managedPlan: MigrationPlan | null;
  customPlan: MigrationPlan | null;
  busy: boolean;
}>();

defineEmits<{
  close: [];
  "update:mode": [mode: "managed" | "custom"];
  chooseCustom: [];
  confirm: [plan: MigrationPlan];
}>();
</script>

<template>
  <BaseDialog
    :open="open"
    title="选择迁移位置"
    description="最终路径由 Rust 根据来源分类和安全规则生成"
    width="650px"
    :close-on-backdrop="!busy"
    @close="$emit('close')"
  >
    <div class="source-line">
      <span>待迁移</span>
      <code>{{ source }}</code>
    </div>

    <label class="option" :class="{ selected: mode === 'managed' }">
      <input
        type="radio"
        name="destination"
        :checked="mode === 'managed'"
        :disabled="!managedPlan?.available || busy"
        @change="$emit('update:mode', 'managed')"
      />
      <span>
        <strong>使用分类托管位置</strong>
        <small>按照 AppData、Program Files 等来源自动归类</small>
        <code>{{ managedPlan?.managedRoot ?? "没有可用位置" }}</code>
        <em v-if="managedPlan?.reason">{{ managedPlan.reason }}</em>
      </span>
    </label>

    <label class="option" :class="{ selected: mode === 'custom' }">
      <input
        type="radio"
        name="destination"
        :checked="mode === 'custom'"
        :disabled="busy"
        @change="$emit('update:mode', 'custom')"
      />
      <span>
        <strong>本次使用自定义位置</strong>
        <small>只影响当前迁移，不修改全局设置</small>
        <BaseButton
          size="small"
          :disabled="busy"
          @click.prevent="$emit('chooseCustom')"
        >
          选择目录…
        </BaseButton>
        <code v-if="customPlan?.managedRoot">{{ customPlan.managedRoot }}</code>
        <em v-if="customPlan?.reason">{{ customPlan.reason }}</em>
      </span>
    </label>

    <div class="destination-preview">
      <span>最终位置</span>
      <code>
        {{
          (mode === "managed" ? managedPlan?.destination : customPlan?.destination) ??
            "请选择一个位于其他固定 NTFS 分区的目录"
        }}
      </code>
    </div>

    <template #footer>
      <BaseButton :disabled="busy" @click="$emit('close')">取消</BaseButton>
      <BaseButton
        variant="primary"
        :loading="busy"
        :disabled="
          !(mode === 'managed' ? managedPlan?.available : customPlan?.available)
        "
        @click="
          $emit(
            'confirm',
            (mode === 'managed' ? managedPlan : customPlan) as MigrationPlan,
          )
        "
      >
        继续迁移
      </BaseButton>
    </template>
  </BaseDialog>
</template>

<style scoped>
.source-line,
.destination-preview {
  display: grid;
  gap: var(--space-1);
  margin-bottom: var(--space-3);
  padding: var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-medium);
  background: var(--surface-secondary);
}

.source-line span,
.destination-preview span {
  color: var(--text-secondary);
  font-size: var(--font-size-small);
}

code {
  overflow-wrap: anywhere;
  color: var(--text-primary);
  font-family: var(--font-family);
  font-size: 13px;
}

.option {
  display: grid;
  grid-template-columns: 22px minmax(0, 1fr);
  gap: var(--space-2);
  margin-bottom: var(--space-3);
  padding: var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-medium);
  background: var(--surface-primary);
}

.option.selected {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

.option > span {
  display: grid;
  justify-items: start;
  gap: var(--space-1);
}

.option small {
  color: var(--text-secondary);
}

.option em {
  color: var(--warning);
  font-size: var(--font-size-small);
  font-style: normal;
}

.destination-preview {
  margin-block: var(--space-4) 0;
  border-color: color-mix(in srgb, var(--accent) 45%, var(--border-subtle));
  background: var(--accent-subtle);
}
</style>
