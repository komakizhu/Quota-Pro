export type ResizeEdge = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

export interface ResizePointerSample {
  screenX: number;
  screenY: number;
}

export const COMPACT_SIZE_RANGE = { min: 48, max: 144 } as const;
export const EXPANDED_SIZE_RANGE = { min: 220, max: 460 } as const;
/** The regular edge hit area, in CSS pixels. */
export const RESIZE_EDGE_HIT_SIZE = 10;
/**
 * Corners get a larger square hit area so the diagonal cursor does not
 * require pixel-perfect positioning near a rounded corner.
 */
export const RESIZE_CORNER_HIT_SIZE = 18;

export function getOrbResizeHitSizes(size: number): { corner: number; edge: number } {
  return {
    corner: Math.min(22, Math.max(14, Math.round(size * 0.25))),
    edge: Math.min(5, Math.max(2, Math.round(size * 0.04))),
  };
}

export function getResizeEdge(
  clientX: number,
  clientY: number,
  rect: Pick<DOMRect, "left" | "top" | "right" | "bottom">,
  hitSize = RESIZE_EDGE_HIT_SIZE,
  cornerHitSize = RESIZE_CORNER_HIT_SIZE,
): ResizeEdge | null {
  const left = clientX - rect.left <= hitSize;
  const right = rect.right - clientX <= hitSize;
  const top = clientY - rect.top <= hitSize;
  const bottom = rect.bottom - clientY <= hitSize;
  const cornerLeft = clientX - rect.left <= cornerHitSize;
  const cornerRight = rect.right - clientX <= cornerHitSize;
  const cornerTop = clientY - rect.top <= cornerHitSize;
  const cornerBottom = rect.bottom - clientY <= cornerHitSize;

  // Check corners first. This keeps the diagonal affordance visible when the
  // pointer is close to two edges, even if it is just outside the regular
  // 10px edge strip.
  if (cornerTop && cornerLeft) return "nw";
  if (cornerTop && cornerRight) return "ne";
  if (cornerBottom && cornerLeft) return "sw";
  if (cornerBottom && cornerRight) return "se";
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
  // A square's corner moves along its 45° diagonal.  Use the signed
  // average of the two axis deltas so a 20px diagonal pointer movement
  // produces a 20px edge movement, rather than expanding it to 28.3px by
  // treating the diagonal distance as an edge length.
  return (deltaX * horizontalSign + deltaY * verticalSign) / 2;
}

export function clampResizeSize(size: number, range: { min: number; max: number }): number {
  return Math.min(range.max, Math.max(range.min, Math.round(size)));
}

export function resizeSizeFromPointer(startSize: number, edge: ResizeEdge, deltaX: number, deltaY: number, range: { min: number; max: number }): number {
  return clampResizeSize(startSize + resizeDelta(edge, deltaX, deltaY), range);
}

export function resizePointerDelta(start: ResizePointerSample, current: ResizePointerSample): { x: number; y: number } {
  return { x: current.screenX - start.screenX, y: current.screenY - start.screenY };
}

export function resizeHasMoved(startX: number, startY: number, currentX: number, currentY: number, threshold = 6): boolean {
  return Math.hypot(currentX - startX, currentY - startY) >= threshold;
}
