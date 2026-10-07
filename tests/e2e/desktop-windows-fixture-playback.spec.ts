import { createServer } from "node:http";
import { _electron as electron, expect, test } from "@playwright/test";
import { desktopLaunchOptions } from "./desktop-executable";

function chunk(name: string, data: Buffer): Buffer {
  const header = Buffer.alloc(8);
  header.write(name); header.writeUInt32LE(data.length, 4);
  return Buffer.concat([header, data, ...(data.length % 2 ? [Buffer.alloc(1)] : [])]);
}

// An owned, silent, uncompressed AVI fixture; this does not prove hardware decoding.
function fixtureVideo(seconds: number): Buffer {
  const width = 160, height = 90, frameBytes = width * height * 3;
  const main = Buffer.alloc(56);
  [1_000_000, frameBytes, 0, 0x10, seconds, 0, 1, frameBytes, width, height].forEach((v, i) => main.writeUInt32LE(v, i * 4));
  const stream = Buffer.alloc(56);
  stream.write("vids"); stream.write("DIB ", 4);
  stream.writeUInt32LE(1, 20); stream.writeUInt32LE(1, 24);
  stream.writeUInt32LE(seconds, 32); stream.writeUInt32LE(frameBytes, 36);
  stream.writeUInt32LE(0xffffffff, 40); stream.writeInt16LE(width, 52); stream.writeInt16LE(height, 54);
  const format = Buffer.alloc(40);
  format.writeUInt32LE(40); format.writeInt32LE(width, 4); format.writeInt32LE(height, 8);
  format.writeUInt16LE(1, 12); format.writeUInt16LE(24, 14); format.writeUInt32LE(frameBytes, 20);
  const headers = chunk("LIST", Buffer.concat([Buffer.from("hdrl"), chunk("avih", main),
    chunk("LIST", Buffer.concat([Buffer.from("strl"), chunk("strh", stream), chunk("strf", format)]))]));
  const frame = chunk("00db", Buffer.alloc(frameBytes, 100));
  const index = Buffer.alloc(seconds * 16);
  for (let i = 0; i < seconds; i++) {
    index.write("00db", i * 16); index.writeUInt32LE(0x10, i * 16 + 4);
    index.writeUInt32LE(4 + i * frame.length, i * 16 + 8); index.writeUInt32LE(frameBytes, i * 16 + 12);
  }
  return chunk("RIFF", Buffer.concat([Buffer.from("AVI "), headers,
    chunk("LIST", Buffer.concat([Buffer.from("movi"), ...Array<Buffer>(seconds).fill(frame)])), chunk("idx1", index)]));
}

test("Windows package plays owned media, switches sources, and loads subtitles asynchronously", async ({}, testInfo) => {
  test.skip(process.platform !== "win32" || testInfo.project.name !== "chromium" || process.env.PANORAMA_DESKTOP_E2E !== "1");
  const soak = process.env.PANORAMA_DESKTOP_SOAK === "1";
  test.setTimeout(soak ? 33 * 60_000 : 120_000);
  const video = fixtureVideo(soak ? 1860 : 60);
  const server = createServer((request, response) => {
    if (request.url === "/missing.avi") { response.writeHead(404); response.end(); return; }
    if (request.url?.endsWith(".srt")) {
      response.writeHead(200, { "Content-Type": "text/plain" });
      response.end("1\n00:00:00,000 --> 00:31:00,000\nPanorama native subtitle fixture\n");
      return;
    }
    const start = Number(request.headers.range?.match(/bytes=(\d+)-/)?.[1] ?? 0);
    response.writeHead(start ? 206 : 200, {
      "Content-Type": "video/x-msvideo", "Accept-Ranges": "bytes", "Content-Length": video.length - start,
      ...(start ? { "Content-Range": `bytes ${start}-${video.length - 1}/${video.length}` } : {}),
    });
    response.end(video.subarray(start));
  });
  await new Promise<void>((resolve, reject) => { server.once("error", reject); server.listen(11474, "127.0.0.1", resolve); });
  const app = await electron.launch(desktopLaunchOptions());
  try {
    const page = await app.firstWindow();
    const sleepBlocked = () => app.evaluate(({ powerSaveBlocker }) => Array.from({ length: 16 }, (_, id) => id).some((id) => powerSaveBlocker.isStarted(id)));
    await expect.poll(() => page.evaluate(async () => (await window.panoramaDesktop?.getCapabilities())?.nativePlayback.status)).toBe("ready");
    await page.evaluate(() => {
      const events: Array<{ channel: string; payload: unknown }> = [];
      Object.assign(window, { nativeFixtureEvents: events });
      const mpv = window.panoramaDesktop!.mpv!;
      for (const channel of ["mpv-prop-change", "mpv-event-ended", "mpv-event-video-ready"] as const) {
        mpv.on(channel, (payload) => events.push({ channel, payload }));
      }
      for (const prop of ["time-pos", "duration", "track-list", "sid"]) mpv.send("mpv-observe-prop", prop);
      mpv.setVideoSurface({ visible: true, x: 0, y: 32, width: 640, height: 360, scaleFactor: window.devicePixelRatio });
      mpv.send("mpv-command", ["loadfile", "http://127.0.0.1:11474/first.avi"]);
    });
    const time = () => page.evaluate(() => {
      const events = (window as unknown as { nativeFixtureEvents: Array<{ channel: string; payload: { name?: string; data?: number } }> }).nativeFixtureEvents;
      return events.filter((e) => e.payload.name === "time-pos").at(-1)?.payload.data ?? 0;
    });
    await expect.poll(time, { timeout: 20_000 }).toBeGreaterThan(1);
    await expect.poll(sleepBlocked).toBe(true);
    await page.evaluate(() => window.panoramaDesktop!.mpv!.send("mpv-set-prop", ["pause", true]));
    await expect.poll(sleepBlocked).toBe(false);
    await page.evaluate(() => window.panoramaDesktop!.mpv!.send("mpv-set-prop", ["pause", false]));
    await expect.poll(sleepBlocked).toBe(true);
    await expect.poll(() => page.evaluate(async () => (await window.panoramaDesktop?.getPlaybackDiagnostics?.())?.videoWidth)).toBe(160);
    await page.evaluate(() => {
      const mpv = window.panoramaDesktop!.mpv!;
      mpv.send("mpv-command", ["sub-add", "http://127.0.0.1:11474/english.srt", "cached", "Panorama addon · fixture", "en"]);
    });
    await expect.poll(() => page.evaluate(() => {
      const events = (window as unknown as { nativeFixtureEvents: Array<{ payload: { name?: string; data?: Array<{ title?: string }> } }> }).nativeFixtureEvents;
      return events.filter((e) => e.payload.name === "track-list").at(-1)?.payload.data?.some((track) => track.title === "Panorama addon · fixture");
    }), { timeout: 20_000 }).toBe(true);
    await page.evaluate(() => {
      const mpv = window.panoramaDesktop!.mpv!;
      mpv.send("mpv-set-prop", ["time-pos", 10]);
      mpv.send("mpv-command", ["loadfile", "http://127.0.0.1:11474/second.avi", "replace", "-1", "start=+10"]);
      mpv.send("mpv-command", ["loadfile", "http://127.0.0.1:11474/third.avi", "replace", "-1", "start=+10"]);
    });
    await expect.poll(time, { timeout: 20_000 }).toBeGreaterThan(10);
    expect(await page.evaluate(() => (window as unknown as { nativeFixtureEvents: Array<{ channel: string }> }).nativeFixtureEvents.filter((e) => e.channel === "mpv-event-ended"))).toEqual([]);
    await app.evaluate(({ BrowserWindow }) => { const win = BrowserWindow.getAllWindows()[0]; win.setSize(1000, 700); win.minimize(); });
    await expect.poll(() => app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].isMinimized())).toBe(true);
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].restore());
    await expect.poll(() => app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].isMinimized())).toBe(false);
    const windowedBounds = await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].getBounds());
    const displayBounds = await app.evaluate(({ BrowserWindow, screen }) => screen.getDisplayMatching(BrowserWindow.getAllWindows()[0].getBounds()).bounds);
    await page.evaluate(() => window.panoramaDesktop!.setFullscreen!(true));
    // Electron 43 uses bounds-based fullscreen for transparent Windows windows;
    // isFullScreen() queries the widget flag rather than that path.
    await expect(page.locator("body")).toHaveClass(/panorama-desktop-fullscreen/);
    await expect(page.locator(".desktop-titlebar")).toBeHidden();
    await expect.poll(() => app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].getBounds())).toEqual(displayBounds);
    await page.evaluate(() => window.panoramaDesktop!.setFullscreen!(false));
    await expect(page.locator("body")).not.toHaveClass(/panorama-desktop-fullscreen/);
    await expect.poll(() => app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].getBounds())).toEqual(windowedBounds);
    if (soak) {
      const start = Date.now();
      let previousTime = await time();
      const samples: Array<{ elapsedSeconds: number; time: number; diagnostics: unknown }> = [];
      while (Date.now() - start < 30 * 60_000) {
        await page.waitForTimeout(30_000);
        const diagnostics = await page.evaluate(() => window.panoramaDesktop?.getPlaybackDiagnostics?.());
        expect(diagnostics?.videoWidth).toBe(160);
        expect(await sleepBlocked()).toBe(true);
        const currentTime = await time();
        expect(currentTime).toBeGreaterThan(previousTime);
        previousTime = currentTime;
        const elapsedSeconds = Math.round((Date.now() - start) / 1000);
        samples.push({ elapsedSeconds, time: currentTime, diagnostics });
        console.log(JSON.stringify({ elapsedSeconds, time: currentTime, buffering: diagnostics?.buffering, frameDrops: diagnostics?.frameDropCount }));
      }
      await testInfo.attach("native-soak-diagnostics", { body: Buffer.from(JSON.stringify(samples, null, 2)), contentType: "application/json" });
    }
    const ended = (reason: string) => page.evaluate((expected) =>
      (window as unknown as { nativeFixtureEvents: Array<{ channel: string; payload: { reason?: string } }> })
        .nativeFixtureEvents.some((event) => event.channel === "mpv-event-ended" && event.payload.reason === expected), reason);
    await page.evaluate((seconds) => window.panoramaDesktop!.mpv!.send("mpv-set-prop", ["time-pos", seconds]), soak ? 1858 : 58);
    await expect.poll(() => ended("eof"), { timeout: 15_000 }).toBe(true);
    await expect.poll(sleepBlocked).toBe(false);
    await page.evaluate(() => window.panoramaDesktop!.mpv!.send("mpv-command", ["loadfile", "http://127.0.0.1:11474/missing.avi"]));
    await expect.poll(() => ended("error"), { timeout: 15_000 }).toBe(true);
    await page.evaluate(() => window.panoramaDesktop!.mpv!.send("mpv-command", ["stop"]));
    await expect.poll(sleepBlocked).toBe(false);
  } finally {
    await app.close();
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});
