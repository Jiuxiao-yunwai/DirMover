import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TaskDescriptor, TaskEvent } from "@/types/domain";
const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ Channel: class { onmessage?: (event: TaskEvent) => void }, invoke: mocks.invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
import { api } from "./api";

const task: TaskDescriptor = {
  id: "id", kind: "scan", state: "running", canCancel: true, cancelRequested: false, percentage: null, phase: "prepare", message: "prepare",
  resultId: null, startedAtUtc: "2026-11-08T00:00:00Z", runningAtUtc: "2026-11-08T00:00:00Z", finishedAtUtc: null,
};
beforeEach(() => { vi.resetAllMocks(); });

describe("task event ordering", () => {
  it.each(["scan", "restore"])("keeps an early completion when the %s start response arrives later", async (kind) => {
    const completed: TaskEvent = { type: "completed", task: { ...task, state: "completed", percentage: 100 }, summary: "done" };
    mocks.invoke.mockImplementation((_command, args: { onEvent: { onmessage: (event: TaskEvent) => void } }) => {
      args.onEvent.onmessage(completed);
      args.onEvent.onmessage({ type: "progress", task });
      return Promise.resolve({ ...task, state: "queued" });
    });
    const receive = vi.fn();
    const result = kind === "scan" ? await api.startScan("C:\\data", false, receive) : await api.startRestore("record", receive);
    expect(result.state).toBe("completed");
    expect(receive).toHaveBeenCalledTimes(1);
  });

  it("keeps the newest phase and independent states in a batch", async () => {
    mocks.invoke.mockImplementation((_command, args: { onEvent: { onmessage: (event: TaskEvent) => void } }) => {
      args.onEvent.onmessage({ type: "progress", task: { ...task, phase: "copy" } });
      args.onEvent.onmessage({ type: "failed", task: { ...task, id: "second", state: "failed" }, message: "failed", code: "io", recoverable: true });
      return Promise.resolve([task, { ...task, id: "second", state: "queued" }]);
    });
    const result = await api.startMigrations([], vi.fn());
    expect(result.map((entry) => [entry.phase, entry.state])).toEqual([["copy", "running"], ["prepare", "failed"]]);
  });
});
