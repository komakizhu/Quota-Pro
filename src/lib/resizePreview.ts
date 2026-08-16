export interface ResizePreviewScheduler {
  schedule(size: number): void;
  flush(size: number): void;
  cancel(): void;
}

export function createResizePreviewScheduler(apply: (size: number) => void): ResizePreviewScheduler {
  let frame: number | null = null;
  let pendingSize: number | null = null;
  let appliedSize: number | null = null;

  const run = () => {
    frame = null;
    if (pendingSize === null) return;
    const size = pendingSize;
    pendingSize = null;
    if (size === appliedSize) return;
    appliedSize = size;
    apply(size);
  };

  return {
    schedule(size) {
      pendingSize = Math.round(size);
      if (frame === null) frame = window.requestAnimationFrame(run);
    },
    flush(size) {
      if (frame !== null) window.cancelAnimationFrame(frame);
      frame = null;
      pendingSize = null;
      if (Math.round(size) === appliedSize) return;
      appliedSize = Math.round(size);
      apply(Math.round(size));
    },
    cancel() {
      if (frame !== null) window.cancelAnimationFrame(frame);
      frame = null;
      pendingSize = null;
      appliedSize = null;
    },
  };
}
