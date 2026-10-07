import { access, stat } from "node:fs/promises";
import { createReadStream } from "node:fs";
import { createServer } from "node:http";
import { _electron as electron, expect, test } from "@playwright/test";
import { desktopExecutablePath, desktopLaunchOptions } from "./desktop-executable";

test("packaged desktop exposes a healthy ShellVideo capability", async ({}, testInfo) => {
  test.skip(testInfo.project.name !== "chromium" || process.env.PANORAMA_DESKTOP_E2E !== "1");
  const executablePath = desktopExecutablePath();
  await access(executablePath);
  const electronApp = await electron.launch(desktopLaunchOptions());
  try {
    const page = await electronApp.firstWindow();
    const capabilities = await page.evaluate(() => window.panoramaDesktop?.getCapabilities());
    expect(capabilities?.nativePlayback).toMatchObject({ status: "ready", device: "ShellVideo" });
    expect(capabilities?.nativePlayback.status === "ready" ? capabilities.nativePlayback.mpvVersion : null).toBeTruthy();
    const diagnostics = await page.evaluate(() => window.panoramaDesktop?.getPlaybackDiagnostics?.());
    expect(diagnostics).toMatchObject({
      cacheSeconds: null,
      cacheEndSeconds: null,
      audioCodec: null,
      audioChannels: null,
      audioOutputChannels: null,
      audioSampleRate: null,
      audioOutputSampleRate: null,
      videoPixelFormat: null,
      videoColorPrimaries: null,
      videoTransferFunction: null,
    });
    for (const value of [diagnostics?.cacheBufferingPercent, diagnostics?.cacheForwardBytes, diagnostics?.inputBytesPerSecond]) {
      expect(value === null || (typeof value === "number" && Number.isFinite(value))).toBe(true);
    }
    await page.evaluate(() => window.panoramaDesktop?.mpv?.setVideoSurface({
      visible: false,
      x: 0,
      y: 0,
      width: 0,
      height: 0,
      scaleFactor: window.devicePixelRatio,
    }));
  } finally {
    await electronApp.close();
  }
});


for (const hostname of ["127.0.0.1", "localhost"]) test(`packaged native playback controls and cleanup through ${hostname}`, async ({}, testInfo) => {
  const media = process.env.PANORAMA_DESKTOP_TEST_MEDIA;
  test.skip(testInfo.project.name !== "chromium" || process.env.PANORAMA_DESKTOP_E2E !== "1" || !media);
  const { size } = await stat(media!);
  const server = createServer((request, response) => {
    const start = Number(request.headers.range?.match(/bytes=(\d+)-/)?.[1] ?? 0);
    const end = Math.min(size - 1, Number(request.headers.range?.match(/bytes=\d+-(\d+)/)?.[1] ?? size - 1));
    if (start >= size) { response.writeHead(416); response.end(); return; }
    response.writeHead(request.headers.range ? 206 : 200, {
      "Accept-Ranges": "bytes", "Content-Length": end - start + 1, ETag: '"packaged-fixture-v1"',
      ...(request.headers.range ? { "Content-Range": `bytes ${start}-${end}/${size}` } : {}),
    });
    const stream = createReadStream(media!, { start, end });
    response.on("close", () => stream.destroy());
    stream.pipe(response);
  });
  await new Promise<void>((resolve, reject) => { server.once("error", reject); server.listen(11474, "127.0.0.1", resolve); });
  let app: Awaited<ReturnType<typeof electron.launch>> | undefined;
  let unsupportedBufferImports = 0;
  try {
    app = await electron.launch(desktopLaunchOptions());
    app.process().stderr?.on("data", (chunk: Buffer) => {
      if (chunk.toString().includes("VK_ERROR_FEATURE_NOT_PRESENT: vkCreateBuffer")) unsupportedBufferImports += 1;
    });
    const page = await app.firstWindow();
    const diagnostics = () => page.evaluate(() => window.panoramaDesktop!.getPlaybackDiagnostics!());
    await page.evaluate((host) => {
      const mpv = window.panoramaDesktop!.mpv!;
      mpv.send("mpv-set-prop", ["vo", "libmpv"]);
      mpv.send("mpv-set-prop", ["osc", "no"]);
      mpv.send("mpv-set-prop", ["hwdec", "auto-copy"]);
      mpv.setVideoSurface({ visible: true, x: 0, y: 38, width: 640, height: 360, scaleFactor: window.devicePixelRatio });
      mpv.send("mpv-set-prop", ["mute", true]);
      mpv.send("mpv-command", ["loadfile", `http://${host}:11474/media`]);
      mpv.send("mpv-set-prop", ["pause", false]);
    }, hostname);
    await expect.poll(async () => (await diagnostics())?.cacheForwardBytes, { timeout: 20_000 }).toBeGreaterThan(0);
    await expect.poll(async () => (await diagnostics())?.audioSampleRate).toBeGreaterThan(0);
    await expect.poll(async () => {
      const value = await diagnostics();
      return value?.cacheEndSeconds != null && value.cacheSeconds != null && value.cacheEndSeconds > value.cacheSeconds;
    }).toBe(true);
    const value = await diagnostics();
    expect(value?.audioCodec).toBeTruthy();
    expect(value?.audioChannels).toBeTruthy();
    expect(value?.audioOutputSampleRate).toBeGreaterThan(0);
    expect(value?.videoPixelFormat).toBeTruthy();
    expect(unsupportedBufferImports).toBe(0);
    if (process.platform === "darwin" && process.env.PANORAMA_MPV_RENDERER !== "opengl") {
      expect(value?.rendererBackend).toBe("macvk");
    }
    await page.evaluate(() => {
      const mpv = window.panoramaDesktop!.mpv!;
      const properties: Record<string, unknown> = {};
      Object.assign(window, { playbackTestProperties: properties });
      mpv.on("mpv-prop-change", (payload) => {
        const property = payload as { name: string; data: unknown };
        properties[property.name] = property.data;
      });
      for (const name of ["pause", "time-pos"]) mpv.send("mpv-observe-prop", name);
      mpv.send("mpv-set-prop", ["pause", true]);
    });
    const property = (name: string) => page.evaluate((key) => (window as unknown as { playbackTestProperties: Record<string, unknown> }).playbackTestProperties[key], name);
    await expect.poll(() => property("pause")).toBe(true);
    const pausedTime = Number(await property("time-pos"));
    await page.waitForTimeout(500);
    expect(Number(await property("time-pos")) - pausedTime).toBeLessThan(0.1);
    await page.evaluate(() => window.panoramaDesktop!.mpv!.send("mpv-set-prop", ["time-pos", 45]));
    await expect.poll(() => property("time-pos"), { timeout: 20_000 }).toBeGreaterThanOrEqual(44);
    await page.evaluate(() => window.panoramaDesktop!.mpv!.send("mpv-set-prop", ["pause", false]));
    await expect.poll(() => property("pause")).toBe(false);
    await expect.poll(() => property("time-pos"), { timeout: 20_000 }).toBeGreaterThan(45.2);
    await page.evaluate(() => window.panoramaDesktop!.mpv!.send("mpv-command", ["stop"]));
    await expect.poll(async () => (await diagnostics())?.cacheEndSeconds).toBeNull();
    await expect.poll(async () => (await diagnostics())?.cacheSeconds).toBeNull();
    await expect.poll(async () => (await diagnostics())?.audioOutputSampleRate).toBeNull();
  } finally {
    if (app) await app.close();
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});
