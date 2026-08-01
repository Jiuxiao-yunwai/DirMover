import { describe, expect, it } from "vitest";
import type { FolderRow } from "@/types/domain";
import { sortFolderRows } from "./folderSort";

const row = (name: string, overrides: Partial<FolderRow> = {}): FolderRow => ({
  name,
  fullPath: `C:\\Root\\${name}`,
  bytes: 0,
  allocatedBytes: 0,
  fileCount: 0,
  linkCount: 0,
  errorCount: 0,
  isLink: false,
  linkTarget: null,
  note: "未扫描",
  measured: false,
  ...overrides,
});

describe("folder preview sorting", () => {
  it("honors descending name order before a scan exists", () => {
    const sorted = sortFolderRows([row("Alpha"), row("Beta")], "name", "descending");
    expect(sorted.map(({ name }) => name)).toEqual(["Beta", "Alpha"]);
  });

  it("uses the selected numeric field and direction", () => {
    const sorted = sortFolderRows(
      [row("Small", { allocatedBytes: 10 }), row("Large", { allocatedBytes: 20 })],
      "allocatedBytes",
      "descending",
    );
    expect(sorted.map(({ name }) => name)).toEqual(["Large", "Small"]);
  });

  it("can group preview links by status", () => {
    const sorted = sortFolderRows(
      [row("Link", { isLink: true, linkCount: 1 }), row("Folder")],
      "status",
      "ascending",
    );
    expect(sorted.map(({ name }) => name)).toEqual(["Folder", "Link"]);
  });
});
