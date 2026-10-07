import { afterEach, describe, expect, it, vi } from "vitest";
import { StremioCoreRuntime } from "@/runtime/stremio-core-runtime";

const originalFullscreenElement = Object.getOwnPropertyDescriptor(document, "fullscreenElement");
const originalExitFullscreen = document.exitFullscreen;

afterEach(() => {
  if (originalFullscreenElement) {
    Object.defineProperty(document, "fullscreenElement", originalFullscreenElement);
  } else {
    Reflect.deleteProperty(document, "fullscreenElement");
  }
  document.exitFullscreen = originalExitFullscreen;
});

describe("player fullscreen", () => {
  it("fullscreens the supplied player surface and follows browser exit", async () => {
    const runtime = new StremioCoreRuntime();
    const playerSurface = document.createElement("div");
    let fullscreenElement: Element | null = null;

    Object.defineProperty(document, "fullscreenElement", {
      configurable: true,
      get: () => fullscreenElement,
    });
    playerSurface.requestFullscreen = vi.fn(async () => {
      fullscreenElement = playerSurface;
      document.dispatchEvent(new Event("fullscreenchange"));
    });
    document.exitFullscreen = vi.fn(async () => {
      fullscreenElement = null;
      document.dispatchEvent(new Event("fullscreenchange"));
    });

    runtime.setPlaybackFullscreen(true, playerSurface);
    await vi.waitFor(() => expect(runtime.getSnapshot().player.fullscreen).toBe(true));
    expect(playerSurface.requestFullscreen).toHaveBeenCalledOnce();

    fullscreenElement = null;
    document.dispatchEvent(new Event("fullscreenchange"));
    expect(runtime.getSnapshot().player.fullscreen).toBe(false);
  });
});
