// @vitest-environment jsdom

import { shallowMount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import FolderTable from "./FolderTable.vue";
import type { FolderRow } from "@/types/domain";

const row = (name: string): FolderRow => ({
  name,
  fullPath: `C:\\Root\\${name}`,
  bytes: 100,
  allocatedBytes: 128,
  fileCount: 1,
  linkCount: 0,
  errorCount: 0,
  isLink: false,
  linkTarget: null,
  note: "",
  measured: true,
});

const rows = [row("One"), row("Two")];

describe("FolderTable multi-selection", () => {
  it("toggles an individual row without replacing the other selections", async () => {
    const wrapper = shallowMount(FolderTable, {
      props: {
        rows,
        selectedPaths: [rows[0].fullPath],
        sortField: "name",
        sortDirection: "ascending",
        totalAllocatedBytes: 256,
      },
    });

    const checkboxes = wrapper.findAll<HTMLInputElement>("tbody input[type='checkbox']");
    expect(checkboxes[0].element.checked).toBe(true);
    expect(checkboxes[1].element.checked).toBe(false);

    await wrapper.findAll("tbody tr")[1].trigger("click");
    expect(wrapper.emitted("toggle")?.[0]).toEqual([rows[1]]);
  });

  it("reports select-all and exposes the mixed selection state", async () => {
    const wrapper = shallowMount(FolderTable, {
      props: {
        rows,
        selectedPaths: [rows[0].fullPath],
        sortField: "name",
        sortDirection: "ascending",
        totalAllocatedBytes: 256,
      },
    });

    const selectAll = wrapper.find<HTMLInputElement>("thead input[type='checkbox']");
    expect(selectAll.element.indeterminate).toBe(true);
    await selectAll.trigger("change");
    expect(wrapper.emitted("toggleAll")).toHaveLength(1);
  });

  it("resizes every column from its keyboard-accessible separator", async () => {
    const wrapper = shallowMount(FolderTable, {
      props: {
        rows,
        selectedPaths: [],
        sortField: "name",
        sortDirection: "ascending",
        totalAllocatedBytes: 256,
      },
    });

    const handles = wrapper.findAll(".resize-handle");
    expect(handles).toHaveLength(5);
    const headerWidths = [42, 420, 140, 120, 160];
    wrapper.findAll("thead th").forEach((header, index) => {
      header.element.getBoundingClientRect = () => ({
        width: headerWidths[index],
      }) as DOMRect;
    });
    await handles[1].trigger("keydown", { key: "ArrowRight" });

    const folderPercentage = Number.parseFloat(
      wrapper.findAll<HTMLTableColElement>("col")[1].element.style.width,
    );
    expect(folderPercentage).toBeCloseTo((432 / 882) * 100);
    expect(wrapper.find("table").attributes("style")).toBeUndefined();
  });
});
