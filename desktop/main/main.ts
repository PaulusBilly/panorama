import { app, BrowserWindow, ipcMain, Menu, powerSaveBlocker, screen } from "electron";
import { appendFile, existsSync, renameSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { createMainWindow, isAllowedDevelopmentOrigin, openApprovedExternal } from "./create-main-window";
import { startRendererServer, type RendererServer } from "./renderer-server";
import { startBundledStremioServer, type BundledStremioServer } from "./stremio-server";
import type { DesktopCapabilities, DesktopExternalTarget, DesktopWindowControl, PlaybackRendererTimingEvent } from "../shared/desktop-api";
import type { NativePlaybackCapability } from "../shared/desktop-api";
import type { ShellSendChannel } from "../shared/mpv-protocol";
import { MpvController } from "./mpv-controller";
import { MediaProxy } from "./media-proxy";
import { createMediaFetch } from "./media-fetch";
import { MpvHost } from "../native/mpv-host/mpv-host";
import type { NativeAddon } from "../native/mpv-host/native-binding";
import { createNativeBinding } from "./native-binding-factory";
import { resolveNativeResourcePaths } from "./platform-paths";
import { DiscordPresence } from "./discord-presence";

let rendererServer: RendererServer | null = null;
let stremioServer: Promise<BundledStremioServer | null> | null = null;
let discordPresence: DiscordPresence | null = null;
let quitting = false;
let mpvController: MpvController | null = null;
let displaySleepBlockerId: number | null = null;

let mediaProxy: MediaProxy | null = null;

const PLAYBACK_LOG_MAX_BYTES = 5 * 1_048_576;

function createPlaybackLog(file: string): (line: string) => void {
  let checked = 0;
  return (line) => {
    if (++checked % 60 === 1) {
      try {
        if (statSync(file).size > PLAYBACK_LOG_MAX_BYTES) renameSync(file, `${file}.1`);
      } catch {}
    }
    appendFile(file, `${line}\n`, () => undefined);
  };
}
let nativePlayback: NativePlaybackCapability = { status: "unavailable", reason: "missing-host" };
const stremioServiceOrigins = [11470, 11471, 11472, 11473, 11474]
  .map((port) => `http://127.0.0.1:${port}`);
const requireNativeAddon = createRequire(__filename);

function loadNativeAddon(addonPath: string): NativeAddon {
  return requireNativeAddon(addonPath) as NativeAddon;
}

function dockIconPath(): string {
  return app.isPackaged
    ? path.join(process.resourcesPath, "desktop-resources/standalone/public/favicon/macos-icon.png")
    : path.resolve(process.cwd(), "public/favicon/macos-icon.png");
}

function capabilities(): DesktopCapabilities {
  if (process.platform !== "darwin" && process.platform !== "win32") throw new Error("Unsupported desktop platform");
  if (process.arch !== "arm64" && process.arch !== "x64") throw new Error("Unsupported desktop architecture");
  return {
    platform: process.platform,
    architecture: process.arch,
    nativePlayback,
    appVersion: app.getVersion(),
  };
}

function validateSender(senderId: number): void {
  if (!BrowserWindow.getAllWindows().some((window) => window.webContents.id === senderId)) {
    throw new Error("Untrusted IPC sender");
  }
}

function setPlaybackActive(active: boolean): void {
  if (active && displaySleepBlockerId === null) {
    displaySleepBlockerId = powerSaveBlocker.start("prevent-display-sleep");
  } else if (!active && displaySleepBlockerId !== null) {
    powerSaveBlocker.stop(displaySleepBlockerId);
    displaySleepBlockerId = null;
  }
}

async function initializeNativePlayback(window: BrowserWindow): Promise<void> {
  window.webContents.on("render-process-gone", () => discordPresence?.clear());
  window.webContents.on("did-start-loading", () => discordPresence?.clear());
  window.on("closed", () => discordPresence?.clear());
  const sendFullscreenState = (fullscreen: boolean) => {
    if (!window.webContents.isDestroyed()) {
      window.webContents.send("panorama:fullscreen-changed", fullscreen);
    }
  };
  window.on("enter-full-screen", () => {
    mpvController?.suspendSurface(false);
    sendFullscreenState(true);
  });
  window.on("leave-full-screen", () => sendFullscreenState(false));
  const supported = (process.platform === "darwin" && process.arch === "arm64")
    || (process.platform === "win32" && process.arch === "x64");
  if (!supported) {
    nativePlayback = { status: "unavailable", reason: "unsupported-platform" };
    return;
  }
  try {
    const resourcePaths = resolveNativeResourcePaths({
      packaged: app.isPackaged,
      platform: process.platform,
      architecture: process.arch,
      resourcesPath: process.resourcesPath,
      projectRoot: process.cwd(),
    });
    // The native host keeps a temporary read-ahead cache here; MPV deletes its
    // files when media closes, and leftovers from a crash are removed now.
    const binding = createNativeBinding({
      platform: process.platform,
      architecture: process.arch,
      nativeWindowHandle: window.getNativeWindowHandle(),
      runtimeDirectory: resourcePaths.runtimeDirectory,
      addon: loadNativeAddon(resourcePaths.addon),
    });
    if (!binding) throw new Error("Native playback is unsupported");
    binding.dispatch({ type: "setProperty", name: "volume-max", value: 200 });
    binding.dispatch({ type: "setProperty", name: "sub-fonts-dir", value: resourcePaths.subtitleFontDirectory });
    binding.dispatch({ type: "setProperty", name: "sub-font", value: "DM Sans" });
    binding.dispatch({ type: "setProperty", name: "sub-border-style", value: "background-box" });
    binding.dispatch({ type: "setProperty", name: "sub-border-size", value: 0 });
    binding.dispatch({ type: "setProperty", name: "sub-shadow-offset", value: 10 });
    const playbackLog = createPlaybackLog(path.join(app.getPath("userData"), "playback-log.jsonl"));
    // Chromium's network stack connects more patiently over congested routes
    // than Node's fetch, whose 10 s connect timeout failed on long-distance hosts.
    mediaProxy = new MediaProxy(createMediaFetch(), Date.now, undefined,
      (event) => playbackLog(JSON.stringify({ at: new Date().toISOString(), proxy: event })));
    await mediaProxy.start();
    mpvController = new MpvController(
      new MpvHost(binding, stremioServiceOrigins),
      (channel, payload) => window.webContents.send("panorama:mpv-event", channel, payload),
      100,
      Date.now,
      setPlaybackActive,
      path.join(app.getPath("userData"), "playback-settings.json"),
      playbackLog,
      mediaProxy,
    );
    nativePlayback = await mpvController.waitUntilReady();
    const refreshSurface = () => mpvController?.refreshSurface();
    if (process.platform === "win32") {
      window.on("minimize", () => mpvController?.suspendSurface(true));
      window.on("restore", () => mpvController?.suspendSurface(false));
      window.on("resize", refreshSurface);
      window.on("move", refreshSurface);
      window.on("maximize", refreshSurface);
      window.on("unmaximize", refreshSurface);
      screen.on("display-metrics-changed", refreshSurface);
    }
    window.on("closed", () => {
      screen.removeListener("display-metrics-changed", refreshSurface);
      mpvController?.destroy();
      mediaProxy?.destroy();
      mediaProxy = null;
      mpvController = null;
    });
  } catch {
    nativePlayback = { status: "unavailable", reason: "initialization-failed" };
  }
}

app.whenReady().then(async () => {
  discordPresence = new DiscordPresence(path.join(app.getPath("userData"), "discord-settings.json"));
  ipcMain.handle("panorama:get-discord-settings", (event) => {
    validateSender(event.sender.id);
    return discordPresence?.getSettings();
  });
  ipcMain.handle("panorama:set-discord-enabled", (event, enabled: unknown) => {
    validateSender(event.sender.id);
    return discordPresence?.setEnabled(enabled);
  });
  ipcMain.on("panorama:discord-playback", (event, playback: unknown) => {
    try {
      validateSender(event.sender.id);
      discordPresence?.update(playback);
    } catch {}
  });
  if (process.platform === "win32") Menu.setApplicationMenu(null);
  if (process.platform === "darwin") app.dock?.setIcon(dockIconPath());
  ipcMain.handle("panorama:get-capabilities", (event) => {
    validateSender(event.sender.id);
    return capabilities();
  });
  ipcMain.handle("panorama:get-playback-diagnostics", (event) => {
    validateSender(event.sender.id);
    return mpvController?.getPlaybackDiagnostics() ?? null;
  });
  ipcMain.handle("panorama:get-playback-settings", (event) => {
    validateSender(event.sender.id);
    if (!mpvController) throw new Error("Native playback is unavailable");
    return mpvController.getPlaybackSettings();
  });
  ipcMain.handle("panorama:set-playback-settings", (event, settings: unknown) => {
    validateSender(event.sender.id);
    if (!mpvController) throw new Error("Native playback is unavailable");
    return mpvController.setPlaybackSettings(settings);
  });
  ipcMain.handle("panorama:set-fullscreen", (event, fullscreen: unknown) => {
    validateSender(event.sender.id);
    if (typeof fullscreen !== "boolean") throw new Error("Invalid fullscreen state");
    const window = BrowserWindow.fromWebContents(event.sender);
    if (!window) throw new Error("Fullscreen window is unavailable");
    window.setFullScreen(fullscreen);
  });
  ipcMain.handle("panorama:window-control", (event, action: DesktopWindowControl) => {
    validateSender(event.sender.id);
    if (process.platform !== "win32" || !["minimize", "toggle-maximize", "close"].includes(action)) {
      throw new Error("Unsupported window control");
    }
    const window = BrowserWindow.fromWebContents(event.sender);
    if (!window) throw new Error("Window is unavailable");
    if (action === "minimize") window.minimize();
    else if (action === "toggle-maximize") {
      if (window.isMaximized()) window.unmaximize();
      else window.maximize();
    }
    else window.close();
  });
  ipcMain.on("panorama:record-playback-timing", (event, timingEvent: PlaybackRendererTimingEvent) => {
    validateSender(event.sender.id);
    if (!mpvController) throw new Error("Native playback is unavailable");
    mpvController.recordRendererTiming(timingEvent);
  });
  ipcMain.handle("panorama:warm-media", async (event, url: unknown) => {
    validateSender(event.sender.id);
    if (!mediaProxy || typeof url !== "string" || url.length > 8_192) return { status: "failed", error: "unavailable", unreachable: false };
    let parsed: URL;
    try { parsed = new URL(url); } catch { return { status: "failed", error: "invalid", unreachable: false }; }
    // Only remote HTTP sources are warmed; local Stremio Service links are not proxied.
    if ((parsed.protocol !== "https:" && parsed.protocol !== "http:") || parsed.username || parsed.password ||
      parsed.hostname === "127.0.0.1" || parsed.hostname === "localhost") return { status: "failed", error: "unsupported", unreachable: false };
    return mediaProxy.warm(url);
  });
  ipcMain.handle("panorama:open-external", async (event, target: DesktopExternalTarget) => {
    validateSender(event.sender.id);
    await openApprovedExternal(target);
  });
  ipcMain.on("panorama:mpv-send", (event, channel: ShellSendChannel, payload: unknown) => {
    validateSender(event.sender.id);
    try {
      if (!mpvController) throw new Error("Native playback is unavailable");
      mpvController.handleSend(channel, payload);
    } catch {
      event.sender.send("panorama:mpv-event", "mpv-event-ended", {
        reason: "error",
        error: { critical: true, message: "Unable to apply playback command." },
      });
    }
  });
  ipcMain.on("panorama:set-video-surface", (event, bounds: unknown) => {
    validateSender(event.sender.id);
    if (!mpvController) throw new Error("Native playback is unavailable");
    mpvController.setVideoSurface(bounds);
  });

  const developmentOrigin = app.isPackaged ? undefined : process.env.PANORAMA_DESKTOP_DEV_ORIGIN;
  if (developmentOrigin && !isAllowedDevelopmentOrigin(developmentOrigin)) {
    throw new Error("Desktop development origin must be an exact IPv4 loopback origin");
  }
  // Development builds rely on a separately running Stremio Service.
  const stremioServerPath = path.join(process.resourcesPath, "desktop-resources", "stremio-server", "launch.cjs");
  // Held as a promise so quitting during startup still stops the server.
  if (app.isPackaged && existsSync(stremioServerPath)) {
    stremioServer = startBundledStremioServer({
      serverPath: stremioServerPath,
      origins: stremioServiceOrigins,
      dataDirectory: path.join(app.getPath("userData"), "stremio-server"),
    }).catch(() => null);
  }
  const origin = developmentOrigin ?? (rendererServer = await startRendererServer()).origin;
  await stremioServer;
  await createMainWindow(origin, initializeNativePlayback);
});

app.on("before-quit", (event) => {
  discordPresence?.clear();
  mpvController?.destroy();
  mediaProxy?.destroy();
  mediaProxy = null;
  mpvController = null;
  if ((!rendererServer && !stremioServer) || quitting) return;
  event.preventDefault();
  quitting = true;
  void Promise.allSettled([rendererServer?.stop(), stremioServer?.then((server) => server?.stop())]).finally(() => {
    rendererServer = null;
    stremioServer = null;
    app.quit();
  });
});

app.on("window-all-closed", () => app.quit());
