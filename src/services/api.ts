import { Channel } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AppSettings,
  BootstrapData,
  FolderRow,
  LinkRow,
  MigrationPlan,
  MoveRow,
  ScanPage,
  ScanQuery,
  TaskDescriptor,
  TaskEvent,
} from "@/types/domain";
import { invokeCommand } from "./tauri";

export const api = {
  bootstrap(): Promise<BootstrapData> {
    return invokeCommand("get_bootstrap_data");
  },

  saveSettings(settings: AppSettings): Promise<BootstrapData> {
    return invokeCommand("save_settings", { settings });
  },

  preview(path: string): Promise<FolderRow[]> {
    return invokeCommand("list_child_folders", { path });
  },

  startScan(
    root: string,
    force: boolean,
    onEvent: (event: TaskEvent) => void,
  ): Promise<TaskDescriptor> {
    return startTask("start_scan", { root, force }, onEvent);
  },

  cancelTask(taskId: string): Promise<TaskDescriptor> {
    return invokeCommand("cancel_task", { taskId });
  },

  scanPage(resultId: string, query: ScanQuery): Promise<ScanPage> {
    return invokeCommand("query_scan_result", { resultId, query });
  },

  detectLinks(root: string): Promise<LinkRow[]> {
    return invokeCommand("detect_links", { root });
  },

  planMigration(
    source: string,
    customRoot: string | null,
  ): Promise<MigrationPlan> {
    return invokeCommand("plan_migration", { source, customRoot });
  },

  startMigration(
    source: string,
    destination: string,
    onEvent: (event: TaskEvent) => void,
  ): Promise<TaskDescriptor> {
    return startTask(
      "start_migration",
      { source, destination },
      onEvent,
    );
  },

  startRestore(
    recordId: string,
    onEvent: (event: TaskEvent) => void,
  ): Promise<TaskDescriptor> {
    return startTask("start_restore", { recordId }, onEvent);
  },

  recoverMove(recordId: string): Promise<string> {
    return invokeCommand("recover_move", { recordId });
  },

  listMoves(): Promise<MoveRow[]> {
    return invokeCommand("list_moves");
  },

  openPath(path: string, select = false): Promise<void> {
    return invokeCommand("open_path", { path, select });
  },

  openManagedRoot(): Promise<void> {
    return invokeCommand("open_managed_root");
  },

  requestElevation(scanRoot: string | null): Promise<boolean> {
    return invokeCommand("request_elevation", { scanRoot });
  },

  async chooseDirectory(title: string, defaultPath?: string): Promise<string | null> {
    const selected = await open({
      title,
      directory: true,
      multiple: false,
      ...(defaultPath ? { defaultPath } : {}),
    });
    return typeof selected === "string" ? selected : null;
  },
};

async function startTask(
  command: string,
  args: Record<string, unknown>,
  onEvent: (event: TaskEvent) => void,
): Promise<TaskDescriptor> {
  const channel = new Channel<TaskEvent>();
  channel.onmessage = onEvent;
  return invokeCommand<TaskDescriptor>(command, { ...args, onEvent: channel });
}
