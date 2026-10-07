import { contextBridge, ipcRenderer } from "electron";
import type { DesktopCapabilities, DesktopExternalTarget, DesktopPlaybackDiagnostics, DesktopWindowControl, PanoramaDesktopApi } from "../shared/desktop-api";

const mpv: NonNullable<PanoramaDesktopApi["mpv"]> = Object.freeze({
  send(channel, payload) {
    ipcRenderer.send("panorama:mpv-send", channel, payload);
  },
  on(channel, listener) {
    const handler = (_event: Electron.IpcRendererEvent, receivedChannel: unknown, payload: unknown) => {
      if (receivedChannel === channel) listener(payload);
    };
    ipcRenderer.on("panorama:mpv-event", handler);
    return () => ipcRenderer.removeListener("panorama:mpv-event", handler);
  },
  setVideoSurface(bounds) {
    ipcRenderer.send("panorama:set-video-surface", bounds);
  },
});

const api: PanoramaDesktopApi = Object.freeze({
  getDiscordSettings: () => ipcRenderer.invoke("panorama:get-discord-settings"),
  setDiscordEnabled: (enabled) => ipcRenderer.invoke("panorama:set-discord-enabled", enabled),
  updateDiscordPlayback: (playback) => ipcRenderer.send("panorama:discord-playback", playback),
  getPlaybackSettings: () => ipcRenderer.invoke("panorama:get-playback-settings"),
  setPlaybackSettings: (settings) => ipcRenderer.invoke("panorama:set-playback-settings", settings),
  getCapabilities: () => ipcRenderer.invoke("panorama:get-capabilities") as Promise<DesktopCapabilities>,
  getPlaybackDiagnostics: () => ipcRenderer.invoke("panorama:get-playback-diagnostics") as Promise<DesktopPlaybackDiagnostics | null>,
  recordPlaybackTiming: (event) => ipcRenderer.send("panorama:record-playback-timing", event),
  warmMedia: (url) => ipcRenderer.invoke("panorama:warm-media", url),
  setFullscreen: (fullscreen) => ipcRenderer.invoke("panorama:set-fullscreen", fullscreen) as Promise<void>,
  onFullscreenChange(listener) {
    const handler = (_event: Electron.IpcRendererEvent, fullscreen: unknown) => {
      if (typeof fullscreen === "boolean") listener(fullscreen);
    };
    ipcRenderer.on("panorama:fullscreen-changed", handler);
    return () => ipcRenderer.removeListener("panorama:fullscreen-changed", handler);
  },
  controlWindow: (action: DesktopWindowControl) => ipcRenderer.invoke("panorama:window-control", action) as Promise<void>,
  openExternal: (target: DesktopExternalTarget) => ipcRenderer.invoke("panorama:open-external", target) as Promise<void>,
  mpv,
});

if (process.platform === "win32") {
  const markWindowsShell = () => document.body?.classList.add("panorama-windows");
  if (document.readyState === "loading") window.addEventListener("DOMContentLoaded", markWindowsShell, { once: true });
  else markWindowsShell();
}

contextBridge.exposeInMainWorld("panoramaDesktop", api);
