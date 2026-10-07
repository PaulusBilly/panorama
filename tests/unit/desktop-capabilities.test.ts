import { afterEach, describe, expect, it } from "vitest";
import { getDesktopCapabilities } from "../../runtime/desktop-capabilities";

afterEach(() => {
  delete window.panoramaDesktop;
});

describe("desktop capabilities", () => {
  it("uses browser fallback when the preload API is absent", async () => {
    await expect(getDesktopCapabilities()).resolves.toBeNull();
  });

  it("returns validated desktop capabilities", async () => {
    window.panoramaDesktop = {
      getCapabilities: async () => ({
        platform: "darwin",
        architecture: "arm64",
        nativePlayback: { status: "ready", device: "ShellVideo", mpvVersion: "0.41.0" },
        appVersion: "0.1.0",
      }),
      openExternal: async () => undefined,
    };

    await expect(getDesktopCapabilities()).resolves.toEqual({
      platform: "darwin",
      architecture: "arm64",
      nativePlayback: { status: "ready", device: "ShellVideo", mpvVersion: "0.41.0" },
      appVersion: "0.1.0",
    });
  });

  it("rejects malformed preload responses", async () => {
    window.panoramaDesktop = {
      getCapabilities: async () => ({ platform: "linux" } as never),
      openExternal: async () => undefined,
    };

    await expect(getDesktopCapabilities()).rejects.toThrow("desktop capabilities");
  });
});
