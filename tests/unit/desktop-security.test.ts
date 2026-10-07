import { describe, expect, it } from "vitest";
import { readFile } from "node:fs/promises";
import path from "node:path";
import {
  createMainWindowOptions,
  getApprovedExternalUrl,
  isAllowedDevelopmentOrigin,
  isAllowedRendererPermission,
  isOwnedRendererNavigation,
} from "../../desktop/main/create-main-window";

describe("desktop shell security", () => {
  it("creates a sandboxed renderer without Node integration", () => {
    const options = createMainWindowOptions("/app/preload.js", "darwin");

    expect(options.webPreferences).toEqual({
      preload: "/app/preload.js",
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: true,
      webSecurity: true,
    });
    expect(options.title).toBe("Panorama");
    expect(options.titleBarStyle).toBe("hiddenInset");
    expect(options.backgroundColor).toBe("#00000000");
    expect(options.transparent).toBe(true);
    expect(options.trafficLightPosition).toEqual({ x: 14, y: 13 });
  });

  it("uses a transparent frameless shell on Windows without weakening renderer security", () => {
    const options = createMainWindowOptions("C:\\app\\preload.js", "win32");
    expect(options.backgroundColor).toBe("#00000000");
    expect(options.transparent).toBe(true);
    expect(options.frame).toBe(false);
    expect(options.thickFrame).toBe(true);
    expect(options.autoHideMenuBar).toBe(true);
    expect(options.title).toBe("");
    expect(options.titleBarStyle).toBeUndefined();
    expect(options.titleBarOverlay).toBeUndefined();
    expect(options.trafficLightPosition).toBeUndefined();
    expect(options.webPreferences).toMatchObject({
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: true,
      webSecurity: true,
    });
  });

  it("removes the Windows menu and exposes only allowlisted custom window controls", async () => {
    const [main, mainWindow, preload, styles, titlebar] = await Promise.all([
      readFile(path.join(process.cwd(), "desktop/main/main.ts"), "utf8"),
      readFile(path.join(process.cwd(), "desktop/main/create-main-window.ts"), "utf8"),
      readFile(path.join(process.cwd(), "desktop/preload/preload.ts"), "utf8"),
      readFile(path.join(process.cwd(), "app/globals.css"), "utf8"),
      readFile(path.join(process.cwd(), "components/DesktopTitlebar.tsx"), "utf8"),
    ]);
    expect(main).toContain("Menu.setApplicationMenu(null)");
    expect(main).toContain('ipcMain.handle("panorama:window-control"');
    expect(mainWindow).toContain("window.removeMenu()");
    expect(mainWindow).toContain('window.on("page-title-updated"');
    expect(preload).toContain('classList.add("panorama-windows")');
    expect(preload).toContain('ipcRenderer.invoke("panorama:window-control"');
    expect(styles).toMatch(/panorama-windows \.desktop-titlebar[\s\S]*?background: #f4f4f4/);
    expect(styles).toMatch(/panorama-windows \.desktop-window-controls[\s\S]*?-webkit-app-region: no-drag/);
    expect(styles).toMatch(/panorama-windows \.app-viewport[\s\S]*?inset: 32px 0 0/);
    expect(titlebar).toContain('controlWindow("minimize")');
    expect(titlebar).toContain('controlWindow("toggle-maximize")');
    expect(titlebar).toContain('controlWindow("close")');
  });

  it("allows fullscreen only for Panorama's owned renderer", () => {
    const origin = "http://127.0.0.1:3100";

    expect(isAllowedRendererPermission("fullscreen", `${origin}/films/1893/watch`, origin)).toBe(true);
    expect(isAllowedRendererPermission("notifications", `${origin}/films/1893/watch`, origin)).toBe(false);
    expect(isAllowedRendererPermission("fullscreen", "https://example.com/", origin)).toBe(false);
  });

  it("allows navigation only within the exact owned origin", () => {
    expect(isOwnedRendererNavigation("http://127.0.0.1:3100/films/1893", "http://127.0.0.1:3100")).toBe(true);
    expect(isOwnedRendererNavigation("http://localhost:3100/", "http://127.0.0.1:3100")).toBe(false);
    expect(isOwnedRendererNavigation("https://example.com/", "http://127.0.0.1:3100")).toBe(false);
  });

  it("maps only the approved external action", () => {
    expect(getApprovedExternalUrl("stremio-service-download")).toBe("https://www.stremio.com/download-service");
    expect(() => getApprovedExternalUrl("https://example.com" as never)).toThrow("external target");
  });

  it("accepts only an exact IPv4 loopback development origin", () => {
    expect(isAllowedDevelopmentOrigin("http://127.0.0.1:3000/")).toBe(true);
    expect(isAllowedDevelopmentOrigin("http://localhost:3000/")).toBe(false);
    expect(isAllowedDevelopmentOrigin("https://127.0.0.1:3000/")).toBe(false);
    expect(isAllowedDevelopmentOrigin("http://127.0.0.1:3000/films/1")).toBe(false);
    expect(isAllowedDevelopmentOrigin("http://127.0.0.1:3000@evil.example/")).toBe(false);
  });
});
