export const normalizeColumnRatios = (widths: number[]): number[] => {
  if (widths.length === 0) return [];
  const safeWidths = widths.map((width) => Math.max(0, width));
  const total = safeWidths.reduce((sum, width) => sum + width, 0);
  if (total <= 0) return safeWidths.map(() => 1 / safeWidths.length);
  return safeWidths.map((width) => width / total);
};

export const projectColumnRatios = (
  ratios: number[],
  containerWidth: number,
): number[] => ratios.map((ratio) => ratio * Math.max(0, containerWidth));

export const resizeColumnPair = (
  widths: number[],
  minimums: number[],
  index: number,
  requestedDelta: number,
): number[] => {
  if (widths.length !== minimums.length || index < 0 || index >= widths.length) {
    return [...widths];
  }
  const neighbor = index === widths.length - 1 ? index - 1 : index + 1;
  if (neighbor < 0) return [...widths];

  const minimumDelta = minimums[index] - widths[index];
  const maximumDelta = widths[neighbor] - minimums[neighbor];
  const delta = Math.min(maximumDelta, Math.max(minimumDelta, requestedDelta));
  const resized = [...widths];
  resized[index] += delta;
  resized[neighbor] -= delta;
  return resized;
};
