import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRefreshScheduler } from "./refreshScheduler";
beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); });

describe("refresh scheduling", () => {
  it("coalesces bursts and rejects responses made stale by another change", async () => {
    let first!: (value: number) => void;
    let second!: (value: number) => void;
    const load = vi.fn()
      .mockImplementationOnce(() => new Promise<number>((resolve) => { first = resolve; }))
      .mockImplementationOnce(() => new Promise<number>((resolve) => { second = resolve; }));
    const apply = vi.fn();
    const scheduler = createRefreshScheduler({ load, apply, onError: vi.fn() });
    for (let index = 0; index < 20; index++) scheduler.request();
    await vi.advanceTimersByTimeAsync(120);
    expect(load).toHaveBeenCalledTimes(1);
    scheduler.request();
    first(1);
    await vi.advanceTimersByTimeAsync(120);
    expect(apply).not.toHaveBeenCalled();
    expect(load).toHaveBeenCalledTimes(2);
    second(2);
    await vi.advanceTimersByTimeAsync(0);
    expect(apply).toHaveBeenCalledWith(2);
    scheduler.dispose();
  });

  it("does not publish or reschedule after disposal", async () => {
    let finish!: (value: number) => void;
    const load = vi.fn(() => new Promise<number>((resolve) => { finish = resolve; }));
    const apply = vi.fn();
    const scheduler = createRefreshScheduler({ load, apply, onError: vi.fn() });
    scheduler.request();
    await vi.advanceTimersByTimeAsync(120);
    scheduler.request();
    scheduler.dispose();
    finish(1);
    await vi.runAllTimersAsync();
    expect(load).toHaveBeenCalledTimes(1);
    expect(apply).not.toHaveBeenCalled();
  });
});
