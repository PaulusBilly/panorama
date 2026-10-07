import path from "node:path";
import { describe, expect, it } from "vitest";
import { resolveNativeResourcePaths } from "../../desktop/main/platform-paths";

describe("Windows native resource paths", () => {
  it("resolves packaged resources beneath the application resources directory", () => {
    const paths = resolveNativeResourcePaths({
      packaged: true,
      platform: "win32",
      architecture: "x64",
      resourcesPath: "C:\\Program Files\\Panorama Ω\\resources",
      projectRoot: "D:\\src\\panorama",
    });

    expect(paths).toEqual({
      addon: path.win32.join("C:\\Program Files\\Panorama Ω\\resources", "desktop-resources", "native", "win32-x64", "mpv_host.node"),
      runtimeDirectory: path.win32.join("C:\\Program Files\\Panorama Ω\\resources", "desktop-resources", "native", "win32-x64"),
      subtitleFontDirectory: path.win32.join("C:\\Program Files\\Panorama Ω\\resources", "desktop-resources", "standalone", "public", "fonts"),
    });
  });

  it("resolves development resources without a machine-specific fallback", () => {
    expect(resolveNativeResourcePaths({
      packaged: false,
      platform: "win32",
      architecture: "x64",
      resourcesPath: "C:\\unused",
      projectRoot: "D:\\src\\panorama",
    })).toEqual({
      addon: path.win32.join("D:\\src\\panorama", "desktop", "native", "mpv-host", "build", "Release", "mpv_host.node"),
      runtimeDirectory: path.win32.join("D:\\src\\panorama", ".cache", "panorama", "windows-libmpv", "current"),
      subtitleFontDirectory: path.win32.join("D:\\src\\panorama", "public", "fonts"),
    });
  });

  it.each([
    { platform: "win32" as const, architecture: "arm64" },
    { platform: "linux" as const, architecture: "x64" },
    { platform: "darwin" as const, architecture: "x64" },
  ])("rejects unsupported target $platform/$architecture", ({ platform, architecture }) => {
    expect(() => resolveNativeResourcePaths({
      packaged: false,
      platform,
      architecture,
      resourcesPath: "C:\\unused",
      projectRoot: "D:\\src\\panorama",
    })).toThrow("Unsupported native playback target");
  });
});
