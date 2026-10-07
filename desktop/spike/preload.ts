import { contextBridge, ipcRenderer } from "electron";
import type { PanoramaSpike, SpikeDiagnostics } from "./types";

const api: PanoramaSpike = Object.freeze({
  play: () => ipcRenderer.invoke("spike:play") as Promise<void>,
  pause: () => ipcRenderer.invoke("spike:pause") as Promise<void>,
  getDiagnostics: () => ipcRenderer.invoke("spike:get-diagnostics") as Promise<SpikeDiagnostics>,
  setVideoBounds: (bounds) => ipcRenderer.send("panorama:spike:set-video-bounds", bounds),
});

contextBridge.exposeInMainWorld("panoramaSpike", api);
