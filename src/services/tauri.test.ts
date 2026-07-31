import { describe, expect, it } from "vitest";
import { normalizeAppError } from "./tauri";

describe("normalizeAppError", () => {
  it("preserves structured command errors", () => {
    const error = {
      code: "invalid_input",
      message: "路径不可用",
      recoverable: true,
    };
    expect(normalizeAppError(error)).toEqual(error);
    expect(normalizeAppError(JSON.stringify(error))).toEqual(error);
  });

  it("normalizes native and unknown errors", () => {
    expect(normalizeAppError(new Error("failed")).message).toBe("failed");
    expect(normalizeAppError(null)).toEqual({
      code: "unknown",
      message: "操作未完成，请稍后重试。",
      recoverable: true,
    });
  });
});
