import { describe, expect, it } from "vitest";
import {
  normalizeColumnRatios,
  projectColumnRatios,
  resizeColumnPair,
} from "./columnSizing";

const minimums = [36, 160, 104, 88, 112];

describe("proportional column sizing", () => {
  it("redraws fullscreen-adjusted columns at every container width", () => {
    const ratios = normalizeColumnRatios([42, 720, 220, 180, 238]);
    const fullscreen = projectColumnRatios(ratios, 1400);
    const restored = projectColumnRatios(ratios, 820);

    expect(fullscreen.reduce((sum, width) => sum + width, 0)).toBeCloseTo(1400);
    expect(restored.reduce((sum, width) => sum + width, 0)).toBeCloseTo(820);
    expect(restored[1] / restored[4]).toBeCloseTo(fullscreen[1] / fullscreen[4]);
  });

  it("resizes adjacent columns without changing the table width", () => {
    const widths = [42, 420, 140, 120, 160];
    const resized = resizeColumnPair(widths, minimums, 1, 60);
    expect(resized).toEqual([42, 456, 104, 120, 160]);
    expect(resized.reduce((sum, width) => sum + width, 0)).toBe(
      widths.reduce((sum, width) => sum + width, 0),
    );
  });

  it("uses the previous column as the last column's resize partner", () => {
    expect(resizeColumnPair([42, 420, 140, 120, 160], minimums, 4, 20)).toEqual([
      42,
      420,
      140,
      100,
      180,
    ]);
  });
});
