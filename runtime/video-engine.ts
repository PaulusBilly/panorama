import { parseSubtitleCue, type NativeSubtitleCue } from "../desktop/shared/subtitle-cue";
import { createSubtitleCueStore } from "./subtitle-cue-store";
import { browserSubtitleText, createSubtitleRenderer } from "./subtitle-renderer";
import { readSubtitlePreferences } from "./subtitle-preferences";
import { normalizeSubtitleAppearance } from "./subtitle-appearance";
import { getDesktopCapabilities } from "./desktop-capabilities";
import { createDesktopShellTransport, type DesktopShellTransport } from "./desktop-shell-transport";

export type VideoEngine = {
  on(eventName: string, listener: (...args: unknown[]) => void): void;
  dispatch(action: Record<string, unknown>, options?: Record<string, unknown>): void;
  destroy(): void;
};

export type VideoEngineDevice = "HTMLVideo" | "ShellVideo";

export const NATIVE_ADDON_SUBTITLE_TITLE_PREFIX = "Panorama addon · ";
export const NATIVE_ADDON_SUBTITLE_LOAD_TIMEOUT_MS = 15_000;

export type VideoEngineSession = {
  device: VideoEngineDevice;
  engine: VideoEngine;
  destroy(): void;
};

type VideoEngineConstructor = new () => VideoEngine;

let nextNativeExtraSubtitleToken = 1;

let videoEngineConstructorPromise: Promise<VideoEngineConstructor> | null = null;

export function normalizeMpvSubtitleColor(value: unknown): unknown {
  if (typeof value !== "string") return value;
  const hex = value.match(/^#([0-9a-f]{6})([0-9a-f]{2})?$/i);
  if (hex) return `#${hex[1]}${hex[2] ?? "ff"}`;
  const rgba = value.match(/^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)(?:\s*,\s*(\d*\.?\d+)\s*)?\)$/i);
  if (!rgba) return value;
  const channel = (entry: string) => Math.min(255, Math.max(0, Number(entry))).toString(16).padStart(2, "0");
  const alpha = Math.round(Math.min(1, Math.max(0, rgba[4] === undefined ? 1 : Number(rgba[4]))) * 255)
    .toString(16)
    .padStart(2, "0");
  return `#${channel(rgba[1])}${channel(rgba[2])}${channel(rgba[3])}${alpha}`;
}

export function loadVideoEngineConstructor(): Promise<VideoEngineConstructor> {
  if (!videoEngineConstructorPromise) {
    videoEngineConstructorPromise = import("@stremio/stremio-video")
      .then((videoModule) => videoModule.default as VideoEngineConstructor)
      .catch((error: unknown) => {
        videoEngineConstructorPromise = null;
        throw error;
      });
  }
  return videoEngineConstructorPromise;
}

export async function createVideoEngine(container: HTMLElement): Promise<VideoEngineSession> {
  const [Video, capabilities] = await Promise.all([
    loadVideoEngineConstructor(),
    getDesktopCapabilities().catch(() => null),
  ]);
  const inner = new Video();
  const nativeReady = capabilities?.nativePlayback.status === "ready" && Boolean(window.panoramaDesktop?.mpv);
  const shellPlatform = capabilities?.platform === "win32" ? "windows" : "macos";
  const windowsDirectHardwareDecode = capabilities?.platform === "win32";
  let shellTransport: DesktopShellTransport | null = nativeReady ? createDesktopShellTransport() : null;
  const device: VideoEngineDevice = shellTransport ? "ShellVideo" : "HTMLVideo";
  const preferences = readSubtitlePreferences();
  let appearance = preferences.style;
  let subtitleVerticalPosition = preferences.verticalPosition;
  const cueStore = createSubtitleCueStore();
  const subtitleRenderer = createSubtitleRenderer(container, cueStore, preferences);
  const capabilityListeners = new Set<(...args: unknown[]) => void>();
  let fallbackSelection: number | null = null;
  let browserGeneration = 0;
  let browserSeekGeneration = 0;
  let browserSequence = 0;
  const browserTracks = new Map<TextTrack, () => void>();
  const browserTrackUpdates = new Map<TextTrack, () => void>();
  let removeBrowserTrackList: (() => void) | null = null;
  let selectedBrowserEmbeddedId: string | null = null;
  let selectedBrowserExtraId: string | null = null;
  let browserVideo: HTMLVideoElement | null = null;
  let lastCapability: { mode: "custom" | "native" | "unknown"; limitation: string | null } = { mode: "unknown", limitation: null };
  const capability = (mode: "custom" | "native" | "unknown", limitation: string | null) => {
    if (lastCapability.mode === mode && lastCapability.limitation === limitation) return;
    lastCapability = { mode, limitation };
    for (const listener of capabilityListeners) listener(lastCapability);
  };
  const visibility = (visible: boolean) => shellTransport?.send("mpv-set-prop", ["sub-visibility", visible]);
  const clearCues = () => { cueStore.clear(); visibility(true); };
  const browserCue = (value: { kind: "text" | "authored" | "none"; text: string; startSeconds?: number | null; endSeconds?: number | null }) => {
    if (destroyed) return false;
    const custom = value.kind !== "authored" && new TextEncoder().encode(value.text).length <= 65536;
    const cue: NativeSubtitleCue = { playbackGeneration: browserGeneration, selectionGeneration: subtitleSelection, seekGeneration: browserSeekGeneration, sequence: ++browserSequence, trackId: null, kind: custom ? value.kind : "authored", text: custom ? value.text : "", startSeconds: value.startSeconds ?? null, endSeconds: value.endSeconds ?? null };
    cueStore.accept(cue);
    capability(custom ? "custom" : "native", custom ? null : "This track keeps its original appearance.");
    return custom;
  };
  const detachBrowser = () => {
    for (const remove of browserTracks.values()) remove();
    browserTracks.clear();
    browserTrackUpdates.clear();
    removeBrowserTrackList?.();
    removeBrowserTrackList = null;
    browserVideo = null;
  };
  const attachBrowser = () => {
    if (device !== "HTMLVideo" || destroyed) return;
    const video = container.querySelector("video");
    if (!video) return;
    if (browserVideo !== video) {
      detachBrowser(); browserVideo = video;
      const change = () => { attachBrowser(); for (const update of browserTrackUpdates.values()) update(); };
      video.textTracks.addEventListener("addtrack", change);
      video.textTracks.addEventListener("change", change);
      video.addEventListener("seeked", change);
      removeBrowserTrackList = () => { video.textTracks.removeEventListener("addtrack", change); video.textTracks.removeEventListener("change", change); video.removeEventListener("seeked", change); };
    }
    for (const track of Array.from(video.textTracks)) {
      if (track.kind !== "subtitles" && track.kind !== "captions") continue;
      if (browserTracks.has(track)) continue;
      const update = () => {
        if (track.mode === "disabled" || selectedBrowserEmbeddedId === null) return;
        const cues = Array.from(track.activeCues ?? []);
        const texts = cues.map(browserSubtitleText);
        if (texts.some((text) => text === null) || (video as HTMLVideoElement & { webkitDisplayingFullscreen?: boolean }).webkitDisplayingFullscreen === true) {
          browserCue({ kind: "authored", text: "" });
          (track as TextTrack & { panoramaCustomRendering?: boolean }).panoramaCustomRendering = false;
          track.mode = "showing";
          return;
        }
        if (browserCue({ kind: "text", text: texts.join("\n"), startSeconds: cues[0]?.startTime ?? null, endSeconds: cues.at(-1)?.endTime ?? null })) {
          (track as TextTrack & { panoramaCustomRendering?: boolean }).panoramaCustomRendering = true;
          if (track.mode !== "hidden") track.mode = "hidden";
        }
      };
      browserTrackUpdates.set(track, update);
      track.addEventListener("cuechange", update);
      browserTracks.set(track, () => { track.removeEventListener("cuechange", update); (track as TextTrack & { panoramaCustomRendering?: boolean }).panoramaCustomRendering = false; if (track.mode === "hidden") track.mode = "showing"; });
      update();
    }
  };
  inner.on("ended", clearCues);
  inner.on("error", (value) => { if (value && typeof value === "object" && (value as { critical?: boolean }).critical) clearCues(); });
  const browserObserver = device === "HTMLVideo" ? new MutationObserver(attachBrowser) : null;
  browserObserver?.observe(container, { childList: true, subtree: true });
  const nativeExtraSubtitleTracks = new Map<string, {
    id: string;
    url: string;
    lang: string;
    label: string;
    origin: string;
  }>();
  const nativeExtraSubtitleTokens = new Map<string, string>();
  const nativeExtraSubtitleIdsByToken = new Map<string, string>();
  const nativeLoadedExtraSubtitleTracks = new Map<string, string>();
  const nativeLoadingExtraSubtitleTracks = new Set<string>();
  const nativeExtraSubtitleLoadTimers = new Map<string, ReturnType<typeof setTimeout>>();
  const nativeExtraSubtitleLoadedListeners = new Set<(...args: unknown[]) => void>();
  const nativeEmbeddedSubtitleLoadedListeners = new Set<(...args: unknown[]) => void>();
  const nativeExtraSubtitleErrorListeners = new Set<(...args: unknown[]) => void>();
  const nativePlaybackActiveListeners = new Set<(...args: unknown[]) => void>();
  let selectedNativeExtraSubtitleId: string | null = null;
  let selectedNativeEmbeddedSubtitleId: string | null = null;
  let destroyed = false;
  let subtitleSelection = 0;
  let nativeLoadActive = false;
  let holdNativeStartup = false;
  let lastNativePlaybackTime: number | null = null;
  const reportNativePlaybackActive = () => {
    for (const listener of nativePlaybackActiveListeners) listener();
  };
  const clearNativeExtraSubtitleLoad = (engineId: string) => {
    nativeLoadingExtraSubtitleTracks.delete(engineId);
    const timer = nativeExtraSubtitleLoadTimers.get(engineId);
    if (timer) clearTimeout(timer);
    nativeExtraSubtitleLoadTimers.delete(engineId);
  };
  const clearNativeExtraSubtitleLoads = () => {
    for (const timer of nativeExtraSubtitleLoadTimers.values()) clearTimeout(timer);
    nativeExtraSubtitleLoadTimers.clear();
    nativeLoadingExtraSubtitleTracks.clear();
  };
  if (shellTransport) {
    shellTransport.on("mpv-event-subtitle-cue", (payload) => {
      if (destroyed) return;
      let cue = parseSubtitleCue(payload);
      if (!cue) { clearCues(); fallbackSelection = subtitleSelection; capability("native", "This track keeps its original appearance."); return; }
      const expectedTrack = selectedNativeExtraSubtitleId ? nativeLoadedExtraSubtitleTracks.get(selectedNativeExtraSubtitleId) : selectedNativeEmbeddedSubtitleId;
      if (cue.kind !== "none" && (!expectedTrack || cue.trackId !== expectedTrack)) return;
      if (cue.kind === "text" && fallbackSelection === subtitleSelection) cue = { ...cue, kind: "authored", text: "" };
      if (!cueStore.accept(cue)) return;
      if (cue.kind === "bitmap" || cue.kind === "authored") {
        fallbackSelection = subtitleSelection;
        visibility(true);
        capability("native", cue.kind === "bitmap" ? "Image subtitles keep their original appearance." : "This track keeps its original appearance.");
      } else if (cue.kind === "text" && fallbackSelection !== subtitleSelection) {
        visibility(false);
        capability("custom", null);
      } else { visibility(true); }
    });
    shellTransport.on("mpv-event-video-ready", (payload) => {
      if (destroyed || !payload || typeof payload !== "object") return;
      const event = payload as { ready?: unknown };
      if (event.ready === false) {
        nativeLoadActive = true;
        lastNativePlaybackTime = null;
      } else if (event.ready === true && nativeLoadActive) {
        reportNativePlaybackActive();
      }
    });
    shellTransport.on("mpv-prop-change", (payload) => {
      if (destroyed || !payload || typeof payload !== "object") return;
      const event = payload as { name?: unknown; data?: unknown };
      if (event.name === "sid" && selectedNativeEmbeddedSubtitleId !== null && String(event.data) === selectedNativeEmbeddedSubtitleId) {
        for (const listener of nativeEmbeddedSubtitleLoadedListeners) listener(`EMBEDDED_${selectedNativeEmbeddedSubtitleId}`);
      }
      if (event.name === "track-list" && Array.isArray(event.data)) {
        for (const entry of event.data) {
          if (!entry || typeof entry !== "object" || Array.isArray(entry)) continue;
          const track = entry as { id?: unknown; type?: unknown; title?: unknown };
          if (track.type !== "sub" || typeof track.title !== "string" || track.id == null) continue;
          if (!track.title.startsWith(NATIVE_ADDON_SUBTITLE_TITLE_PREFIX)) continue;
          const token = track.title.slice(NATIVE_ADDON_SUBTITLE_TITLE_PREFIX.length).split(" · ", 1)[0];
          const engineId = nativeExtraSubtitleIdsByToken.get(token);
          if (!engineId) continue;
          nativeLoadedExtraSubtitleTracks.set(engineId, String(track.id));
          clearNativeExtraSubtitleLoad(engineId);
        }
        if (selectedNativeExtraSubtitleId) {
          const mpvTrackId = nativeLoadedExtraSubtitleTracks.get(selectedNativeExtraSubtitleId);
          const selectedTrack = nativeExtraSubtitleTracks.get(selectedNativeExtraSubtitleId);
          if (mpvTrackId && selectedTrack) {
            shellTransport?.send("mpv-set-prop", ["sid", mpvTrackId]);
            for (const listener of nativeExtraSubtitleLoadedListeners) listener(selectedTrack);
          }
        } else if (selectedNativeEmbeddedSubtitleId) {
          shellTransport?.send("mpv-set-prop", ["sid", selectedNativeEmbeddedSubtitleId]);
        } else {
          shellTransport?.send("mpv-set-prop", ["sid", "no"]);
        }
        return;
      }
      if (!nativeLoadActive) return;
      if (event.name !== "time-pos" || typeof event.data !== "number" || !Number.isFinite(event.data)) return;
      if (lastNativePlaybackTime !== null && Math.abs(event.data - lastNativePlaybackTime) >= 0.05) {
        reportNativePlaybackActive();
      }
      lastNativePlaybackTime = event.data;
    });
  }
  const engine: VideoEngine = {
    on: (eventName, listener) => {
      if (device === "ShellVideo" && eventName === "subtitlesTrackLoaded") {
        nativeEmbeddedSubtitleLoadedListeners.add(listener);
        return;
      }
      if (eventName === "nativePlaybackActive") {
        nativePlaybackActiveListeners.add(listener);
        return;
      }
      if (device === "ShellVideo" && eventName === "extraSubtitlesTrackLoaded") {
        nativeExtraSubtitleLoadedListeners.add(listener);
        return;
      }
      if (device === "ShellVideo" && eventName === "extraSubtitlesTrackError") {
        nativeExtraSubtitleErrorListeners.add(listener);
        return;
      }
      if (eventName === "subtitleRenderingMode") { capabilityListeners.add(listener); listener(lastCapability); return; }
      inner.on(eventName, listener);
    },
    dispatch: (action, options) => {
      if (destroyed) return;
      const commandArgs = action.commandArgs && typeof action.commandArgs === "object"
        ? action.commandArgs as Record<string, unknown>
        : null;
      if (action.type === "setProp" && action.propName === "subtitleAppearance") {
        appearance = normalizeSubtitleAppearance(action.propValue);
        subtitleRenderer.updateAppearance(appearance, subtitleVerticalPosition);
        return;
      }
      if (action.type === "setProp" && action.propName === "subtitleVerticalPosition") {
        if (typeof action.propValue === "number" && Number.isFinite(action.propValue)) subtitleVerticalPosition = Math.max(0, Math.min(100, action.propValue));
        subtitleRenderer.updateAppearance(appearance, subtitleVerticalPosition);
        return;
      }
      if (action.type === "setProp" && action.propName === "time") { clearCues(); browserSeekGeneration += 1; }
      if (action.type === "setProp" && ["selectedSubtitlesTrackId", "selectedExtraSubtitlesTrackId"].includes(String(action.propName))) {
        clearCues();
        fallbackSelection = null;
        capability("unknown", null);
        if (device === "HTMLVideo") {
          subtitleSelection += 1;
          if (action.propName === "selectedSubtitlesTrackId") selectedBrowserEmbeddedId = typeof action.propValue === "string" ? action.propValue : null;
          else selectedBrowserExtraId = typeof action.propValue === "string" ? action.propValue : null;
        }
      }
      if (action.type === "command" && (action.commandName === "load" || action.commandName === "unload")) {
        clearCues();
        fallbackSelection = null;
        browserGeneration += 1;
        browserSeekGeneration = 0;
        detachBrowser();
        selectedBrowserEmbeddedId = null; selectedBrowserExtraId = null;
      }
      if (action.type === "setProp" && action.propName === "paused") holdNativeStartup = false;
      if (
        device === "ShellVideo" && action.type === "command" &&
        (action.commandName === "load" || action.commandName === "unload")
      ) {
        holdNativeStartup = action.commandName === "load" && commandArgs?.autoplay === false;
        subtitleSelection += 1;
        nativeLoadActive = false;
        lastNativePlaybackTime = null;
        nativeExtraSubtitleTracks.clear();
        nativeExtraSubtitleTokens.clear();
        nativeExtraSubtitleIdsByToken.clear();
        nativeLoadedExtraSubtitleTracks.clear();
        clearNativeExtraSubtitleLoads();
        selectedNativeExtraSubtitleId = null;
        selectedNativeEmbeddedSubtitleId = null;
      }
      if (
        device === "ShellVideo" && action.type === "command" &&
        action.commandName === "addExtraSubtitlesTracks" && Array.isArray(commandArgs?.tracks)
      ) {
        for (const entry of commandArgs.tracks) {
          if (!entry || typeof entry !== "object" || Array.isArray(entry)) continue;
          const track = entry as Record<string, unknown>;
          if (
            typeof track.id !== "string" || typeof track.url !== "string" ||
            typeof track.lang !== "string" || typeof track.label !== "string" ||
            typeof track.origin !== "string"
          ) continue;
          nativeExtraSubtitleTracks.set(track.id, {
            id: track.id,
            url: track.url,
            lang: track.lang,
            label: track.label,
            origin: track.origin,
          });
        }
        return;
      }
      let nativeAction = device === "ShellVideo" && action.type === "command" && action.commandName === "load" && commandArgs
        ? {
            ...action,
            commandArgs: {
              ...commandArgs,
              platform: shellPlatform,
              hardwareDecoding: true,
              gpuVideoProcessing: windowsDirectHardwareDecode,
              assSubtitlesStyling: true,
            },
          }
        : action;
      if (device === "ShellVideo" && action.type === "setProp") {
        const propName = String(action.propName ?? "");
        if (propName === "selectedSubtitlesTrackId" || propName === "selectedExtraSubtitlesTrackId") subtitleSelection += 1;
        if (propName === "selectedSubtitlesTrackId") {
          selectedNativeExtraSubtitleId = null;
          selectedNativeEmbeddedSubtitleId = typeof action.propValue === "string"
            ? action.propValue.replace(/^EMBEDDED_/, "")
            : null;
        }
        if (propName === "selectedExtraSubtitlesTrackId") {
          if (action.propValue == null) {
            selectedNativeEmbeddedSubtitleId = null;
            selectedNativeExtraSubtitleId = null;
            shellTransport?.send("mpv-set-prop", ["sid", "no"]);
            return;
          }
          const engineId = String(action.propValue);
          selectedNativeEmbeddedSubtitleId = null;
          selectedNativeExtraSubtitleId = engineId;
          const track = nativeExtraSubtitleTracks.get(engineId);
          if (!track) return;
          const loadedTrackId = nativeLoadedExtraSubtitleTracks.get(engineId);
          if (loadedTrackId) {
            shellTransport?.send("mpv-set-prop", ["sid", loadedTrackId]);
            const selection = subtitleSelection;
            queueMicrotask(() => {
              if (destroyed || selection !== subtitleSelection) return;
              for (const listener of nativeExtraSubtitleLoadedListeners) listener(track);
            });
            return;
          }
          if (nativeLoadingExtraSubtitleTracks.has(engineId)) return;
          const label = track.label.replace(/[\u0000-\u001f\u007f]/g, " ").trim() || track.lang;
          const language = track.lang.replace(/[\u0000-\u001f\u007f]/g, "").trim() || "und";
          const previousToken = nativeExtraSubtitleTokens.get(engineId);
          if (previousToken) nativeExtraSubtitleIdsByToken.delete(previousToken);
          const token = `p${nextNativeExtraSubtitleToken++}`;
          nativeExtraSubtitleTokens.set(engineId, token);
          nativeExtraSubtitleIdsByToken.set(token, engineId);
          nativeLoadingExtraSubtitleTracks.add(engineId);
          const titlePrefix = `${NATIVE_ADDON_SUBTITLE_TITLE_PREFIX}${token} · `;
          shellTransport?.send("mpv-command", [
            "sub-add",
            track.url,
            "cached",
            `${titlePrefix}${label}`.slice(0, 240),
            language.slice(0, 32),
          ]);
          nativeExtraSubtitleLoadTimers.set(engineId, setTimeout(() => {
            clearNativeExtraSubtitleLoad(engineId);
            nativeExtraSubtitleIdsByToken.delete(token);
            nativeExtraSubtitleTokens.delete(engineId);
            if (selectedNativeExtraSubtitleId !== engineId || nativeLoadedExtraSubtitleTracks.has(engineId)) return;
            for (const listener of nativeExtraSubtitleErrorListeners) {
              listener({ critical: false, trackId: engineId, message: "Subtitle loading timed out." });
            }
          }, NATIVE_ADDON_SUBTITLE_LOAD_TIMEOUT_MS));
          return;
        }
        if (propName.startsWith("extraSubtitles")) return;
        if (propName === "subtitlesPaddingX" || propName === "subtitlesPaddingY") return;
        if (propName === "subtitlesFontWeight") {
          const weight = String(action.propValue ?? "medium");
          const medium = weight === "medium" || weight === "semibold";
          shellTransport?.send("mpv-set-prop", ["sub-font", medium ? "DM Sans Medium" : "DM Sans"]);
          shellTransport?.send("mpv-set-prop", ["sub-bold", weight === "bold"]);
          return;
        }
        const colorProp = ["subtitlesTextColor", "subtitlesBackgroundColor", "subtitlesOutlineColor"].includes(propName);
        nativeAction = {
          ...action,
          propValue: colorProp
            ? normalizeMpvSubtitleColor(action.propValue)
            : propName === "subtitlesDelay" && typeof action.propValue === "number"
              ? action.propValue / 1000
              : action.propValue,
        };
      }
      if (device === "HTMLVideo" && action.type === "setProp" && ["selectedSubtitlesTrackId", "selectedExtraSubtitlesTrackId"].includes(String(action.propName))) {
        queueMicrotask(() => { if (destroyed) return; attachBrowser(); for (const update of browserTrackUpdates.values()) update(); });
      }
      inner.dispatch(nativeAction, {
        ...options,
        containerElement: container,
        onSubtitleCue: (cue: Parameters<typeof browserCue>[0]) => selectedBrowserExtraId !== null ? browserCue(cue) : false,
        ...(shellTransport ? { shellTransport: {
          ...shellTransport,
          send: (channel, payload) => shellTransport?.send(channel,
            holdNativeStartup && channel === "mpv-set-prop" && Array.isArray(payload) && payload[0] === "pause"
              ? ["pause", true] : payload),
        } satisfies DesktopShellTransport, mpvSeparateWindow: false } : {}),
      });
    },
    destroy: () => { clearCues(); inner.destroy(); },
  };
  return {
    device,
    engine,
    destroy() {
      if (destroyed) return;
      destroyed = true;
      clearCues();
      capabilityListeners.clear();
      browserObserver?.disconnect();
      detachBrowser();
      subtitleRenderer.destroy();
      nativePlaybackActiveListeners.clear();
      nativeExtraSubtitleLoadedListeners.clear();
      nativeEmbeddedSubtitleLoadedListeners.clear();
      nativeExtraSubtitleErrorListeners.clear();
      clearNativeExtraSubtitleLoads();
      nativeExtraSubtitleTokens.clear();
      nativeExtraSubtitleIdsByToken.clear();
      nativeLoadedExtraSubtitleTracks.clear();
      nativeExtraSubtitleTracks.clear();
      engine.destroy();
      shellTransport?.destroy();
      shellTransport = null;
    },
  };
}
