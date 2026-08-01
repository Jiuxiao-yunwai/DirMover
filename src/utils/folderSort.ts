import type { FolderRow, SortDirection, SortField } from "@/types/domain";

const compareNames = (left: FolderRow, right: FolderRow): number =>
  left.name.localeCompare(right.name, "zh-CN", {
    numeric: true,
    sensitivity: "base",
  });

const statusRank = (row: FolderRow): number => {
  if (!row.measured && !row.isLink) return 0;
  if (row.errorCount > 0) return 1;
  if (row.isLink) return 2;
  return 3;
};

const compareByField = (left: FolderRow, right: FolderRow, field: SortField): number => {
  switch (field) {
    case "name":
      return compareNames(left, right);
    case "bytes":
      return left.bytes - right.bytes;
    case "allocatedBytes":
      return left.allocatedBytes - right.allocatedBytes;
    case "fileCount":
      return left.fileCount - right.fileCount;
    case "status":
      return statusRank(left) - statusRank(right);
  }
};

export const sortFolderRows = (
  rows: FolderRow[],
  field: SortField,
  direction: SortDirection,
): FolderRow[] => {
  const multiplier = direction === "ascending" ? 1 : -1;
  return [...rows].sort((left, right) => {
    const comparison = compareByField(left, right, field) || compareNames(left, right);
    return comparison * multiplier;
  });
};
