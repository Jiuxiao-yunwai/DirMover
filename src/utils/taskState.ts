import type { TaskDescriptor } from "@/types/domain";

export const isTerminalTask = (task: TaskDescriptor): boolean =>
  ["completed", "failed", "cancelled"].includes(task.state);

export const mergeTaskState = (
  current: TaskDescriptor | undefined,
  incoming: TaskDescriptor,
): TaskDescriptor => {
  if (!current || current.id !== incoming.id) return incoming;
  if (isTerminalTask(current)) return current;
  if (current.state === "running" && incoming.state === "queued") return current;
  if (!isTerminalTask(incoming) && !current.canCancel && incoming.canCancel) return current;
  if (!isTerminalTask(incoming) && current.cancelRequested && !incoming.cancelRequested) {
    return { ...incoming, canCancel: current.canCancel && incoming.canCancel, cancelRequested: current.cancelRequested || incoming.cancelRequested };
  }
  return incoming;
};

export const taskElapsedMs = (task: TaskDescriptor, now = Date.now()): number | null => {
  if (!task.runningAtUtc) return null;
  const start = Date.parse(task.runningAtUtc);
  const end = task.finishedAtUtc ? Date.parse(task.finishedAtUtc) : now;
  return Number.isFinite(start) && Number.isFinite(end) ? Math.max(0, end - start) : null;
};

export const formatElapsedTime = (milliseconds: number): string => {
  const seconds = Math.max(0, Math.floor(milliseconds / 1000));
  if (seconds < 60) return `${seconds} 秒`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} 分 ${seconds % 60} 秒`;
  return `${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分`;
};

export const taskStepLabel = (task: TaskDescriptor): string | null => {
  const phases = task.kind === "migration"
    ? ["measure", "copy", "cutover", "sync", "junction", "cleanup"]
    : task.kind === "restore" ? ["measure", "copy", "switch", "sync", "cleanup"] : [];
  const index = phases.indexOf(task.phase);
  return index < 0 ? null : `第 ${index + 1}/${phases.length} 步`;
};
