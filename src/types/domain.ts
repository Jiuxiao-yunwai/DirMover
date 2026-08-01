export type AppError = {
  code: string;
  message: string;
  recoverable: boolean;
};

export type AppSettings = {
  managedRoot: string | null;
  categoryRoots: Record<string, string>;
};

export type ManagedCategoryRow = {
  key: string;
  label: string;
  path: string | null;
  overridden: boolean;
};

export type DriveInfo = {
  root: string;
  fileSystem: string;
  totalBytes: number;
  freeBytes: number;
  fixed: boolean;
  ready: boolean;
};

export type ScanPreset = {
  key: string;
  label: string;
  path: string;
};

export type LinkRow = {
  kind: string;
  path: string;
  target: string;
  state: string;
  scanRoot: string;
  detectedAtUtc: string | null;
};

export type MoveRow = {
  id: string;
  status: string;
  source: string;
  destination: string;
  createdAtUtc: string;
  stage: string;
  recoveryAvailable: boolean;
};

export type FolderRow = {
  name: string;
  fullPath: string;
  bytes: number;
  allocatedBytes: number;
  fileCount: number;
  linkCount: number;
  errorCount: number;
  isLink: boolean;
  linkTarget: string | null;
  note: string;
  measured: boolean;
};

export type BootstrapData = {
  version: string;
  settings: AppSettings;
  managedLocations: ManagedCategoryRow[];
  drives: DriveInfo[];
  moves: MoveRow[];
  links: LinkRow[];
  isAdministrator: boolean;
  defaultScanRoot: string;
  scanPresets: ScanPreset[];
  startupScanRoot: string | null;
  automaticManagedRoot: string | null;
  dataDirectory: string;
};

export type MigrationPlan = {
  source: string;
  categoryKey: string;
  categoryLabel: string;
  managedRoot: string | null;
  destination: string | null;
  available: boolean;
  reason: string | null;
};

export type MigrationRequest = {
  source: string;
  destination: string;
};

export type SortField =
  | "name"
  | "bytes"
  | "allocatedBytes"
  | "fileCount"
  | "status";

export type SortDirection = "ascending" | "descending";

export type ScanQuery = {
  page: number;
  pageSize: number;
  sortField: SortField;
  sortDirection: SortDirection;
};

export type ScanPage = {
  resultId: string;
  root: string;
  engine: string;
  createdAtUtc: string;
  items: FolderRow[];
  totalItems: number;
  page: number;
  pageSize: number;
  totalBytes: number;
  totalAllocatedBytes: number;
  totalFiles: number;
};

export type TaskKind = "scan" | "migration" | "restore";
export type TaskState = "queued" | "running" | "completed" | "failed" | "cancelled";

export type TaskDescriptor = {
  id: string;
  kind: TaskKind;
  state: TaskState;
  percentage: number;
  phase: string;
  message: string;
  resultId: string | null;
  startedAtUtc: string;
  finishedAtUtc: string | null;
};

export type TaskEvent =
  | { type: "progress"; task: TaskDescriptor }
  | { type: "completed"; task: TaskDescriptor; summary: string }
  | {
      type: "failed";
      task: TaskDescriptor;
      code: string;
      message: string;
      recoverable: boolean;
    }
  | { type: "cancelled"; task: TaskDescriptor };

export type ThemePreference = "system" | "light" | "dark";
