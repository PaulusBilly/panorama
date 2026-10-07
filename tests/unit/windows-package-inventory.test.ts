import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  requiredWindowsPackageFiles,
  verifyWindowsPackageInventory,
} from "../../desktop/scripts/verify-windows-package.mjs";

const temporaryRoots: string[] = [];

afterEach(async () => {
  await Promise.all(temporaryRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

describe("Windows package inventory", () => {
  it("requires the renderer, fonts, native host, runtime, notices, and streaming server", async () => {
    const required = await requiredWindowsPackageFiles();
    expect(required).toEqual(expect.arrayContaining([
      "panorama.exe",
      "resources/desktop-resources/standalone/server.js",
      "resources/desktop-resources/standalone/.next-desktop/static",
      "resources/desktop-resources/standalone/public/fonts/DMSans-Regular.woff2",
      "resources/desktop-resources/native/win32-x64/mpv_host.node",
      "resources/desktop-resources/native/win32-x64/libmpv-2.dll",
      "resources/desktop-resources/native/win32-x64/licenses/README.md",
      "resources/desktop-resources/stremio-server/server.js",
      "resources/desktop-resources/stremio-server/launch.cjs",
      "resources/desktop-resources/stremio-server/NOTICE.md",
    ]));
  });

  it("reports every missing package entry and accepts a complete inventory", async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), "panorama-package-"));
    temporaryRoots.push(root);
    await expect(verifyWindowsPackageInventory(root)).rejects.toThrow("panorama.exe");

    for (const relative of await requiredWindowsPackageFiles()) {
      const target = path.join(root, ...relative.split("/"));
      if (path.extname(target)) {
        await mkdir(path.dirname(target), { recursive: true });
        await writeFile(target, "test");
      } else {
        await mkdir(target, { recursive: true });
      }
    }
    await expect(verifyWindowsPackageInventory(root)).resolves.toBeUndefined();
  });
});
