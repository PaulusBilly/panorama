import type { VideoSurfaceBounds } from "./mpv-protocol";

type ViewportRect = Pick<DOMRect, "left" | "top" | "width" | "height">;

export function nativeVideoSurfaceBounds(
  rect: ViewportRect,
  scaleFactor: number,
  visible: boolean,
  viewport: { width: number; height: number } = {
    width: typeof window === "undefined" ? Number.POSITIVE_INFINITY : window.innerWidth,
    height: typeof window === "undefined" ? Number.POSITIVE_INFINITY : window.innerHeight,
  },
): VideoSurfaceBounds {
  const left = Math.max(0, rect.left);
  const top = Math.max(0, rect.top);
  const right = Math.min(viewport.width, rect.left + Math.max(0, rect.width));
  const bottom = Math.min(viewport.height, rect.top + Math.max(0, rect.height));
  const width = Math.max(0, right - left);
  const height = Math.max(0, bottom - top);
  const hasArea = width > 0 && height > 0;
  return {
    visible: visible && hasArea,
    x: hasArea ? left : 0,
    y: hasArea ? top : 0,
    width: hasArea ? width : 0,
    height: hasArea ? height : 0,
    scaleFactor: Number.isFinite(scaleFactor) && scaleFactor > 0 ? scaleFactor : 1,
  };
}
