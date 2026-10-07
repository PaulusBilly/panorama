import { readFile } from "node:fs/promises";
import path from "node:path";
import { describe, expect, it } from "vitest";

const root = process.cwd();

describe("Windows native MPV binding contract", () => {
  it("selects a Win32-only source and dynamically loads the pinned runtime", async () => {
    const [gyp, source, wrapper] = await Promise.all([
      readFile(path.join(root, "desktop/native/mpv-host/binding.gyp"), "utf8"),
      readFile(path.join(root, "desktop/native/mpv-host/src/addon_win.cc"), "utf8"),
      readFile(path.join(root, "desktop/native/mpv-host/native-binding.ts"), "utf8"),
    ]);

    expect(gyp).toContain('OS==\\"win\\"');
    expect(gyp).toContain("addon_win.cc");
    expect(gyp).toContain("win_delay_load_hook");
    expect(source).toContain("IsWindow(parent)");
    expect(source).toContain("WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN");
    expect(source).toContain("EnableWindow(video_, FALSE)");
    expect(source).toContain("LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS");
    expect(source).toContain('SetOption("wid"');
    expect(source).toContain('SetOption("gpu-api", "d3d11")');
    expect(source).toContain('SetOption("hwdec", "d3d11va,auto-safe")');
    expect(source).toContain('"absolute+exact"');
    expect(source).not.toContain('"absolute+keyframes"');
    expect(wrapper).toContain("class WindowsNativeMpvBinding");
  });
});
