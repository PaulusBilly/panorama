import { readFile } from "node:fs/promises";
import path from "node:path";
import { describe, expect, it } from "vitest";

const root = process.cwd();

describe("Windows native playback spike contract", () => {
  it("keeps native ownership in the main process and exposes only bounded surface IPC", async () => {
    const [main, preload, renderer] = await Promise.all([
      readFile(path.join(root, "desktop/spike/main.ts"), "utf8"),
      readFile(path.join(root, "desktop/spike/preload.ts"), "utf8"),
      readFile(path.join(root, "desktop/spike/renderer.ts"), "utf8"),
    ]);

    expect(main).toContain('process.platform === "win32"');
    expect(main).toContain("getNativeWindowHandle");
    expect(main).toContain("panorama:spike:set-video-bounds");
    expect(main).not.toMatch(/child_process|exec\(|spawn\(|shell\.openPath/);
    expect(preload).toContain("panorama:spike:set-video-bounds");
    expect(preload).not.toMatch(/nativeWindowHandle|mediaUrl|command\(/);
    expect(renderer).toContain("ResizeObserver");
  });
});
