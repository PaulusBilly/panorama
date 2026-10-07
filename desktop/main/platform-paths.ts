import path from "node:path";

export type NativeResourcePaths = {
  addon: string;
  runtimeDirectory: string;
  subtitleFontDirectory: string;
};

type ResolveNativeResourcePathsInput = {
  packaged: boolean;
  platform: NodeJS.Platform;
  architecture: string;
  resourcesPath: string;
  projectRoot: string;
};

export function resolveNativeResourcePaths(input: ResolveNativeResourcePathsInput): NativeResourcePaths {
  const pathApi = input.platform === "win32" ? path.win32 : path.posix;
  const supported = (input.platform === "win32" && input.architecture === "x64")
    || (input.platform === "darwin" && input.architecture === "arm64");
  if (!supported) throw new Error("Unsupported native playback target");

  if (input.packaged) {
    const runtimeDirectory = pathApi.join(
      input.resourcesPath,
      "desktop-resources",
      "native",
      `${input.platform}-${input.architecture}`,
    );
    return {
      addon: pathApi.join(runtimeDirectory, "mpv_host.node"),
      runtimeDirectory,
      subtitleFontDirectory: pathApi.join(
        input.resourcesPath,
        "desktop-resources",
        "standalone",
        "public",
        "fonts",
      ),
    };
  }

  return {
    addon: pathApi.join(input.projectRoot, "desktop", "native", "mpv-host", "build", "Release", "mpv_host.node"),
    runtimeDirectory: input.platform === "win32"
      ? pathApi.join(input.projectRoot, ".cache", "panorama", "windows-libmpv", "current")
      : pathApi.join(input.projectRoot, ".cache", "panorama", "macos-libmpv", "current", "lib"),
    subtitleFontDirectory: pathApi.join(input.projectRoot, "public", "fonts"),
  };
}
