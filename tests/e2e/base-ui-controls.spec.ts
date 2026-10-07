import { expect, test, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

async function openPlayer(page: Page) {
  await page.goto("/films/100/watch");
  await page.getByRole("button", { name: "Sign in to watch" }).click();
  const login = page.getByRole("dialog", { name: "Sign in to sync addons" });
  await login.getByLabel("Email").fill("viewer@example.com");
  await login.getByLabel("Password").fill("password");
  await login.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(login).toBeHidden();
  const check = page.getByRole("button", { name: "Check service", exact: true }).first();
  if (await check.isVisible()) await check.click();
  await expect(page.getByRole("slider", { name: "Playback position" })).toBeVisible();
}

test("Base UI sign-in traps focus, keeps outside presses open and restores the opener", async ({ page }) => {
  await page.goto("/");
  const trigger = page.getByRole("button", { name: "Account menu" });
  await trigger.click();
  await page.getByRole("button", { name: "Log In" }).click();
  const login = page.getByRole("dialog", { name: "Sign in to sync addons" });
  await expect(login.getByLabel("Email")).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(login.getByRole("button", { name: "Close sign in" })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(login.getByRole("button", { name: "Sign in", exact: true })).toBeFocused();
  await page.mouse.click(10, 10);
  await expect(login).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(login).toBeHidden();
  await expect(trigger).toBeFocused();
});

test("Base UI player sliders preview drags, commit on release and retain keyboard control", async ({ page }) => {
  await openPlayer(page);
  await page.evaluate(() => document.fonts.ready);
  await page.locator(".player-video").click();
  const progress = page.getByRole("slider", { name: "Playback position" });
  const track = page.locator(".player-range--seek .player-range-control");
  const bounds = (await track.boundingBox())!;
  await page.mouse.move(bounds.x + 7, bounds.y + bounds.height / 2);
  await page.mouse.down();
  await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2, { steps: 5 });
  const previewBounds = (await track.boundingBox())!;
  await page.mouse.move(previewBounds.x + previewBounds.width / 2, previewBounds.y + previewBounds.height / 2);
  const preview = Number(await progress.inputValue());
  const liveBounds = (await track.boundingBox())!;
  const pixelTolerance = 6120 / (liveBounds.width - 14);
  expect(Math.abs(preview - 3060)).toBeLessThanOrEqual(pixelTolerance);
  await page.mouse.up();
  await expect.poll(async () => Math.abs(Number(await progress.inputValue()) - preview)).toBeLessThanOrEqual(pixelTolerance);
  const committed = Number(await progress.inputValue());
  await progress.focus();
  await page.keyboard.press("ArrowRight");
  expect(Number(await progress.inputValue())).toBeCloseTo(committed + 0.1, 1);
  const volume = page.getByRole("slider", { name: "Volume" });
  await volume.focus();
  await page.keyboard.press("End");
  await expect(volume).toHaveValue("200");
  await page.keyboard.press("Home");
  await expect(volume).toHaveValue("0");
  await page.getByRole("button", { name: "Close", exact: true }).click();
  await expect(page).toHaveURL(/\/films\/100$/);
});

test("Base UI subtitle panels preserve wide columns, narrow tabs, numeric edits and focus return", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openPlayer(page);
  const trigger = page.getByRole("button", { name: "Subtitles", exact: true });
  await trigger.click();
  const panel = page.getByRole("dialog", { name: "Subtitles", exact: true });
  await expect(panel.getByRole("radio", { name: "English", exact: true })).toBeVisible();
  await expect(panel.getByRole("radio", { name: "Embedded 1", exact: true })).toBeVisible();
  await expect(panel.getByRole("button", { name: "Edit font size", exact: true })).toBeVisible();
  await panel.getByRole("button", { name: "Edit font size", exact: true }).click();
  const size = panel.getByRole("textbox", { name: "Edit font size", exact: true });
  await size.fill("48");
  await size.press("Enter");
  await expect(panel.getByLabel("Subtitle appearance sample").locator("[aria-hidden=true]")).toHaveCSS("font-size", "48px");
  const sample = panel.getByLabel("Subtitle appearance sample").locator("[aria-hidden=true]");
  await panel.getByRole("radio", { name: "Yellow", exact: true }).click();
  await expect(panel.getByRole("radio", { name: "Yellow", exact: true })).toBeChecked();
  await expect(sample).toHaveCSS("color", "rgb(255, 224, 102)");
  await panel.getByRole("radio", { name: "Yellow", exact: true }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(panel.getByRole("radio", { name: "Green", exact: true })).toBeChecked();
  await panel.getByLabel("Custom text color").evaluate((input) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "#123456");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await expect(sample).toHaveCSS("color", "rgb(18, 52, 86)");
  await panel.getByRole("button", { name: "Edit text opacity", exact: true }).click();
  const opacity = panel.getByRole("textbox", { name: "Edit text opacity", exact: true });
  await opacity.fill("50");
  await opacity.press("Enter");
  expect(await sample.evaluate((node) => getComputedStyle(node).color)).toMatch(/(?:0\.5|50%)/);
  expect(await sample.evaluate((node) => getComputedStyle(node).backgroundColor)).toMatch(/(?:0\.68|68%)/);
  await expect(sample).toHaveCSS("opacity", "1");
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("panorama.subtitle-preferences.v1")!).style)).toMatchObject({ textColor: "#123456", textOpacity: 50 });
  await testInfo.attach("wide-subtitles", { body: await page.screenshot(), contentType: "image/png" });
  await page.setViewportSize({ width: 390, height: 844 });
  const language = panel.getByRole("tab", { name: "Language", exact: true });
  await expect(language).toBeVisible();
  await language.focus();
  await page.keyboard.press("End");
  await expect(panel.getByRole("tab", { name: "Appearance", exact: true })).toBeFocused();
  await expect(panel.getByRole("tab", { name: "Appearance", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(panel.getByRole("radio", { name: "English", exact: true })).toBeHidden();
  await expect(panel.getByRole("button", { name: "Edit font size", exact: true })).toBeVisible();
  await panel.getByRole("radio", { name: "bold", exact: true }).click();
  await expect(panel.getByRole("radio", { name: "bold", exact: true })).toBeChecked();
  const bounds = (await panel.boundingBox())!;
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(390);
  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations.filter((violation) => violation.impact === "serious" || violation.impact === "critical")).toEqual([]);
  await page.keyboard.press("Escape");
  await expect(panel).toBeHidden();
  await expect(trigger).toBeFocused();
  await expect(page).toHaveURL(/\/watch$/);
  await page.reload();
  await openPlayer(page);
  await page.getByRole("button", { name: "Subtitles", exact: true }).click();
  await page.getByRole("tab", { name: "Appearance", exact: true }).click();
  await expect(page.getByLabel("Custom text color")).toHaveValue("#123456");
  await expect(page.getByRole("button", { name: "Edit text opacity", exact: true })).toHaveText("50%");
});

test("Base UI settings select closes independently, stays inside fullscreen and saves preferences", async ({ page }) => {
  await page.addInitScript(() => {
    let settings = { deviceId: "auto", channels: "auto", passthrough: false, video: "auto", devices: [{ id: "auto", label: "System default", passthroughAvailable: false, passthroughEnabled: false }], effectivePassthrough: false, notice: null };
    Object.assign(window, { panoramaDesktop: {
      getCapabilities: async () => ({ platform: "browser", playback: "browser" }),
      mpv: { send: () => {}, on: () => () => {}, setVideoSurface: () => {} },
      getPlaybackSettings: async () => settings,
      setPlaybackSettings: async (patch: Partial<typeof settings>) => { settings = { ...settings, ...patch }; return settings; },
      getDiscordSettings: async () => ({ enabled: false, available: true }),
      setDiscordEnabled: async (enabled: boolean) => ({ enabled, available: true }),
    } });
  });
  await openPlayer(page);
  await page.getByRole("button", { name: "Fullscreen", exact: true }).click();
  await expect(page.getByRole("button", { name: "Exit fullscreen", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Playback settings", exact: true }).click();
  const panel = page.getByRole("dialog", { name: "Playback settings", exact: true });
  const channels = panel.getByRole("combobox", { name: "Decoded channels", exact: true });
  await channels.click();
  const stereo = page.getByRole("option", { name: "Stereo", exact: true });
  await expect(stereo).toBeVisible();
  expect(await stereo.evaluate((node) => Boolean(document.fullscreenElement?.contains(node)))).toBe(true);
  await page.evaluate(() => document.exitFullscreen());
  await expect(page.getByRole("button", { name: "Fullscreen", exact: true })).toBeVisible();
  await channels.focus();
  await page.keyboard.press("Escape");
  await expect(stereo).toBeHidden();
  await expect(panel).toBeVisible();
  await channels.click();
  await stereo.click();
  await expect(channels).toContainText("Stereo");
  await panel.getByRole("checkbox", { name: "Share watching activity on Discord" }).check();
  await expect(panel.getByRole("checkbox", { name: "Share watching activity on Discord" })).toBeChecked();
  await page.keyboard.press("Escape");
  await expect(panel).toBeHidden();
  await expect(page).toHaveURL(/\/watch$/);
});
