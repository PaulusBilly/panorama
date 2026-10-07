import { access } from "node:fs/promises";
import path from "node:path";
import { _electron as electron, expect, test } from "@playwright/test";
import { desktopExecutablePath, desktopLaunchOptions } from "./desktop-executable";

test("packaged Windows shell uses owned resources and exits cleanly", async ({}, testInfo) => {
  test.skip(process.platform !== "win32" || testInfo.project.name !== "chromium" || process.env.PANORAMA_DESKTOP_E2E !== "1");
  const executablePath = desktopExecutablePath();
  await Promise.all([
    access(executablePath),
    access(path.join(path.dirname(executablePath), "resources/desktop-resources/native/win32-x64/mpv_host.node")),
    access(path.join(path.dirname(executablePath), "resources/desktop-resources/native/win32-x64/libmpv-2.dll")),
  ]);
  const electronApp = await electron.launch(desktopLaunchOptions());
  try {
    const page = await electronApp.firstWindow();
    const capabilities = await page.evaluate(() => window.panoramaDesktop?.getCapabilities());
    expect(capabilities).toMatchObject({
      platform: "win32",
      architecture: "x64",
      nativePlayback: { status: "ready", device: "ShellVideo" },
    });
    const shell = await electronApp.evaluate(({ BrowserWindow, Menu }) => {
      const window = BrowserWindow.getAllWindows()[0];
      return window ? {
        titlebarHeight: window.getBounds().height - window.getContentBounds().height,
        menuVisible: window.isMenuBarVisible(),
        applicationMenu: Menu.getApplicationMenu(),
      } : null;
    });
    expect(shell?.titlebarHeight).toBe(0);
    expect(shell?.menuVisible).toBe(false);
    expect(shell?.applicationMenu).toBeNull();
    await expect(page.getByRole("button", { name: "Search" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Minimize window" })).toBeVisible();
    const maximize = page.getByRole("button", { name: "Maximize window" });
    await expect(maximize).toBeVisible();
    await expect(page.getByRole("button", { name: "Close window" })).toBeVisible();
    const windowedLayout = await page.evaluate(() => ({
      titlebarHeight: document.querySelector(".desktop-titlebar")?.getBoundingClientRect().height,
      contentTop: document.querySelector(".app-viewport")?.getBoundingClientRect().top,
      headerTop: document.querySelector(".panorama-site-header")?.getBoundingClientRect().top,
    }));
    expect(windowedLayout).toEqual({ titlebarHeight: 32, contentTop: 32, headerTop: 32 });
    await page.evaluate(() => window.panoramaDesktop?.setFullscreen?.(true));
    await expect(page.locator(".desktop-titlebar")).toBeHidden();
    await expect.poll(() => page.locator(".app-viewport").evaluate((element) => element.getBoundingClientRect().top)).toBe(0);
    await page.evaluate(() => window.panoramaDesktop?.setFullscreen?.(false));
    await expect(page.locator(".desktop-titlebar")).toBeVisible();
    await maximize.click();
    await expect.poll(() => electronApp.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]?.isMaximized())).toBe(true);
    await maximize.click();
    await expect.poll(() => electronApp.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]?.isMaximized())).toBe(false);
  } finally {
    await electronApp.close();
  }
});
