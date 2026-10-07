import { access } from "node:fs/promises";
import { _electron as electron, expect, test } from "@playwright/test";
import { desktopExecutablePath, desktopLaunchOptions } from "./desktop-executable";

test("packaged native player preserves Panorama controls", async ({}, testInfo) => {
  test.skip(testInfo.project.name !== "chromium" || process.env.PANORAMA_DESKTOP_PARITY_E2E !== "1");
  const executablePath = desktopExecutablePath();
  await access(executablePath);
  const electronApp = await electron.launch(desktopLaunchOptions());
  try {
    const page = await electronApp.firstWindow();
    await page.getByRole("button", { name: /Open details for/ }).first().click();
    const play = page.getByRole("button", { name: /^(?:Play|Resume(?:,.*)?)$/ });
    test.skip(await play.count() === 0, "Packaged parity requires an existing signed-in test profile and online service.");
    await play.click();
    await expect(page).toHaveURL(/\/films\/\d+\/watch(?:\?resume=1)?$/);
    await expect(page.getByRole("button", { name: /^(Play|Pause)$/ })).toBeVisible();
    await expect(page.getByRole("button", { name: "Sources" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Audio" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Subtitles" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Fullscreen" })).toBeVisible();
    await page.keyboard.press("f");
    await expect(page.getByRole("button", { name: "Exit fullscreen" })).toBeVisible();
    await expect(page.locator("body")).toHaveClass(/panorama-desktop-fullscreen/);
    expect(await page.evaluate(() => document.fullscreenElement)).toBeNull();
    await page.getByRole("button", { name: "Close", exact: true }).click();
    await expect(page).toHaveURL(/\/films\/\d+$/);
    await expect(page.locator("body")).toHaveClass(/panorama-desktop-fullscreen/);
    await expect.poll(() => electronApp.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]?.isFullScreen())).toBe(false);
  } finally {
    await electronApp.close();
  }
});
