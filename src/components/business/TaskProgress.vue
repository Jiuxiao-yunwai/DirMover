<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import BaseProgress from "@/components/base/BaseProgress.vue";
import type { TaskDescriptor } from "@/types/domain";
import { formatElapsedTime, taskElapsedMs, taskStepLabel } from "@/utils/taskState";

const props = defineProps<{ task: TaskDescriptor }>();
const now = ref(Date.now());
let timer: ReturnType<typeof setInterval> | undefined;
watch(() => props.task.state, (state) => {
  if (timer) clearInterval(timer);
  timer = undefined;
  now.value = Date.now();
  if (state === "running") {
    timer = setInterval(() => { now.value = Date.now(); }, 1000);
  }
}, { immediate: true });
onBeforeUnmount(() => { if (timer) clearInterval(timer); });
const elapsed = computed(() => taskElapsedMs(props.task, now.value));
const timedLabel = computed(() => elapsed.value === null
  ? (props.task.state === "queued" ? "等待执行" : "处理中")
  : `已用时 ${formatElapsedTime(elapsed.value)}`);
const measured = computed(() => props.task.percentage !== null);
const step = computed(() => taskStepLabel(props.task));
</script>

<template>
  <section class="task-progress-display" :aria-label="task.message">
    <div class="progress-heading">
      <span class="phase-message">{{ task.message }}</span>
      <strong v-if="measured">{{ task.percentage }}%</strong>
    </div>
    <BaseProgress
      :value="task.percentage ?? 0"
      :indeterminate="task.state === 'running' && !measured"
      :aria-label="task.message"
      :aria-valuetext="measured ? `${task.percentage}%` : task.message"
    />
    <small>{{ timedLabel }}<template v-if="step"> · {{ step }}</template><template v-if="measured && task.state === 'running'"> · 当前阶段进度</template></small>
    <small v-if="task.state === 'running' && !task.canCancel" role="status">{{ task.cancelRequested ? "正在安全停止，请稍候…" : "安全收尾中，暂不能取消，请等待完成。" }}</small>
  </section>
</template>

<style scoped>
.task-progress-display { display: grid; min-width: 0; gap: 6px; }
.progress-heading { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
.phase-message { min-width: 0; overflow-wrap: anywhere; }
strong { color: var(--accent); white-space: nowrap; font-variant-numeric: tabular-nums; }
small { color: var(--text-secondary); font-size: var(--font-size-small); font-variant-numeric: tabular-nums; }
</style>
