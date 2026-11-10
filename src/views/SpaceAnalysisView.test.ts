// @vitest-environment jsdom

import { flushPromises, shallowMount, type VueWrapper } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import SpaceAnalysisView from "./SpaceAnalysisView.vue";
import FolderTable from "@/components/business/FolderTable.vue";
import type { BootstrapData, FolderRow, ScanPage, ScanQuery, TaskDescriptor, TaskEvent } from "@/types/domain";

const mocks = vi.hoisted(() => ({
  preview: vi.fn(), scanPage: vi.fn(), startScan: vi.fn(), planMigration: vi.fn(),
}));
vi.mock("@/services/api", () => ({ api: mocks }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: vi.fn() }) }));

const root = "C:\\Data";
const bootstrap: BootstrapData = {
  version: "test", settings: { managedRoot: null, categoryRoots: {} }, managedLocations: [],
  drives: [], moves: [], links: [], isAdministrator: true, defaultScanRoot: root,
  scanPresets: [], startupScanRoot: root, automaticManagedRoot: null, dataDirectory: "C:\\state",
};
const task: TaskDescriptor = {
  id: "task", kind: "scan", state: "running", canCancel: true, cancelRequested: false, percentage: null, phase: "scan", message: "scan",
  resultId: null, startedAtUtc: "2026-11-08T00:00:00Z", runningAtUtc: "2026-11-08T00:00:00Z", finishedAtUtc: null,
};
const allRows: FolderRow[] = Array.from({ length: 1101 }, (_, index) => ({
  name: `Folder-${index}`, fullPath: `${root}\\Folder-${index}`, bytes: index,
  allocatedBytes: index, fileCount: 1, linkCount: 0, errorCount: 0, isLink: false,
  linkTarget: null, note: "", measured: true,
}));
let scanEvent: (event: TaskEvent) => void;
let mounted: VueWrapper | undefined;

const page = (number: number, items?: FolderRow[]): ScanPage => ({
  resultId: "result", root, engine: "test", createdAtUtc: "2026-11-08T00:00:00Z",
  items: items ?? allRows.slice((number - 1) * 500, number * 500), totalItems: allRows.length,
  page: number, pageSize: 500, totalBytes: 10000, totalAllocatedBytes: 10000, totalFiles: 1101,
});

beforeEach(() => {
  vi.resetAllMocks();
  mocks.preview.mockResolvedValue(allRows);
  mocks.startScan.mockImplementation((_root: string, _force: boolean, callback: typeof scanEvent) => {
    scanEvent = callback;
    return Promise.resolve(task);
  });
  mocks.scanPage.mockImplementation((_id: string, query: ScanQuery) => Promise.resolve(page(query.page)));
  mocks.planMigration.mockImplementation((source: string) => Promise.resolve({
    source, categoryKey: "other", categoryLabel: "其他", managedRoot: "D:\\Moved",
    destination: `D:\\Moved\\${source.split("\\").at(-1)}`, available: true, reason: null,
  }));
});
afterEach(() => { mounted?.unmount(); mounted = undefined; });

async function mountView(scan = true) {
  const wrapper = shallowMount(SpaceAnalysisView, {
    props: { bootstrap: { ...bootstrap, startupScanRoot: scan ? root : null } },
    global: { stubs: { BaseButton: false } },
  });
  mounted = wrapper;
  await flushPromises();
  if (scan) {
    scanEvent({ type: "completed", task: { ...task, state: "completed", resultId: "result" }, summary: "done" });
    await flushPromises();
  }
  return wrapper;
}

async function clickPage(wrapper: VueWrapper, label: string) {
  const button = wrapper.get("nav[aria-label='目录分页']").findAll("button").find((item) => item.text() === label);
  expect(button).toBeDefined();
  await button!.trigger("click");
  await flushPromises();
}

describe("SpaceAnalysisView pagination", () => {
  it("blocks repeated scan submissions until the command resolves and resets after failure", async () => {
    const wrapper = await mountView(false);
    let rejectStart!: (error: Error) => void;
    mocks.startScan.mockImplementationOnce(() => new Promise<TaskDescriptor>((_resolve, reject) => { rejectStart = reject; }));
    const scan = wrapper.findAllComponents({ name: "BaseButton" }).find((button) => button.text() === "扫描")!;
    scan.vm.$emit("click");
    scan.vm.$emit("click");
    await flushPromises();
    expect(mocks.startScan).toHaveBeenCalledTimes(1);
    expect(scan.attributes("disabled")).toBeDefined();
    rejectStart(new Error("start failed"));
    await flushPromises();
    expect(scan.attributes("disabled")).toBeUndefined();
    scan.vm.$emit("click");
    await flushPromises();
    expect(mocks.startScan).toHaveBeenCalledTimes(2);
  });
  it("drops stale scan pages when another view changes filesystem data", async () => {
    const wrapper = await mountView();
    const table = wrapper.getComponent(FolderTable);
    let finishOldPage!: (value: ScanPage) => void;
    mocks.scanPage.mockImplementationOnce(() => new Promise<ScanPage>((resolve) => { finishOldPage = resolve; }));
    table.vm.$emit("sort", "bytes");
    mocks.preview.mockResolvedValueOnce([allRows[9]]);
    await wrapper.setProps({ dataRevision: 1 });
    await flushPromises();
    finishOldPage(page(1, [allRows[88]]));
    await flushPromises();
    expect(table.props("rows")).toEqual([allRows[9]]);
    expect(wrapper.find("nav[aria-label='目录分页']").exists()).toBe(false);
  });
  it("loads every scan page and keeps selected rows available for migration across pages", async () => {
    const wrapper = await mountView();
    const table = wrapper.getComponent(FolderTable);
    expect(table.props("rows")).toHaveLength(500);
    table.vm.$emit("toggle", allRows[0]);
    await clickPage(wrapper, "下一页");
    expect(mocks.scanPage).toHaveBeenLastCalledWith("result", expect.objectContaining({ page: 2 }));
    expect(table.props("rows")[0].fullPath).toBe(allRows[500].fullPath);
    table.vm.$emit("toggle", allRows[500]);
    await clickPage(wrapper, "末页");
    expect(table.props("rows")).toHaveLength(101);
    expect(table.props("selectedPaths")).toEqual([allRows[0].fullPath, allRows[500].fullPath]);
    const migrate = wrapper.findAll("button").find((button) => button.text().includes("迁移选中项"))!;
    expect(migrate.text()).toContain("2");
    await migrate.trigger("click");
    await flushPromises();
    expect(mocks.planMigration.mock.calls.map((call) => call[0])).toEqual([allRows[0].fullPath, allRows[500].fullPath]);
  });

  it("paginates previews locally and select-all affects only the visible page", async () => {
    const wrapper = await mountView(false);
    const table = wrapper.getComponent(FolderTable);
    table.vm.$emit("toggleAll");
    await clickPage(wrapper, "下一页");
    table.vm.$emit("toggleAll");
    await flushPromises();
    expect(table.props("selectedPaths")).toHaveLength(1000);
    table.vm.$emit("toggleAll");
    await flushPromises();
    expect(table.props("selectedPaths")).toHaveLength(500);
    await clickPage(wrapper, "末页");
    expect(table.props("rows")).toHaveLength(101);
    expect(mocks.scanPage).not.toHaveBeenCalled();
  });

  it("ignores an older sort response that arrives after the newer response", async () => {
    const wrapper = await mountView();
    let resolveOld!: (value: ScanPage) => void;
    let resolveNew!: (value: ScanPage) => void;
    mocks.scanPage
      .mockImplementationOnce(() => new Promise<ScanPage>((resolve) => { resolveOld = resolve; }))
      .mockImplementationOnce(() => new Promise<ScanPage>((resolve) => { resolveNew = resolve; }));
    const table = wrapper.getComponent(FolderTable);
    table.vm.$emit("sort", "bytes");
    table.vm.$emit("sort", "fileCount");
    resolveNew(page(1, [allRows[77]]));
    await flushPromises();
    resolveOld(page(1, [allRows[88]]));
    await flushPromises();
    expect(table.props("rows")).toEqual([allRows[77]]);
    expect(table.props("sortField")).toBe("fileCount");
  });
});
