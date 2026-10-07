import { app, BrowserWindow, ipcMain } from "electron";
import { mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { MpvHost } from "../native/mpv-host/mpv-host";
import type { NativeAddon } from "../native/mpv-host/native-binding";
import { createNativeBinding } from "../main/native-binding-factory";
import { parseVideoSurface } from "../shared/mpv-protocol";
import { createSpikeWindowOptions, isAllowedSpikeNavigation } from "./security";
import type { SpikeDiagnostics } from "./types";

const requireNativeAddon = createRequire(__filename);
const spikeDataRoot = path.resolve(process.cwd(), ".cache/panorama/spike-electron");
const spikeSessionRoot = path.join(spikeDataRoot, "session");
mkdirSync(spikeSessionRoot, { recursive: true });
app.setPath("userData", spikeDataRoot);
app.setPath("sessionData", spikeSessionRoot);

function loadNativeAddon(): NativeAddon {
  const addonPath = path.resolve(
    process.cwd(),
    "desktop/native/mpv-host/build/Release/mpv_host.node",
  );
  return requireNativeAddon(addonPath) as NativeAddon;
}

function registerSpikeIpc(host: MpvHost, mediaUrl: string | null): void {
  ipcMain.handle("spike:play", async () => {
    if (!mediaUrl) throw new Error("Set PANORAMA_SPIKE_MEDIA_URL in the main process.");
    host.dispatch({ type: "load", url: mediaUrl, startSeconds: 0 });
  });
  ipcMain.handle("spike:pause", async () => {
    host.dispatch({ type: "setPaused", paused: true });
  });
  ipcMain.handle("spike:get-diagnostics", async (): Promise<SpikeDiagnostics> => host.getDiagnostics());
  ipcMain.on("panorama:spike:set-video-bounds", (_event, value: unknown) => {
    const bounds = parseVideoSurface(value);
    host.dispatch({
      type: "setBounds",
      x: bounds.visible ? bounds.x : 0,
      y: bounds.visible ? bounds.y : 0,
      width: bounds.visible ? bounds.width : 0,
      height: bounds.visible ? bounds.height : 0,
      scaleFactor: bounds.scaleFactor,
    });
  });
}

function removeSpikeIpc(): void {
  ipcMain.removeHandler("spike:play");
  ipcMain.removeHandler("spike:pause");
  ipcMain.removeHandler("spike:get-diagnostics");
  ipcMain.removeAllListeners("panorama:spike:set-video-bounds");
}

function exerciseNativePlayback(window: BrowserWindow, host: MpvHost): void {
  const originalSize = window.getContentSize();
  setTimeout(() => host.dispatch({ type: "setPaused", paused: true }), 10_000);
  setTimeout(() => host.dispatch({ type: "setPaused", paused: false }), 12_000);
  setTimeout(() => {
    const current = host.getDiagnostics().timeSeconds ?? 0;
    host.dispatch({ type: "seek", seconds: current + 60 });
  }, 15_000);
  setTimeout(() => {
    const current = host.getDiagnostics().timeSeconds ?? 30;
    host.dispatch({ type: "seek", seconds: Math.max(0, current - 30) });
  }, 20_000);
  setTimeout(() => window.setContentSize(960, 540), 25_000);
  setTimeout(() => window.setContentSize(originalSize[0], originalSize[1]), 28_000);
  setTimeout(() => window.setFullScreen(true), 32_000);
  setTimeout(() => window.setFullScreen(false), 36_000);
}

async function createWindow(): Promise<BrowserWindow> {
  if (!((process.platform === "darwin" && process.arch === "arm64") || (process.platform === "win32" && process.arch === "x64"))) {
    throw new Error("Native playback requires macOS arm64 or Windows x64.");
  }
  const preload = path.join(__dirname, "preload.js");
  const transparent = process.platform === "darwin" || process.env.PANORAMA_SPIKE_WINDOW_MODE !== "opaque";
  const window = new BrowserWindow({
    ...createSpikeWindowOptions(preload),
    transparent,
    frame: !transparent,
    backgroundColor: transparent ? "#00000000" : "#080808",
    title: "Panorama Native Playback Spike",
  });
  console.log("SPIKE_STAGE", "window-created");
  const runtimeDirectory = path.resolve(process.cwd(), ".cache/panorama/windows-libmpv/current");
  const binding = createNativeBinding({
    platform: process.platform,
    architecture: process.arch,
    nativeWindowHandle: window.getNativeWindowHandle(),
    runtimeDirectory,
    addon: loadNativeAddon(),
  });
  if (!binding) throw new Error("Native playback is unsupported");
  console.log("SPIKE_STAGE", "native-host-created");
  const mediaUrl = process.env.PANORAMA_SPIKE_MEDIA_URL?.trim() || null;
  const host = new MpvHost(binding, "http://127.0.0.1:11470", mediaUrl);

  registerSpikeIpc(host, mediaUrl);
  window.on("closed", () => {
    removeSpikeIpc();
    host.destroy();
  });

  window.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
  window.webContents.session.setPermissionRequestHandler((_webContents, _permission, callback) => callback(false));
  console.log("SPIKE_STAGE", "renderer-loading");
  await window.loadFile(path.join(__dirname, "index.html"));
  console.log("SPIKE_STAGE", "renderer-loaded");
  const rendererUrl = window.webContents.getURL();
  window.webContents.on("will-navigate", (event, target) => {
    if (!isAllowedSpikeNavigation(target, rendererUrl)) event.preventDefault();
  });
  if (process.env.PANORAMA_SPIKE_AUTOPLAY === "1" && mediaUrl) {
    host.dispatch({ type: "load", url: mediaUrl, startSeconds: 0 });
  }
  if (process.env.PANORAMA_SPIKE_EXERCISE === "1") {
    exerciseNativePlayback(window, host);
  }
  if (process.env.PANORAMA_SPIKE_DIAGNOSTICS === "1") {
    const diagnosticsTimer = setInterval(() => console.log("MPV_DIAGNOSTICS", host.getDiagnostics()), 2_000);
    window.on("closed", () => clearInterval(diagnosticsTimer));
  }
  return window;
};

app.whenReady().then(async () => {
  await createWindow();
});

app.on("window-all-closed", () => app.quit());
