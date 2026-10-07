import { access } from "node:fs/promises";
import { _electron as electron, expect, test } from "@playwright/test";
import { desktopExecutablePath, desktopLaunchOptions } from "./desktop-executable";

test("persists opt-in Discord sharing across packaged restarts", async ({}, testInfo) => {
  test.skip(testInfo.project.name !== "chromium" || process.env.PANORAMA_DESKTOP_E2E !== "1" || !process.env.PANORAMA_DESKTOP_USER_DATA_DIR);
  test.setTimeout(120_000);
  for (const enabled of [false, true]) {
    const electronApp = await electron.launch(desktopLaunchOptions());
    try {
      const page = await electronApp.firstWindow();
      await expect.poll(() => page.evaluate(() => window.panoramaDesktop?.getDiscordSettings?.())).toEqual({ enabled, available: true });
      expect(await page.evaluate((next) => window.panoramaDesktop?.setDiscordEnabled?.(next), !enabled)).toEqual({ enabled: !enabled, available: true });
    } finally { await electronApp.close(); }
  }
});

test("runs the renderer server without launching a second application", async ({}, testInfo) => {
  test.skip(process.platform !== "darwin");
  test.skip(testInfo.project.name !== "chromium" || process.env.PANORAMA_DESKTOP_E2E !== "1");
  const executablePath = desktopExecutablePath();
  await access(executablePath);
  const electronApp = await electron.launch(desktopLaunchOptions());

  try {
    await electronApp.firstWindow();
    await expect.poll(() => electronApp.evaluate(({ app }) => (
      app.getAppMetrics().some((process) => process.name === "Panorama Renderer Server")
    ))).toBe(true);
  } finally {
    await electronApp.close();
  }
});

test("uses native macOS controls and removes the titlebar surface in fullscreen", async ({}, testInfo) => {
  test.skip(process.platform !== "darwin");
  test.skip(testInfo.project.name !== "chromium" || process.env.PANORAMA_DESKTOP_E2E !== "1");
  const executablePath = desktopExecutablePath();
  await access(executablePath);
  const electronApp = await electron.launch(desktopLaunchOptions());

  try {
    const page = await electronApp.firstWindow();
    const windowButtonPosition = await electronApp.evaluate(({ BrowserWindow }) => {
      const window = BrowserWindow.getAllWindows()[0];
      if (!window) return null;
      return window.getWindowButtonPosition();
    });
    expect(windowButtonPosition).toEqual({ x: 14, y: 13 });
    await expect(page.locator(".desktop-titlebar")).toBeVisible();
    await expect(page.locator(".desktop-titlebar")).toHaveText("Panorama");

    const fullscreenChrome = await electronApp.evaluate(async ({ BrowserWindow }) => {
      const window = BrowserWindow.getAllWindows()[0];
      if (!window) return null;
      const entered = new Promise<void>((resolve) => window.once("enter-full-screen", resolve));
      window.setFullScreen(true);
      await entered;
      return window.isFullScreen();
    });
    expect(fullscreenChrome).toBe(true);
    await expect(page.locator(".desktop-titlebar")).toBeHidden();
  } finally {
    await electronApp.close();
  }
});

test("packaged shell preserves the browser interface and quits cleanly", async ({}, testInfo) => {
  test.skip(testInfo.project.name !== "chromium" || process.env.PANORAMA_DESKTOP_E2E !== "1");
  const executablePath = desktopExecutablePath();
  await access(executablePath);
  const electronApp = await electron.launch(desktopLaunchOptions());

  try {
    const page = await electronApp.firstWindow();
    const nativeChrome = await electronApp.evaluate(({ BrowserWindow }) => {
      const window = BrowserWindow.getAllWindows()[0];
      if (!window) return null;
      return {
        buttons: process.platform === "darwin" ? window.getWindowButtonPosition() : null,
        title: window.getTitle(),
        titlebarHeight: window.getBounds().height - window.getContentBounds().height,
      };
    });
    if (process.platform === "win32") expect(nativeChrome?.title).toBe("");
    else expect(nativeChrome?.title).toBe("Panorama");
    if (process.platform === "win32") expect(nativeChrome?.titlebarHeight).toBe(0);
    else expect(nativeChrome?.buttons).toEqual({ x: 14, y: 13 });
    await expect(page.locator(".desktop-titlebar")).toBeVisible();

    await expect(page.getByRole("button", { name: "Search" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Account menu" })).toBeVisible();
    await expect.poll(() => page.evaluate(() => document.fonts.status)).toBe("loaded");

    await page.getByRole("button", { name: "Account menu" }).click();
    await page.getByRole("button", { name: "Log In" }).click();
    await expect(page.getByRole("dialog", { name: "Sign in to sync addons" })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog", { name: "Sign in to sync addons" })).not.toBeVisible();

    await page.getByRole("button", { name: "Search" }).click();
    await expect(page.getByRole("searchbox", { name: "Search movies" })).toBeFocused();
    await page.getByRole("button", { name: "Close search" }).click();

    await page.getByRole("button", { name: /Open details for/ }).first().click();
    await expect(page).toHaveURL(/\/films\/\d+$/);
    await page.goBack();
    await expect(page).toHaveURL(/\/$/);

    await page.evaluate(() => {
      const target = document.createElement("button");
      target.textContent = "Test fullscreen";
      target.id = "fullscreen-test";
      target.style.cssText = "position:fixed;inset:100px auto auto 100px;z-index:99999";
      target.onclick = () => { void target.requestFullscreen(); };
      document.body.append(target);
    });
    await page.getByRole("button", { name: "Test fullscreen", exact: true }).click();
    await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id)).toBe("fullscreen-test");
    await expect.poll(() => electronApp.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].isFullScreen())).toBe(true);
    await expect(page.locator("body")).toHaveClass(/panorama-desktop-fullscreen/);
    await page.evaluate(() => document.exitFullscreen());
    await expect.poll(() => page.evaluate(() => document.fullscreenElement === null)).toBe(true);
    await page.evaluate(() => document.getElementById("fullscreen-test")?.remove());
  } finally {
    await electronApp.close();
  }
});

test("keeps search open through residual desktop scroll", async ({}, testInfo) => {
  test.skip(testInfo.project.name !== "chromium" || process.env.PANORAMA_DESKTOP_E2E !== "1");
  const executablePath = desktopExecutablePath();
  await access(executablePath);
  const electronApp = await electron.launch(desktopLaunchOptions());

  try {
    const page = await electronApp.firstWindow();
    const header = page.locator("header");
    await expect(page.getByTestId("film-grid")).toBeVisible();
    await page.evaluate(() => {
      const viewport = document.querySelector<HTMLElement>(".app-viewport");
      const grid = document.querySelector<HTMLElement>("[data-testid='film-grid']");
      if (viewport && grid) viewport.scrollTop = grid.offsetTop;
    });
    await expect(header).toHaveAttribute("inert", "");
    await expect.poll(async () => {
      const bounds = await header.boundingBox();
      return bounds ? bounds.y + bounds.height : Number.POSITIVE_INFINITY;
    }).toBeLessThanOrEqual(0);
    await page.evaluate(() => document.querySelector<HTMLElement>(".app-viewport")?.scrollBy(0, -1));
    await expect(header).not.toHaveAttribute("inert", "");

    await page.getByRole("button", { name: "Search" }).click();
    await expect(page.getByRole("searchbox", { name: "Search movies" })).toBeVisible();
    await page.evaluate(() => document.querySelector<HTMLElement>(".app-viewport")?.scrollBy(0, 20));
    await page.waitForTimeout(400);

    await expect(header).not.toHaveAttribute("inert", "");
  } finally {
    await electronApp.close();
  }
});

test("keeps film detail information inside the packaged desktop viewport", async ({}, testInfo) => {
  test.skip(testInfo.project.name !== "chromium" || process.env.PANORAMA_DESKTOP_E2E !== "1");
  const executablePath = desktopExecutablePath();
  await access(executablePath);
  const electronApp = await electron.launch(desktopLaunchOptions());

  try {
    const page = await electronApp.firstWindow();
    await page.getByRole("button", { name: /Open details for/ }).first().click();
    await expect(page).toHaveURL(/\/films\/\d+$/);
    await expect(page.getByRole("heading", { name: "Synopsis" })).toBeVisible();
    const bounds = await page.evaluate(() => {
      const viewport = document.querySelector<HTMLElement>(".app-viewport");
      const content = document.querySelector<HTMLElement>("main > .desktop-hero-viewport.content-container");
      if (!viewport || !content) return null;
      return {
        viewportBottom: viewport.getBoundingClientRect().bottom,
        contentBottom: content.getBoundingClientRect().bottom,
      };
    });
    expect(bounds).not.toBeNull();
    expect(bounds!.contentBottom).toBeLessThanOrEqual(bounds!.viewportBottom);
  } finally {
    await electronApp.close();
  }
});
