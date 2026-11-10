// @vitest-environment jsdom
import { flushPromises, shallowMount, type VueWrapper } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MovesView from "./MovesView.vue";
import MoveTable from "@/components/business/MoveTable.vue";
import TaskProgress from "@/components/business/TaskProgress.vue";
import type { BootstrapData, MoveRow, TaskDescriptor, TaskEvent } from "@/types/domain";

const mocks = vi.hoisted(() => ({ listMoves: vi.fn(), startRestore: vi.fn(), recoverMove: vi.fn(), cancelTask: vi.fn() }));
vi.mock("@/services/api", () => ({ api: mocks }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: vi.fn() }) }));
const row: MoveRow = { id: "record", source: "C:\\Data", destination: "D:\\Data", createdAtUtc: "2026-11-08T00:00:00Z", status: "有效", stage: "Active", recoveryAvailable: true };
const bootstrap: BootstrapData = {
  version: "test", settings: { managedRoot: null, categoryRoots: {} }, managedLocations: [],
  drives: [], moves: [row], links: [], isAdministrator: true, defaultScanRoot: "C:\\Data",
  scanPresets: [], startupScanRoot: null, automaticManagedRoot: null, dataDirectory: "C:\\state",
};
const task: TaskDescriptor = {
  id: "task", kind: "restore", state: "running", canCancel: true, cancelRequested: false,
  percentage: null, phase: "copy", message: "copy", resultId: null,
  startedAtUtc: "2026-11-08T00:00:00Z", runningAtUtc: "2026-11-08T00:00:00Z", finishedAtUtc: null,
};
let mounted: VueWrapper | undefined;
beforeEach(() => vi.resetAllMocks());
afterEach(() => { mounted?.unmount(); mounted = undefined; });
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function mountView() {
  const wrapper = shallowMount(MovesView, {
    props: { bootstrap },
    global: { stubs: { BaseButton: false, BaseDialog: { props: ["open"], template: '<div v-if="open"><slot /><slot name="footer" /></div>' } } },
  });
  mounted = wrapper;
  wrapper.getComponent(MoveTable).vm.$emit("select", row);
  await flushPromises();
  return wrapper;
}
function button(wrapper: VueWrapper, label: string) {
  const match = wrapper.findAllComponents({ name: "BaseButton" }).find((item) => item.text() === label);
  expect(match).toBeDefined();
  return match!;
}

describe("MovesView task submission", () => {
  it("guards repeated refreshes and ignores results older than new bootstrap data", async () => {
    const pending = deferred<MoveRow[]>();
    mocks.listMoves.mockReturnValue(pending.promise);
    const wrapper = await mountView();
    const refresh = button(wrapper, "重新检查");
    refresh.vm.$emit("click");
    refresh.vm.$emit("click");
    expect(mocks.listMoves).toHaveBeenCalledTimes(1);
    await wrapper.setProps({ bootstrap: { ...bootstrap, moves: [] } });
    pending.resolve([row]);
    await flushPromises();
    expect(wrapper.getComponent(MoveTable).props("rows")).toEqual([]);
    expect(refresh.attributes("disabled")).toBeUndefined();
  });

  it("guards restore submission and disables cancellation once cutover starts", async () => {
    const pending = deferred<TaskDescriptor>();
    let onEvent!: (event: TaskEvent) => void;
    mocks.startRestore.mockImplementation((_id: string, callback: typeof onEvent) => { onEvent = callback; return pending.promise; });
    const wrapper = await mountView();
    button(wrapper, "还原选中项").vm.$emit("click");
    await flushPromises();
    const restore = button(wrapper, "开始还原");
    restore.vm.$emit("click");
    restore.vm.$emit("click");
    expect(mocks.startRestore).toHaveBeenCalledTimes(1);
    pending.resolve(task);
    await flushPromises();
    onEvent({ type: "progress", task: { ...task, phase: "switch", canCancel: false } });
    await flushPromises();
    const stop = button(wrapper, "安全收尾中");
    expect(stop.attributes("disabled")).toBeDefined();
    stop.vm.$emit("click");
    expect(mocks.cancelTask).not.toHaveBeenCalled();
    onEvent({ type: "progress", task });
    await flushPromises();
    expect(button(wrapper, "安全收尾中").attributes("disabled")).toBeDefined();
    onEvent({ type: "completed", task: { ...task, state: "completed", canCancel: false }, summary: "done" });
    await flushPromises();
    expect(button(wrapper, "重新检查").attributes("disabled")).toBeUndefined();
  });

  it("blocks repeated recovery and releases controls on failure without a fake stop action", async () => {
    const pending = deferred<string>();
    mocks.recoverMove.mockReturnValue(pending.promise);
    const wrapper = await mountView();
    const recover = button(wrapper, "恢复中断操作");
    recover.vm.$emit("click");
    recover.vm.$emit("click");
    await flushPromises();
    expect(mocks.recoverMove).toHaveBeenCalledTimes(1);
    expect(recover.attributes("disabled")).toBeDefined();
    expect(wrapper.findAll("button").some((item) => item.text() === "停止")).toBe(false);
    pending.reject(new Error("recovery failed"));
    await flushPromises();
    expect(recover.attributes("disabled")).toBeUndefined();
    expect(wrapper.emitted("notify")?.at(-1)).toEqual(["recovery failed", "error"]);
  });

  it("does not replace a new restore with an old cancellation response", async () => {
    const pendingCancel = deferred<TaskDescriptor>();
    mocks.cancelTask.mockReturnValue(pendingCancel.promise);
    let onEvent!: (event: TaskEvent) => void;
    mocks.startRestore.mockImplementation((_id: string, callback: typeof onEvent) => { onEvent = callback; return Promise.resolve(task); });
    const wrapper = await mountView();
    button(wrapper, "还原选中项").vm.$emit("click");
    await flushPromises();
    button(wrapper, "开始还原").vm.$emit("click");
    await flushPromises();
    button(wrapper, "停止").vm.$emit("click");
    onEvent({ type: "completed", task: { ...task, state: "completed", canCancel: false }, summary: "done" });
    await flushPromises();
    wrapper.getComponent(MoveTable).vm.$emit("select", row);
    mocks.startRestore.mockResolvedValueOnce({ ...task, id: "next" });
    button(wrapper, "还原选中项").vm.$emit("click");
    await flushPromises();
    button(wrapper, "开始还原").vm.$emit("click");
    await flushPromises();
    pendingCancel.resolve({ ...task, state: "cancelled", canCancel: false, cancelRequested: true });
    await flushPromises();
    expect(wrapper.getComponent(TaskProgress).props("task").id).toBe("next");
  });
});
