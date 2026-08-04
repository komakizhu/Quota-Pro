export type ResizeEdge = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

export const COMPACT_SIZE_RANGE = { min: 48, max: 144 } as const;
export const EXPANDED_SIZE_RANGE = { min: 220, max: 460 } as const;

export function getResizeEdge(clientX: number, clientY: number, rect: Pick<DOMRect, "left" | "top" | "right" | "bottom">, hitSize = 10): ResizeEdge | null {
  const left = clientX - rect.left <= hitSize;
  const right = rect.right - clientX <= hitSize;
  const top = clientY - rect.top <= hitSize;
  const bottom = rect.bottom - clientY <= hitSize;
  if (top && left) return "nw";
  if (top && right) return "ne";
  if (bottom && left) return "sw";
  if (bottom && right) return "se";
  if (top) return "n";
  if (bottom) return "s";
  if (left) return "w";
  if (right) return "e";
  return null;
}

export function resizeDelta(edge: ResizeEdge, deltaX: number, deltaY: number): number {
  if (edge === "e") return deltaX;
  if (edge === "w") return -deltaX;
  if (edge === "s") return deltaY;
  if (edge === "n") return -deltaY;
  const horizontalSign = edge.endsWith("e") ? 1 : -1;
  const verticalSign = edge.startsWith("s") ? 1 : -1;
  return (deltaX * horizontalSign + deltaY * verticalSign) / Math.sqrt(2);
}

export function clampResizeSize(size: number, range: { min: number; max: number }): number {
  return Math.min(range.max, Math.max(range.min, Math.round(size)));
}

export function resizeSizeFromPointer(startSize: number, edge: ResizeEdge, deltaX: number, deltaY: number, range: { min: number; max: number }): number {
  return clampResizeSize(startSize + resizeDelta(edge, deltaX, deltaY), range);
}

export function resizeHasMoved(startX: number, startY: number, currentX: number, currentY: number, threshold = 6): boolean {
  return Math.hypot(currentX - startX, currentY - startY) >= threshold;
}
