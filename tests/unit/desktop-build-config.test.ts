import { access, readFile } from "node:fs/promises";
import path from "node:path";
import { describe, expect, it } from "vitest";

const projectRoot = process.cwd();

describe("desktop build configuration", () => {
  it("isolates the standalone renderer build", async () => {
    const config = await readFile(path.join(projectRoot, "next.config.ts"), "utf8");
    const script = await readFile(path.join(projectRoot, "desktop/scripts/build-renderer.mjs"), "utf8");

    expect(config).toContain('process.env.PANORAMA_DESKTOP_BUILD === "1"');
    expect(config).toContain('output: isDesktopBuild ? "standalone" : undefined');
    expect(script).toContain('PANORAMA_DIST_DIR: ".next-desktop"');
    expect(script).toContain('PANORAMA_DESKTOP_BUILD: "1"');
  });

  it("starts Playwright's renderer through a cross-platform environment wrapper", async () => {
    const [config, script] = await Promise.all([
      readFile(path.join(projectRoot, "playwright.config.ts"), "utf8"),
      readFile(path.join(projectRoot, "desktop/scripts/start-playwright-server.mjs"), "utf8"),
    ]);

    expect(config).toContain('command: "node desktop/scripts/start-playwright-server.mjs"');
    expect(script).toContain('PANORAMA_DIST_DIR: ".next-playwright"');
    expect(script).toContain('NEXT_PUBLIC_PANORAMA_RUNTIME: "fake"');
    expect(script).not.toMatch(/shell:\s*true/);
  });

  it("packages standalone static and public assets", async () => {
    const [script, manifest] = await Promise.all([
      readFile(path.join(projectRoot, "desktop/scripts/copy-standalone.mjs"), "utf8"),
      readFile(path.join(projectRoot, "desktop/native/mpv-host/windows-libmpv.json"), "utf8"),
    ]);

    expect(script).toContain('"standalone"');
    expect(script).toContain('"public"');
    expect(script).toContain('"static"');
    expect(script).toContain('".next-desktop"');
    expect(script).toContain('"server.js"');
    expect(script).toContain('`${process.platform}-${process.arch}`');
    expect(manifest).toContain('"libmpv-2.dll"');
    expect(script).toContain('"licenses"');
  });

  it("builds native code against the Electron ABI", async () => {
    const script = await readFile(path.join(projectRoot, "desktop/scripts/build-native.mjs"), "utf8");
    expect(script).toContain('require("electron/package.json").version');
    expect(script).toContain('`--target=${electronVersion}`');
    expect(script).toContain('"--dist-url=https://electronjs.org/headers"');
    expect(script).toContain("stage-windows-libmpv.mjs");
  });

  it("packages the Panorama favicon as the desktop application icon", async () => {
    const config = await readFile(path.join(projectRoot, "desktop/forge.config.ts"), "utf8");

    expect(config).toContain('icon: "public/favicon/favicon"');
    await expect(access(path.join(projectRoot, "public/favicon/favicon.icns"))).resolves.toBeUndefined();
    await expect(access(path.join(projectRoot, "public/favicon/favicon.ico"))).resolves.toBeUndefined();
  });
});
