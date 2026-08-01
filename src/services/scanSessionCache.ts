import type {
  ScanPage,
  SortDirection,
  SortField,
} from "@/types/domain";

export type CachedScanState = {
  page: ScanPage;
  sortField: SortField;
  sortDirection: SortDirection;
};

const normalizeRoot = (root: string): string =>
  root.trim().replaceAll("/", "\\").replace(/\\+$/, "").toLowerCase();

export const createScanSessionCache = (capacity = 8) => {
  const entries = new Map<string, CachedScanState>();
  const limit = Math.max(1, Math.trunc(capacity));

  return {
    get(root: string): CachedScanState | undefined {
      const key = normalizeRoot(root);
      const entry = entries.get(key);
      if (!entry) return undefined;
      entries.delete(key);
      entries.set(key, entry);
      return entry;
    },

    set(root: string, entry: CachedScanState): void {
      const key = normalizeRoot(root);
      entries.delete(key);
      entries.set(key, entry);
      while (entries.size > limit) {
        const oldest = entries.keys().next().value;
        if (oldest === undefined) break;
        entries.delete(oldest);
      }
    },

    clear(): void {
      entries.clear();
    },
  };
};
