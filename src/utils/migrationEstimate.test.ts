import { describe, expect, it } from "vitest";
import { estimateMigrationQueue, type MigrationEstimateEntry } from "./migrationEstimate";

const entry = (
  id: string,
  overrides: Partial<MigrationEstimateEntry>,
): MigrationEstimateEntry => ({
  id,
  state: "queued",
  percentage: 0,
  bytes: 1_000,
  runningAtMs: null,
  finishedAtMs: null,
  ...overrides,
});

describe("migration queue estimates", () => {
  it("estimates running time from elapsed time and progress", () => {
    const result = estimateMigrationQueue([
      entry("running", { state: "running", percentage: 25, runningAtMs: 0 }),
    ], 10_000);
    expect(result.entries.running.remainingMs).toBe(30_000);
    expect(result.totalRemainingMs).toBe(30_000);
  });

  it("uses sampled throughput to estimate queued start and duration", () => {
    const result = estimateMigrationQueue([
      entry("running", {
        state: "running",
        percentage: 50,
        bytes: 2_000,
        runningAtMs: 0,
      }),
      entry("queued", { bytes: 4_000 }),
    ], 10_000);
    expect(result.entries.queued.waitMs).toBe(10_000);
    expect(result.entries.queued.durationMs).toBe(40_000);
    expect(result.totalRemainingMs).toBe(50_000);
  });

  it("reports an unknown estimate until enough speed data exists", () => {
    const result = estimateMigrationQueue([
      entry("running", { state: "running", percentage: 0, runningAtMs: 0 }),
      entry("queued", {}),
    ], 1_000);
    expect(result.entries.running.remainingMs).toBeNull();
    expect(result.entries.queued.waitMs).toBeNull();
    expect(result.totalRemainingMs).toBeNull();
  });
});
