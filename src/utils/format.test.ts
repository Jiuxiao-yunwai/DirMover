import { describe, expect, it } from "vitest";
import { formatBytes, formatDate, formatInteger } from "./format";

describe("format utilities", () => {
  it("formats byte values using compact binary units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1.50 KB");
    expect(formatBytes(5 * 1024 ** 3)).toBe("5.00 GB");
  });

  it("rejects invalid sizes and dates without throwing", () => {
    expect(formatBytes(-1)).toBe("—");
    expect(formatBytes(Number.NaN)).toBe("—");
    expect(formatDate("not-a-date")).toBe("—");
    expect(formatDate(null)).toBe("—");
  });

  it("formats integer counts for the Chinese locale", () => {
    expect(formatInteger(1234567)).toBe("1,234,567");
  });
});
