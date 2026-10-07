import { describe, expect, it, vi } from "vitest";
import { createNativeBinding } from "../../desktop/main/native-binding-factory";
import type { NativeAddon, NativeAddonHost } from "../../desktop/native/mpv-host/native-binding";

function addonRecorder() {
  const calls: unknown[][] = [];
  const host: NativeAddonHost = {
    load: vi.fn(), setPaused: vi.fn(), seek: vi.fn(), setBounds: vi.fn(), command: vi.fn(),
    setProperty: vi.fn(), stop: vi.fn(), destroy: vi.fn(),
    getDiagnostics: vi.fn(() => ({
      mpvVersion: "0.41.0", videoCodec: null, hardwareDecoder: null, buffering: false,
      cacheSeconds: null, timeSeconds: null, durationSeconds: null, tracks: [],
    })),
  };
  const addon: NativeAddon = {
    NativeMpvHost: class {
      constructor(...args: unknown[]) { calls.push(args); return host; }
    } as NativeAddon["NativeMpvHost"],
  };
  return { addon, calls, host };
}

describe("native binding platform selection", () => {
  it.each([
    { platform: "darwin" as const, architecture: "arm64" },
    { platform: "win32" as const, architecture: "x64" },
  ])("forwards bounds and cleanup on $platform", ({ platform, architecture }) => {
    const { addon, host } = addonRecorder();
    const binding = createNativeBinding({
      platform, architecture, nativeWindowHandle: Buffer.alloc(8), runtimeDirectory: "/runtime", addon,
    });
    binding!.dispatch({ type: "setBounds", x: 10, y: 32, width: 960, height: 540, scaleFactor: 1.5 });
    expect(host.setBounds).toHaveBeenCalledWith(10, 32, 960, 540, 1.5);
    binding!.destroy();
    expect(host.destroy).toHaveBeenCalledOnce();
  });

  it("creates Windows x64 with the packaged runtime directory", () => {
    const { addon, calls } = addonRecorder();
    expect(createNativeBinding({
      platform: "win32", architecture: "x64", nativeWindowHandle: Buffer.alloc(8),
      runtimeDirectory: "C:\\Program Files\\Panorama\\resources\\native", addon,
    })).not.toBeNull();
    expect(calls).toEqual([[Buffer.alloc(8), "C:\\Program Files\\Panorama\\resources\\native"]]);
  });

  it("preserves the macOS constructor and rejects unsupported targets", () => {
    const { addon, calls } = addonRecorder();
    expect(createNativeBinding({
      platform: "darwin", architecture: "arm64", nativeWindowHandle: Buffer.alloc(8),
      runtimeDirectory: "/unused", addon,
    })).not.toBeNull();
    expect(calls).toEqual([[Buffer.alloc(8)]]);
    expect(createNativeBinding({
      platform: "win32", architecture: "arm64", nativeWindowHandle: Buffer.alloc(8),
      runtimeDirectory: "C:\\unused", addon,
    })).toBeNull();
  });
});
