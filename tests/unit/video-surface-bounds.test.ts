import { describe, expect, it } from "vitest";
import { nativeVideoSurfaceBounds } from "../../desktop/shared/video-surface";

describe("native video surface bounds", () => {
  it("normalizes a visible viewport rectangle", () => {
    expect(nativeVideoSurfaceBounds(
      { left: 12, top: 24, width: 1280, height: 720 },
      2,
      true,
      { width: 1920, height: 1080 },
    )).toEqual({
      visible: true,
      x: 12,
      y: 24,
      width: 1280,
      height: 720,
      scaleFactor: 2,
    });
  });

  it("hides zero-sized and explicitly hidden surfaces", () => {
    expect(nativeVideoSurfaceBounds({ left: 12, top: 24, width: 0, height: 720 }, 2, true).visible).toBe(false);
    expect(nativeVideoSurfaceBounds({ left: 12, top: 24, width: 1280, height: 720 }, 2, false).visible).toBe(false);
  });

  it.each([1, 1.25, 1.5])("keeps CSS coordinates unscaled at %sx DPI", (scaleFactor) => {
    expect(nativeVideoSurfaceBounds(
      { left: 20, top: 38, width: 640, height: 360 },
      scaleFactor,
      true,
      { width: 1280, height: 720 },
    )).toEqual({ visible: true, x: 20, y: 38, width: 640, height: 360, scaleFactor });
  });

  it("clamps partially clipped rectangles to the renderer viewport", () => {
    expect(nativeVideoSurfaceBounds(
      { left: -40, top: 38, width: 1400, height: 800 },
      1.25,
      true,
      { width: 1280, height: 720 },
    )).toEqual({ visible: true, x: 0, y: 38, width: 1280, height: 682, scaleFactor: 1.25 });
  });
});
