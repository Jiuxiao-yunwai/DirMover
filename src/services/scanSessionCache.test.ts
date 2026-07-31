import { describe, expect, it } from "vitest";
import type { ScanPage } from "@/types/domain";
import {
  createScanSessionCache,
  type CachedScanState,
} from "./scanSessionCache";

const cachedState = (root: string, resultId: string): CachedScanState => ({
  page: {
    resultId,
    root,
    engine: "test",
    createdAtUtc: "2026-07-31T00:00:00Z",
    items: [],
    totalItems: 0,
    page: 1,
    pageSize: 500,
    totalBytes: 0,
    totalAllocatedBytes: 0,
    totalFiles: 0,
  } satisfies ScanPage,
  sortField: "allocatedBytes",
  sortDirection: "descending",
});

describe("scan session cache", () => {
  it("restores a scan for equivalent Windows paths", () => {
    const cache = createScanSessionCache();
    const state = cachedState("D:\\Games", "one");
    cache.set("D:\\Games\\", state);

    expect(cache.get("d:/games")).toBe(state);
  });

  it("keeps only the most recently used scan states", () => {
    const cache = createScanSessionCache(2);
    cache.set("C:\\One", cachedState("C:\\One", "one"));
    cache.set("C:\\Two", cachedState("C:\\Two", "two"));
    cache.get("C:\\One");
    cache.set("C:\\Three", cachedState("C:\\Three", "three"));

    expect(cache.get("C:\\One")?.page.resultId).toBe("one");
    expect(cache.get("C:\\Two")).toBeUndefined();
    expect(cache.get("C:\\Three")?.page.resultId).toBe("three");
  });
});
