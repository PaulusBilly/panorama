import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import ts from "typescript";

test("shared subtitles preserve glyphs, independent geometry, fallback and teardown", async ({ page }, testInfo) => {
  await page.route("**/__subtitle-fixture/*.js", async (route) => {
    const name = new URL(route.request().url()).pathname.split("/").at(-1)!.replace(/\.js$/, "");
    const source = readFileSync(`runtime/${name}.ts`, "utf8");
    const code = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } }).outputText.replace(/from "\.\/subtitle-appearance"/g, 'from "./subtitle-appearance.js"');
    await route.fulfill({ contentType: "text/javascript", body: code });
  });
  await page.goto("/");
  await page.evaluate(async () => {
    const load = (name: string) => import(`/__subtitle-fixture/${name}.js`);
    const [{ createSubtitleRenderer }, { createSubtitleCueStore }, { normalizeSubtitleAppearance }] = await Promise.all([load("subtitle-renderer"), load("subtitle-cue-store"), load("subtitle-appearance")]);
    const container = document.createElement("div");
    container.id = "subtitle-fixture";
    Object.assign(container.style, { position: "fixed", inset: "0", background: "#000", zIndex: "9999" });
    document.body.append(container);
    const store = createSubtitleCueStore();
    const renderer = createSubtitleRenderer(container, store, { style: normalizeSubtitleAppearance({}), verticalPosition: 15 });
    const text = "♪ Singing ♪\n♫ First line — Café é ♬ ♩\nمرحبا بالعالم — Selamat malam ♫";
    const cue = { playbackGeneration: 1, selectionGeneration: 1, seekGeneration: 0, sequence: 1, trackId: "1", kind: "text", text, startSeconds: 0, endSeconds: 3 };
    store.accept(cue);
    Object.assign(window, { subtitleFixture: { store, renderer, normalizeSubtitleAppearance, cue } });
    await document.fonts.ready;
  });
  const box = page.locator("#subtitle-fixture .panorama-subtitle-box");
  await expect(box).toHaveText("♪ Singing ♪\n♫ First line — Café é ♬ ♩\nمرحبا بالعالم — Selamat malam ♫");
  await expect(box).toHaveCSS("font-size", "38px");
  await expect(box).toHaveCSS("overflow", "visible");
  await page.evaluate(() => {
    const fixture = (window as unknown as { subtitleFixture: { renderer: { updateAppearance(style: unknown, position: number): void }; normalizeSubtitleAppearance(value: unknown): unknown } }).subtitleFixture;
    fixture.renderer.updateAppearance(fixture.normalizeSubtitleAppearance({ textColor: "#ffe066", textOpacity: 50, backgroundOpacity: 68 }), 15);
  });
  expect(await box.evaluate((node) => getComputedStyle(node).color)).toMatch(/(?:0\.5|50%)/);
  expect(await box.evaluate((node) => getComputedStyle(node).backgroundColor)).toMatch(/(?:0\.68|68%)/);
  await expect(box).toHaveCSS("opacity", "1");
  await testInfo.attach("music-symbols", { body: await page.screenshot(), contentType: "image/png" });
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await page.evaluate(() => {
      const fixture = (window as unknown as { subtitleFixture: { renderer: { updateAppearance(style: unknown, position: number): void }; normalizeSubtitleAppearance(value: unknown): unknown } }).subtitleFixture;
      fixture.renderer.updateAppearance(fixture.normalizeSubtitleAppearance({ fontSizePx: 48, lineHeight: 1, paddingX: 64, paddingY: 64, borderRadius: 32, backgroundOpacity: 0 }), 100);
    });
    await expect(box).toHaveCSS("padding", "64px");
    await expect(box).toHaveCSS("border-radius", "32px");
    const bounds = await box.boundingBox();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.y).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(width);
    expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(844);
  }
  await page.evaluate(() => {
    const fixture = (window as unknown as { subtitleFixture: { store: { accept(cue: unknown): void }; cue: Record<string, unknown> } }).subtitleFixture;
    fixture.store.accept({ ...fixture.cue, sequence: 2, kind: "bitmap", text: "" });
  });
  await expect(page.locator("#subtitle-fixture .panorama-subtitle-layer")).toBeHidden();
  await page.evaluate(() => (window as unknown as { subtitleFixture: { renderer: { destroy(): void } } }).subtitleFixture.renderer.destroy());
  await expect(box).toHaveCount(0);
});
