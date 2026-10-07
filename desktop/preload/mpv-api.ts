import { parseSubtitleCue } from "../shared/subtitle-cue";
import { ipcRenderer } from "electron";
import type { PanoramaDesktopApi } from "../shared/desktop-api";

export function createMpvApi(): NonNullable<PanoramaDesktopApi["mpv"]> {
  return Object.freeze({
    send(channel, payload) {
      ipcRenderer.send("panorama:mpv-send", channel, payload);
    },
    on(channel, listener) {
      const handler = (_event: Electron.IpcRendererEvent, receivedChannel: unknown, payload: unknown) => {
        if (receivedChannel !== channel) return;
        if (channel === "mpv-event-subtitle-cue") { listener(parseSubtitleCue(payload)); return; }
        listener(payload);
      };
      ipcRenderer.on("panorama:mpv-event", handler);
      return () => ipcRenderer.removeListener("panorama:mpv-event", handler);
    },
    setVideoSurface(bounds) {
      ipcRenderer.send("panorama:set-video-surface", bounds);
    },
  });
}
