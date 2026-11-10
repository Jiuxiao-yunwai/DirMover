// @vitest-environment jsdom
import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import TaskProgress from "./TaskProgress.vue";
import type { TaskDescriptor } from "@/types/domain";

const task: TaskDescriptor = {
  id: "task", kind: "migration", state: "running", canCancel: true, cancelRequested: false, percentage: null, phase: "copy", message: "正在复制文件",
  resultId: null, startedAtUtc: "2026-11-08T00:00:00Z", runningAtUtc: "2026-11-08T00:00:10Z", finishedAtUtc: null,
};
afterEach(() => { vi.useRealTimers(); });
describe("TaskProgress", () => {
  it("distinguishes stopping from an uncancellable cutover", async () => {
    const wrapper = mount(TaskProgress, { props: { task: { ...task, phase: "cutover", canCancel: false } } });
    expect(wrapper.get('[role="status"]').text()).toContain("安全收尾中");
    await wrapper.setProps({ task: { ...task, canCancel: false, cancelRequested: true } });
    expect(wrapper.get('[role="status"]').text()).toContain("正在安全停止");
    wrapper.unmount();
  });
  it("animates unknown work, shows elapsed time, and never invents a percentage or ETA", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-08T00:00:15Z"));
    const wrapper = mount(TaskProgress, { props: { task } });
    expect(wrapper.text()).toContain("已用时 5 秒");
    expect(wrapper.text()).toContain("第 2/6 步");
    expect(wrapper.text()).not.toMatch(/%|剩余|预计/);
    expect(wrapper.find(".indeterminate").exists()).toBe(true);
    expect(wrapper.get("[role='progressbar']").attributes("aria-valuenow")).toBeUndefined();
    await vi.advanceTimersByTimeAsync(2000);
    expect(wrapper.text()).toContain("已用时 7 秒");
    wrapper.unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("shows measured stage progress and leaves queued jobs stationary", async () => {
    const wrapper = mount(TaskProgress, { props: { task: { ...task, kind: "scan", phase: "mft-read", percentage: 42 } } });
    expect(wrapper.text()).toContain("42%");
    expect(wrapper.text()).toContain("当前阶段进度");
    expect(wrapper.find(".indeterminate").exists()).toBe(false);
    await wrapper.setProps({ task: { ...task, state: "queued", runningAtUtc: null } });
    expect(wrapper.text()).toContain("等待执行");
    expect(wrapper.find(".indeterminate").exists()).toBe(false);
    wrapper.unmount();
  });
});
