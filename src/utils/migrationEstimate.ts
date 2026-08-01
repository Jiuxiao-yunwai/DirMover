export type MigrationEstimateEntry = {
  id: string;
  state: "queued" | "running" | "completed" | "failed" | "cancelled";
  percentage: number;
  bytes: number;
  runningAtMs: number | null;
  finishedAtMs: number | null;
};

export type EntryTimeEstimate = {
  elapsedMs: number | null;
  remainingMs: number | null;
  waitMs: number | null;
  durationMs: number | null;
};

export type MigrationQueueEstimate = {
  entries: Record<string, EntryTimeEstimate>;
  totalRemainingMs: number | null;
};

const runtimeMs = (entry: MigrationEstimateEntry, nowMs: number): number | null => {
  if (entry.runningAtMs === null) return null;
  return Math.max(0, (entry.finishedAtMs ?? nowMs) - entry.runningAtMs);
};

export const estimateMigrationQueue = (
  entries: MigrationEstimateEntry[],
  nowMs: number,
): MigrationQueueEstimate => {
  let sampledBytes = 0;
  let sampledTimeMs = 0;
  const completedDurations: number[] = [];

  for (const entry of entries) {
    const elapsed = runtimeMs(entry, nowMs);
    if (entry.state === "completed" && elapsed !== null && elapsed > 0) {
      completedDurations.push(elapsed);
      if (entry.bytes > 0) {
        sampledBytes += entry.bytes;
        sampledTimeMs += elapsed;
      }
    } else if (
      entry.state === "running"
      && elapsed !== null
      && elapsed > 0
      && entry.bytes > 0
      && entry.percentage >= 2
    ) {
      sampledBytes += entry.bytes * Math.min(100, entry.percentage) / 100;
      sampledTimeMs += elapsed;
    }
  }

  const throughput = sampledBytes > 0 && sampledTimeMs > 0
    ? sampledBytes / sampledTimeMs
    : null;
  const averageDuration = completedDurations.length > 0
    ? completedDurations.reduce((sum, duration) => sum + duration, 0)
      / completedDurations.length
    : null;
  const durationFor = (entry: MigrationEstimateEntry): number | null => {
    if (throughput !== null && entry.bytes > 0) return entry.bytes / throughput;
    return averageDuration;
  };

  const estimates: Record<string, EntryTimeEstimate> = {};
  let accumulatedRemaining = 0;
  let remainingKnown = true;

  for (const entry of entries) {
    const elapsed = runtimeMs(entry, nowMs);
    let remaining: number | null = null;
    let wait: number | null = null;
    let duration: number | null = null;

    if (entry.state === "running") {
      if (elapsed !== null && entry.percentage >= 1) {
        remaining = elapsed * (100 - Math.min(100, entry.percentage)) / entry.percentage;
      } else {
        duration = durationFor(entry);
        if (duration !== null && elapsed !== null) remaining = Math.max(0, duration - elapsed);
      }
      if (remaining === null) remainingKnown = false;
      else if (remainingKnown) accumulatedRemaining += remaining;
    } else if (entry.state === "queued") {
      wait = remainingKnown ? accumulatedRemaining : null;
      duration = durationFor(entry);
      if (duration === null) remainingKnown = false;
      else if (remainingKnown) accumulatedRemaining += duration;
    } else if (entry.state === "completed") {
      duration = elapsed;
    }

    estimates[entry.id] = {
      elapsedMs: elapsed,
      remainingMs: remaining,
      waitMs: wait,
      durationMs: duration,
    };
  }

  return {
    entries: estimates,
    totalRemainingMs: remainingKnown ? accumulatedRemaining : null,
  };
};
