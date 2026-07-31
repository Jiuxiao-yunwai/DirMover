import { invoke } from "@tauri-apps/api/core";
import type { AppError } from "@/types/domain";

export async function invokeCommand<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error: unknown) {
    throw normalizeAppError(error);
  }
}

export function normalizeAppError(error: unknown): AppError {
  if (isAppError(error)) {
    return error;
  }
  if (typeof error === "string") {
    try {
      const parsed: unknown = JSON.parse(error);
      if (isAppError(parsed)) {
        return parsed;
      }
    } catch {
      return { code: "unknown", message: error, recoverable: true };
    }
    return { code: "unknown", message: error, recoverable: true };
  }
  if (error instanceof Error) {
    return { code: "unknown", message: error.message, recoverable: true };
  }
  return {
    code: "unknown",
    message: "操作未完成，请稍后重试。",
    recoverable: true,
  };
}

function isAppError(value: unknown): value is AppError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" &&
    typeof candidate.message === "string" &&
    typeof candidate.recoverable === "boolean"
  );
}
