import { useEffect, useState } from "react";

function readDevicePixelRatio(): number {
  if (typeof window === "undefined") return 1;
  const ratio = window.devicePixelRatio;
  return Number.isFinite(ratio) && ratio > 0 ? ratio : 1;
}

/**
 * Keep the CSS footprint on the same grid as the physical WebView pixels.
 * Retina displays commonly use a 2x scale, so half-pixel logical sizes are
 * valid and avoid the fractional layer interpolation that makes text soft.
 */
export function snapLogicalSizeToDevicePixels(size: number, devicePixelRatio = readDevicePixelRatio()): number {
  if (!Number.isFinite(size)) return 0;
  const ratio = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1;
  return Math.round(size * ratio) / ratio;
}

export function widgetScaleForSize(size: number, baseSize: number, devicePixelRatio = readDevicePixelRatio()): number {
  if (!Number.isFinite(baseSize) || baseSize <= 0) return 1;
  return snapLogicalSizeToDevicePixels(size, devicePixelRatio) / baseSize;
}

/** Re-render when macOS moves the window between displays with different DPRs. */
export function useDevicePixelRatio(): number {
  const [devicePixelRatio, setDevicePixelRatio] = useState(readDevicePixelRatio);

  useEffect(() => {
    const update = () => setDevicePixelRatio(readDevicePixelRatio());
    window.addEventListener("resize", update);
    window.visualViewport?.addEventListener("resize", update);
    return () => {
      window.removeEventListener("resize", update);
      window.visualViewport?.removeEventListener("resize", update);
    };
  }, []);

  return devicePixelRatio;
}
