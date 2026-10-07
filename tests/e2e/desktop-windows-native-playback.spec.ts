import { _electron as electron, expect, test } from "@playwright/test";
import { desktopLaunchOptions } from "./desktop-executable";

test("packaged Windows native surface survives window lifecycle changes", async ({}, testInfo) => {
  test.skip(process.platform !== "win32" || testInfo.project.name !== "chromium" || process.env.PANORAMA_DESKTOP_E2E !== "1");
  const electronApp = await electron.launch(desktopLaunchOptions());
  try {
    const page = await electronApp.firstWindow();
    await page.evaluate(() => window.panoramaDesktop?.mpv?.setVideoSurface({
      visible: true, x: 20, y: 40, width: 640, height: 360, scaleFactor: window.devicePixelRatio,
    }));
    await electronApp.evaluate(async ({ BrowserWindow }) => {
      const window = BrowserWindow.getAllWindows()[0];
      if (!window) throw new Error("Window missing");
      window.minimize();
      await new Promise((resolve) => setTimeout(resolve, 100));
      window.restore();
      window.setFullScreen(true);
      await new Promise((resolve) => setTimeout(resolve, 100));
      window.setFullScreen(false);
    });
    await expect(page.getByRole("button", { name: "Search" })).toBeVisible();
    const capability = await page.evaluate(() => window.panoramaDesktop?.getCapabilities());
    expect(capability?.nativePlayback.status).toBe("ready");
  } finally {
    await electronApp.close();
  }
});
