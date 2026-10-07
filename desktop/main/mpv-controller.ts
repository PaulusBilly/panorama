import { parseSubtitleCue } from "../shared/subtitle-cue";
import { PlaybackSettingsController } from "./playback-settings";
import type {
  DesktopPlaybackDiagnostics,
  NativePlaybackCapability,
  PlaybackRendererTimingEvent,
  PlaybackTimingEvent,
} from "../shared/desktop-api";
import {
  parseShellSend,
  parseVideoSurface,
  type ShellEventChannel,
  type ShellSendChannel,
  type VideoSurfaceBounds,
} from "../shared/mpv-protocol";
import type { MpvHost } from "../native/mpv-host/mpv-host";
import type { MpvHostDiagnostics } from "../native/mpv-host/types";
import type { MediaProxyStats } from "./media-proxy";

type EventSink = (channel: ShellEventChannel, payload: unknown) => void;

export type MediaSourceProxy = {
  open(url: string): string;
  close(): void;
  stats(): MediaProxyStats;
  setReadAhead(aheadSeconds: number | null, buffering: boolean, context?: { sourceMbps: number | null; paused: boolean; playbackSpeed: number }): void;
};

function finiteOrNull(value: number | null | undefined): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function counterDelta(value: number | null | undefined, baseline: number): number {
  const finiteValue = finiteOrNull(value);
  return finiteValue === null ? 0 : Math.max(0, finiteValue - baseline);
}

function propertyValue(name: string, diagnostics: MpvHostDiagnostics): unknown {
  if (name === "path") return diagnostics.path;
  if (name === "time-pos") return diagnostics.timeSeconds;
  if (name === "duration") return diagnostics.durationSeconds;
  if (name === "volume") return diagnostics.volume;
  if (name === "pause") return diagnostics.paused;
  if (name === "seeking") return diagnostics.seeking;
  if (name === "eof-reached") return diagnostics.eofReached;
  if (name === "metadata") return {};
  if (name === "video-params") {
    return diagnostics.videoWidth && diagnostics.videoHeight
      ? { w: diagnostics.videoWidth, h: diagnostics.videoHeight }
      : null;
  }
  if (name === "track-list") {
    return diagnostics.tracks.map((track) => ({
      id: track.id,
      type: track.type,
      lang: track.language ?? undefined,
      title: track.title ?? undefined,
      selected: track.selected,
      forced: track.forced,
      "hearing-impaired": track.hearingImpaired,
      codec: track.codec ?? undefined,
    }));
  }
  if (name === "paused-for-cache") return diagnostics.buffering;
  if (name === "cache-buffering-state") return finiteOrNull(diagnostics.cacheBufferingPercent);
  if (name === "demuxer-cache-time") return finiteOrNull(diagnostics.cacheEndSeconds);
  if (name === "aid") return diagnostics.selectedAudioId;
  if (name === "vid") return diagnostics.selectedVideoId;
  if (name === "sid") return diagnostics.selectedSubtitleId;
  if (name === "sub-scale") return diagnostics.subtitleScale;
  if (name === "sub-pos") return diagnostics.subtitlePosition;
  if (name === "sub-delay") return diagnostics.subtitleDelay;
  if (name === "speed") return diagnostics.speed;
  if (name === "mpv-version") return diagnostics.mpvVersion;
  if (name === "ffmpeg-version") return diagnostics.ffmpegVersion;
  return null;
}

const rendererTimingEvents = new Set<PlaybackRendererTimingEvent>([
  "play-requested",
  "preparation-ready",
  "video-load-requested",
  "seek-requested",
]);

const POLL_FAILURE_LIMIT = 5;
const RELOAD_DELAYS_MS = [1_000, 3_000];
const TRUNCATED_EOF_MARGIN_SECONDS = 30;

function sourceKind(url: string): "loopback-service" | "remote" {
  return new URL(url).hostname === "127.0.0.1" ? "loopback-service" : "remote";
}

export class MpvController {
  private readonly settings: PlaybackSettingsController;
  private readonly observed = new Set<string>();
  private readonly lastValues = new Map<string, string>();
  private timer: ReturnType<typeof setInterval> | null = null;
  private destroyed = false;
  private playbackActive = false;
  private loadId = 0;
  private videoReady = false;
  private expectedLoadGeneration = 0;
  private awaitingMovement = false;
  private endedLoadId = 0;
  private lastEndSequence = 0;
  private surface: VideoSurfaceBounds = { visible: false, x: 0, y: 0, width: 0, height: 0, scaleFactor: 1 };
  private surfaceSuspended = false;
  private timingOrigin: number | null = null;
  private timingMarks = new Map<PlaybackTimingEvent, number>();
  private timeline: DesktopPlaybackDiagnostics["timeline"] = [];
  private playbackSourceKind: DesktopPlaybackDiagnostics["sourceKind"] = null;
  private pendingSeekGeneration: number | null = null;
  private pendingSeekDeadline: number | null = null;
  private latestDiagnostics: MpvHostDiagnostics | null = null;
  private rebufferStarted: number | null = null;
  private rebufferCount = 0;
  private rebufferMilliseconds = 0;
  private pollFailures = 0;
  private loadUrl: string | null = null;
  private sourceUrl: string | null = null;
  private lastPlaybackSeconds = 0;
  private reloadAttempts = 0;
  private reloadTimer: ReturnType<typeof setTimeout> | null = null;
  private reloadIssuedAt: number | null = null;
  private lastLogAt = 0;
  private diagnosticsBaseline = {
    frameDropCount: 0,
    decoderFrameDropCount: 0,
    mistimedFrameCount: 0,
    delayedFrameCount: 0,
    renderUpdates: 0,
    renderedFrames: 0,
    reportedSwaps: 0,
  };

  private unsubscribeSubtitleCue: (() => void) | null = null;

  constructor(
    private readonly host: MpvHost,
    private readonly emit: EventSink,
    pollIntervalMs = 100,
    private readonly now = () => Date.now(),
    private readonly setPlaybackActive: (active: boolean) => void = () => undefined,
    preferencesFile?: string,
    private readonly playbackLog?: (line: string) => void,
    private readonly mediaProxy?: MediaSourceProxy,
  ) {
    this.settings = new PlaybackSettingsController(host, preferencesFile);
    this.unsubscribeSubtitleCue = host.onSubtitleCue?.((value) => {
      if (this.destroyed) return;
      const cue = parseSubtitleCue(value);
      if (!cue) {
        this.host.dispatch({ type: "setProperty", name: "sub-visibility", value: true });
      }
      this.emit("mpv-event-subtitle-cue", cue);
    }) ?? null;
    this.timer = setInterval(() => this.pollSafely(), pollIntervalMs);
  }

  async waitUntilReady(timeoutMs = 2_000): Promise<NativePlaybackCapability> {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() <= deadline) {
      const version = this.readDiagnostics().mpvVersion;
      if (version) {
        this.settings.initialize(this.readDiagnostics());
        return { status: "ready", device: "ShellVideo", mpvVersion: version };
      }
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    return { status: "unavailable", reason: "initialization-failed" };
  }

  getPlaybackSettings() {
    this.settings.poll(this.readDiagnostics());
    return this.settings.getState();
  }

  setPlaybackSettings(value: unknown) {
    this.settings.poll(this.readDiagnostics());
    return this.settings.set(value);
  }

  getPlaybackDiagnostics(): DesktopPlaybackDiagnostics {
    const diagnostics = this.readDiagnostics();
    const duration = (from: PlaybackTimingEvent, to: PlaybackTimingEvent): number | null => {
      const start = this.timingMarks.get(from);
      const end = this.timingMarks.get(to);
      return start === undefined || end === undefined ? null : Math.max(0, end - start);
    };
    return {
      mpvVersion: diagnostics.mpvVersion,
      videoCodec: diagnostics.videoCodec,
      hardwareDecoder: diagnostics.hardwareDecoder,
      rendererBackend: ["opengl", "macvk", "d3d11"].includes(diagnostics.rendererBackend ?? "")
        ? diagnostics.rendererBackend as "opengl" | "macvk" | "d3d11" : null,
      rendererFallbackReason: diagnostics.rendererFallbackReason === "initialization-failed" ? "initialization-failed" : null,
      audioOutputFormat: diagnostics.audioOutputFormat ?? null,
      audioOutputDriver: diagnostics.audioOutputDriver ?? null,
      videoOutputPrimaries: diagnostics.videoOutputPrimaries ?? null,
      videoOutputTransferFunction: diagnostics.videoOutputTransferFunction ?? null,
      buffering: diagnostics.buffering,
      ownedEntryId: diagnostics.ownedEntryId ?? null,
      loadGeneration: diagnostics.loadGeneration,
      fileStarted: diagnostics.fileStarted ?? false,
      fileLoaded: diagnostics.fileLoaded ?? false,
      decodedReady: diagnostics.decodedReady ?? false,
      presented: diagnostics.presented ?? false,
      presentationEvidence: diagnostics.presentationEvidence,
      moving: diagnostics.moving ?? false,
      seekGeneration: diagnostics.seekGeneration,
      restartedSeekGeneration: diagnostics.restartedSeekGeneration,
      presentedSeekGeneration: diagnostics.presentedSeekGeneration,
      rebufferCount: this.rebufferCount,
      rebufferMilliseconds: this.rebufferMilliseconds + (this.rebufferStarted === null ? 0 : Math.max(0, this.now() - this.rebufferStarted)),
      cacheSeconds: finiteOrNull(diagnostics.cacheSeconds),
      cacheEndSeconds: finiteOrNull(diagnostics.cacheEndSeconds),
      cacheBufferingPercent: finiteOrNull(diagnostics.cacheBufferingPercent),
      cacheForwardBytes: finiteOrNull(diagnostics.cacheForwardBytes),
      inputBytesPerSecond: finiteOrNull(diagnostics.inputBytesPerSecond),
      ...this.throughput(diagnostics),
      sourceUnreachable: this.sourceUnreachable(),
      proxy: this.mediaProxy?.stats() ?? null,
      resumeBufferSeconds: this.mediaProxy?.stats().resumeBufferSeconds ?? 5,
      audioSampleRate: finiteOrNull(diagnostics.audioSampleRate),
      audioOutputSampleRate: finiteOrNull(diagnostics.audioOutputSampleRate),
      audioCodec: diagnostics.audioCodec ?? null,
      audioChannels: diagnostics.audioChannels ?? null,
      audioOutputChannels: diagnostics.audioOutputChannels ?? null,
      videoPixelFormat: diagnostics.videoPixelFormat ?? null,
      videoColorPrimaries: diagnostics.videoColorPrimaries ?? null,
      videoTransferFunction: diagnostics.videoTransferFunction ?? null,
      sourceFps: finiteOrNull(diagnostics.sourceFps),
      displayFps: finiteOrNull(diagnostics.displayFps),
      frameDropCount: counterDelta(diagnostics.frameDropCount, this.diagnosticsBaseline.frameDropCount),
      decoderFrameDropCount: counterDelta(diagnostics.decoderFrameDropCount, this.diagnosticsBaseline.decoderFrameDropCount),
      mistimedFrameCount: counterDelta(diagnostics.mistimedFrameCount, this.diagnosticsBaseline.mistimedFrameCount),
      delayedFrameCount: counterDelta(diagnostics.delayedFrameCount, this.diagnosticsBaseline.delayedFrameCount),
      videoWidth: finiteOrNull(diagnostics.videoWidth),
      videoHeight: finiteOrNull(diagnostics.videoHeight),
      renderUpdates: finiteOrNull(diagnostics.renderUpdates) === null ? null : counterDelta(diagnostics.renderUpdates, this.diagnosticsBaseline.renderUpdates),
      renderedFrames: finiteOrNull(diagnostics.renderedFrames) === null ? null : counterDelta(diagnostics.renderedFrames, this.diagnosticsBaseline.renderedFrames),
      reportedSwaps: finiteOrNull(diagnostics.reportedSwaps) === null ? null : counterDelta(diagnostics.reportedSwaps, this.diagnosticsBaseline.reportedSwaps),
      sourceKind: this.playbackSourceKind,
      startupTiming: {
        totalMs: duration("play-requested", "first-frame"),
        preparationMs: duration("play-requested", "preparation-ready"),
        videoDispatchMs: duration("video-load-requested", "loadfile-issued"),
        nativeFirstFrameMs: duration("loadfile-issued", "first-frame"),
      },
      lastSeekToFirstFrameMs: duration("seek-requested", "seek-first-frame"),
      timeline: [...this.timeline],
    };
  }

  recordRendererTiming(event: PlaybackRendererTimingEvent): void {
    if (!rendererTimingEvents.has(event)) throw new Error("Unsupported playback timing event");
    if (event === "play-requested") {
      this.timingOrigin = this.now();
      this.timingMarks.clear();
      this.timeline = [];
      this.playbackSourceKind = null;
      this.pendingSeekGeneration = null;
      this.pendingSeekDeadline = null;
    }
    this.recordTiming(event);
  }

  handleSend(channel: ShellSendChannel, payload: unknown): void {
    this.assertActive();
    // Only a failed media load is fatal. Property, observer, subtitle and stop
    // failures must never surface as an unplayable source.
    const fatal = channel === "mpv-command" && Array.isArray(payload) && payload[0] === "loadfile";
    try {
      this.applySend(channel, payload);
    } catch (error) {
      if (fatal) throw error;
    }
  }

  private applySend(channel: ShellSendChannel, payload: unknown): void {
    const message = parseShellSend(channel, payload);
    if (message.channel === "mpv-observe-prop") {
      const name = message.payload as string;
      const data = propertyValue(name, this.latestDiagnostics ?? this.readDiagnostics());
      this.observed.add(name);
      this.lastValues.set(name, JSON.stringify(data));
      this.emit("mpv-prop-change", { name, data });
      return;
    }
    if (message.channel === "mpv-command") {
      let args = message.payload as string[];
      if (args[0] === "loadfile" || args[0] === "stop") this.cancelReload();
      if (args[0] === "stop") this.mediaProxy?.close();
      if (args[0] === "loadfile") {
        this.sourceUrl = args[1];
        if (this.mediaProxy && sourceKind(args[1]) === "remote") {
          try {
            args = [args[0], this.mediaProxy.open(args[1]), ...args.slice(2)];
          } catch {}
        } else {
          this.mediaProxy?.close();
        }
        this.loadUrl = args[1];
        this.lastPlaybackSeconds = Number(args.find((entry) => entry.startsWith("start=+"))?.slice(7) ?? 0) || 0;
        this.reloadAttempts = 0;
        this.reloadIssuedAt = null;
        this.pollFailures = 0;
        this.settings.resetSource();
        if (this.timingMarks.has("loadfile-issued")) {
          this.timingMarks.clear();
          this.timeline = [];
          this.timingOrigin = this.now();
        }
        this.pendingSeekGeneration = null;
        this.pendingSeekDeadline = null;
        this.rebufferStarted = null;
        this.rebufferCount = 0;
        this.rebufferMilliseconds = 0;
        const diagnostics = this.readDiagnostics();
        this.lastEndSequence = Math.max(this.lastEndSequence, diagnostics.endSequence ?? 0);
        this.diagnosticsBaseline = {
          frameDropCount: finiteOrNull(diagnostics.frameDropCount) ?? 0,
          decoderFrameDropCount: finiteOrNull(diagnostics.decoderFrameDropCount) ?? 0,
          mistimedFrameCount: finiteOrNull(diagnostics.mistimedFrameCount) ?? 0,
          delayedFrameCount: finiteOrNull(diagnostics.delayedFrameCount) ?? 0,
          renderUpdates: finiteOrNull(diagnostics.renderUpdates) ?? 0,
          renderedFrames: finiteOrNull(diagnostics.renderedFrames) ?? 0,
          reportedSwaps: finiteOrNull(diagnostics.reportedSwaps) ?? 0,
        };
        this.loadId += 1;
        this.endedLoadId = 0;
        this.videoReady = false;
        this.expectedLoadGeneration = (diagnostics.loadGeneration ?? 0) + 1;
        this.awaitingMovement = true;
        this.playbackSourceKind = sourceKind(this.sourceUrl);
        this.recordTiming("loadfile-issued");
        this.emit("mpv-event-video-ready", { loadId: this.loadId, ready: false });
      }
      if (args[0] === "loadfile") this.host.setPlaybackProperty("cache-pause-wait", 2);
      this.host.dispatch({ type: "shellCommand", args });
      if (args[0] === "loadfile" || args[0] === "stop") this.latestDiagnostics = null;
      if (args[0] === "stop") {
        this.endedLoadId = this.loadId;
        this.updatePlaybackActive(false);
      }
      return;
    }
    if (message.channel === "mpv-set-prop") {
      const [name, value] = message.payload as [string, string | number | boolean | null];
      if (name === "osc" && (value === "no" || value === false)) return;
      // The native host owns renderer and decoder selection; ShellVideo's generic
      // requests (e.g. hwdec=auto-copy) would force slow copy-back decoding.
      if (name === "vo" || name === "hwdec") return;
      if (name === "time-pos" && typeof value === "number") {
        const seconds = Number.isFinite(value) ? Math.max(0, value) : 0;
        const diagnostics = this.readDiagnostics();
        this.pendingSeekGeneration = (diagnostics.seekGeneration ?? 0) + 1;
        this.pendingSeekDeadline = this.now() + 10_000;
        this.awaitingMovement = true;
        this.host.setPlaybackProperty("cache-pause-wait", 2);
        for (const event of ["seek-first-frame", "seek-restart", "seek-cache-exit", "seek-moving"] as const) this.timingMarks.delete(event);
        this.recordTiming("seek-dispatched");
        this.lastPlaybackSeconds = seconds;
        this.host.dispatch({ type: "seek", seconds });
        return;
      }
      if (name === "speed" && typeof value === "number") this.settings.setSpeed(value);
      this.host.dispatch({ type: "setProperty", name, value: name === "volume" && typeof value === "number" ? this.settings.volume(value) : value });
      return;
    }
    this.host.dispatch({ type: "setGpuVideoProcessing", enabled: message.payload as boolean });
  }

  setVideoSurface(value: unknown): void {
    this.assertActive();
    this.surface = parseVideoSurface(value);
    this.applySurface();
  }

  suspendSurface(suspended: boolean): void {
    this.surfaceSuspended = suspended;
    this.applySurface();
  }

  refreshSurface(): void {
    this.applySurface();
  }

  poll(): void {
    if (this.destroyed) return;
    const diagnostics = this.readDiagnostics();
    try {
      this.settings.poll(diagnostics);
    } catch {}
    if (
      diagnostics.seeking !== true && this.reloadTimer === null &&
      typeof diagnostics.timeSeconds === "number" && Number.isFinite(diagnostics.timeSeconds) && diagnostics.timeSeconds > 0
    ) {
      this.lastPlaybackSeconds = diagnostics.timeSeconds;
    }
    for (const name of this.observed) {
      const data = propertyValue(name, diagnostics);
      const serialized = JSON.stringify(data);
      if (this.lastValues.get(name) === serialized) continue;
      this.lastValues.set(name, serialized);
      this.emit("mpv-prop-change", { name, data });
    }
    const owned = this.loadId > 0 && (diagnostics.loadGeneration ?? 0) >= this.expectedLoadGeneration &&
      diagnostics.ownedEntryId != null && diagnostics.fileStarted === true;
    const seek = this.pendingSeekGeneration;
    const restarted = owned && diagnostics.decodedReady === true &&
      (seek === null || diagnostics.restartedSeekGeneration === seek);
    if (restarted) {
      const restartEvent = seek === null ? "playback-restart" : "seek-restart";
      if (!this.timingMarks.has(restartEvent)) this.recordTiming(restartEvent);
      const cacheEvent = this.timingMarks.has("seek-dispatched") ? "seek-cache-exit" : "cache-exit";
      if (!diagnostics.buffering && !this.timingMarks.has(cacheEvent)) this.recordTiming(cacheEvent);
    }
    if (restarted && diagnostics.presented === true && diagnostics.renderReady &&
        Boolean(diagnostics.videoWidth && diagnostics.videoHeight) && !this.videoReady) {
      this.videoReady = true;
      this.recordTiming("first-frame");
      this.logStartup();
      this.emit("mpv-event-video-ready", { loadId: this.loadId, ready: true });
    }
    if (seek !== null && restarted && diagnostics.presented === true && diagnostics.presentedSeekGeneration === seek) {
      this.pendingSeekGeneration = null;
      this.pendingSeekDeadline = null;
      this.recordTiming("seek-first-frame");
    }
    if (this.pendingSeekGeneration !== null && this.pendingSeekDeadline !== null &&
        this.now() >= this.pendingSeekDeadline && diagnostics.seeking !== true &&
        diagnostics.restartedSeekGeneration !== this.pendingSeekGeneration) {
      this.pendingSeekGeneration = null;
      this.pendingSeekDeadline = null;
      if (this.awaitingMovement) {
        this.awaitingMovement = false;
        this.host.setPlaybackProperty("cache-pause-wait", this.mediaProxy?.stats().resumeBufferSeconds ?? 5);
      }
    }
    if (owned && diagnostics.moving === true && !diagnostics.buffering && diagnostics.paused === false &&
        diagnostics.seeking !== true && this.pendingSeekGeneration === null && this.awaitingMovement) {
      this.awaitingMovement = false;
      this.host.setPlaybackProperty("cache-pause-wait", this.mediaProxy?.stats().resumeBufferSeconds ?? 5);
      this.recordTiming(this.timingMarks.has("seek-dispatched") ? "seek-moving" : "playback-moving");
    }
    const rebuffering = this.videoReady && !this.awaitingMovement &&
      diagnostics.buffering && diagnostics.paused === false && diagnostics.seeking !== true &&
      this.pendingSeekGeneration === null && this.endedLoadId !== this.loadId;
    if (rebuffering && this.rebufferStarted === null) {
      this.rebufferStarted = this.now();
      this.rebufferCount += 1;
    } else if (!rebuffering && this.rebufferStarted !== null) {
      this.rebufferMilliseconds += Math.max(0, this.now() - this.rebufferStarted);
      this.rebufferStarted = null;
    }
    if (this.mediaProxy && this.loadUrl !== this.sourceUrl) {
      const { sourceBitrateMbps } = this.throughput(diagnostics);
      this.mediaProxy.setReadAhead(finiteOrNull(diagnostics.cacheSeconds), diagnostics.buffering, { sourceMbps: sourceBitrateMbps, paused: diagnostics.paused ?? false, playbackSpeed: diagnostics.speed ?? 1 });
    }
    this.logSample(diagnostics);
    const endSequence = diagnostics.endSequence ?? 0;
    if (endSequence > this.lastEndSequence) {
      this.lastEndSequence = endSequence;
      if (
        diagnostics.endReason && diagnostics.endReason !== "stop" && diagnostics.endReason !== "quit" &&
        this.reloadTimer === null && !this.scheduleReload(diagnostics)
      ) {
        this.endedLoadId = this.loadId;
        this.emit("mpv-event-ended", diagnostics.endError
          ? { reason: diagnostics.endReason ?? "error", error: { critical: true, message: diagnostics.endError } }
          : { reason: diagnostics.endReason ?? "other" });
      }
    } else if (
      diagnostics.eofReached && this.endedLoadId !== this.loadId && this.reloadTimer === null &&
      (this.reloadIssuedAt === null || this.now() - this.reloadIssuedAt > 5_000) &&
      !this.scheduleReload(diagnostics)
    ) {
      this.endedLoadId = this.loadId;
      this.emit("mpv-event-ended", { reason: "eof" });
    }
    this.updatePlaybackActive(
      this.loadId > 0 && this.endedLoadId !== this.loadId && diagnostics.paused === false,
    );
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.cancelReload();
    this.mediaProxy?.close();
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    this.updatePlaybackActive(false);
    this.unsubscribeSubtitleCue?.();
    this.unsubscribeSubtitleCue = null;
    this.host.destroy();
  }

  private applySurface(): void {
    if (this.destroyed) return;
    const value = this.surfaceSuspended ? { ...this.surface, visible: false } : this.surface;
    this.host.dispatch({
      type: "setBounds",
      x: value.visible ? value.x : 0,
      y: value.visible ? value.y : 0,
      width: value.visible ? value.width : 0,
      height: value.visible ? value.height : 0,
      scaleFactor: value.scaleFactor,
    });
  }

  private pollSafely(): void {
    try {
      this.poll();
      this.pollFailures = 0;
    } catch {
      // Transient native read failures must not permanently stop the controller;
      // only a persistent failure is reported, and only for the active load.
      this.pollFailures += 1;
      if (this.pollFailures < POLL_FAILURE_LIMIT || this.loadId === 0 || this.endedLoadId === this.loadId) return;
      this.endedLoadId = this.loadId;
      this.cancelReload();
      this.updatePlaybackActive(false);
      this.emit("mpv-event-ended", {
        reason: "error",
        error: { critical: true, message: "Native player stopped." },
      });
    }
  }

  // Reloads the current source at the last position after a network error or a
  // premature end of stream. Returns false when the end should be reported.
  private scheduleReload(diagnostics: MpvHostDiagnostics): boolean {
    let url = this.loadUrl;
    if (this.mediaProxy?.stats().representationChanged && this.sourceUrl) {
      url = this.mediaProxy.open(this.sourceUrl);
      this.loadUrl = url;
    }
    if (!url || this.reloadAttempts >= RELOAD_DELAYS_MS.length) return false;
    // Reloading cannot help while the source's server refuses connections.
    if (this.sourceUnreachable()) return false;
    const duration = finiteOrNull(diagnostics.durationSeconds);
    const failed = diagnostics.endReason === "error" || diagnostics.endReason === "other";
    const truncated = !failed && duration !== null &&
      this.lastPlaybackSeconds < duration - TRUNCATED_EOF_MARGIN_SECONDS;
    if (!failed && !truncated) return false;
    const delay = RELOAD_DELAYS_MS[this.reloadAttempts];
    this.reloadAttempts += 1;
    const loadId = this.loadId;
    this.reloadTimer = setTimeout(() => {
      this.reloadTimer = null;
      if (this.destroyed || loadId !== this.loadId || this.endedLoadId === loadId) return;
      const start = Math.max(0, Math.floor(this.lastPlaybackSeconds));
      try {
        this.expectedLoadGeneration = (this.readDiagnostics().loadGeneration ?? 0) + 1;
        this.awaitingMovement = true;
        this.pendingSeekGeneration = null;
        this.pendingSeekDeadline = null;
        this.host.setPlaybackProperty("cache-pause-wait", 2);
        this.host.dispatch({ type: "shellCommand", args: ["loadfile", url, "replace", "-1", `start=+${start}`] });
        this.reloadIssuedAt = this.now();
        this.latestDiagnostics = null;
        this.lastEndSequence = Math.max(this.lastEndSequence, this.readDiagnostics().endSequence ?? 0);
      } catch {
        this.endedLoadId = loadId;
        this.emit("mpv-event-ended", { reason: "error", error: { critical: true, message: "Unable to reload media." } });
      }
    }, delay);
    return true;
  }

  private sourceUnreachable(): boolean {
    return Boolean(this.sourceUrl && this.loadUrl !== this.sourceUrl && this.mediaProxy?.stats().unreachable);
  }

  // Download rate prefers the parallel proxy's measurement; source bitrate is
  // the file's average (size over duration) when the proxy knows the size.
  private throughput(diagnostics: MpvHostDiagnostics): { downloadMbps: number | null; sourceBitrateMbps: number | null } {
    const stats = this.sourceUrl && this.loadUrl !== this.sourceUrl ? this.mediaProxy?.stats() : undefined;
    const speed = finiteOrNull(diagnostics.inputBytesPerSecond);
    const duration = finiteOrNull(diagnostics.durationSeconds);
    return {
      downloadMbps: stats?.downloadMbps ?? (speed === null ? null : speed * 8 / 1_000_000),
      sourceBitrateMbps: stats?.sizeBytes && duration && duration > 0 ? stats.sizeBytes * 8 / duration / 1_000_000 : null,
    };
  }

  private logStartup(): void {
    if (!this.playbackLog) return;
    try {
      const { startupTiming, timeline } = this.getPlaybackDiagnostics();
      this.playbackLog(JSON.stringify({ at: new Date(this.now()).toISOString(), load: this.loadId, startup: startupTiming, timeline }));
    } catch {}
  }

  // One local sample per second while media is loaded, for diagnosing
  // throughput and stalls. Records only the source host, never the URL.
  private logSample(diagnostics: MpvHostDiagnostics): void {
    if (!this.playbackLog || this.loadId === 0 || this.endedLoadId === this.loadId) return;
    const now = this.now();
    if (now - this.lastLogAt < 1_000) return;
    this.lastLogAt = now;
    const round = (value: number | null, digits = 1) => value === null ? null : Number(value.toFixed(digits));
    const cacheSeconds = finiteOrNull(diagnostics.cacheSeconds);
    const cacheBytes = finiteOrNull(diagnostics.cacheForwardBytes);
    const speed = finiteOrNull(diagnostics.inputBytesPerSecond);
    let host: string | null = null;
    try { host = this.sourceUrl ? new URL(this.sourceUrl).host : null; } catch {}
    const { downloadMbps, sourceBitrateMbps } = this.throughput(diagnostics);
    try {
      this.playbackLog(JSON.stringify({
        at: new Date(now).toISOString(),
        load: this.loadId,
        host,
        pos: round(finiteOrNull(diagnostics.timeSeconds)),
        dur: round(finiteOrNull(diagnostics.durationSeconds), 0),
        paused: diagnostics.paused,
        seeking: diagnostics.seeking,
        buffering: diagnostics.buffering,
        cacheSec: round(cacheSeconds),
        cacheMB: cacheBytes === null ? null : round(cacheBytes / 1_048_576),
        netMbps: speed === null ? null : round(speed * 8 / 1_000_000),
        downloadMbps: round(downloadMbps),
        sourceMbps: round(sourceBitrateMbps),
        mediaMbps: cacheBytes !== null && cacheSeconds !== null && cacheSeconds > 1 ? round(cacheBytes * 8 / cacheSeconds / 1_000_000) : null,
        proxy: this.mediaProxy?.stats() ?? null,
        codec: diagnostics.videoCodec,
        hwdec: diagnostics.hardwareDecoder,
        res: diagnostics.videoWidth && diagnostics.videoHeight ? `${diagnostics.videoWidth}x${diagnostics.videoHeight}` : null,
        drops: finiteOrNull(diagnostics.frameDropCount),
        decoderDrops: finiteOrNull(diagnostics.decoderFrameDropCount),
        rebuffers: this.rebufferCount,
        reloads: this.reloadAttempts,
        aid: diagnostics.selectedAudioId ?? null,
        audioTracks: diagnostics.tracks.filter((track) => track.type === "audio").length,
        acodec: diagnostics.audioCodec ?? null,
        achannels: diagnostics.audioChannels ?? null,
        aout: diagnostics.audioOutputFormat ?? null,
        ao: diagnostics.audioOutputDriver ?? null,
        aoErrors: diagnostics.audioOutputErrorSequence ?? 0,
        volume: finiteOrNull(diagnostics.volume),
      }));
    } catch {}
  }

  private cancelReload(): void {
    if (this.reloadTimer) clearTimeout(this.reloadTimer);
    this.reloadTimer = null;
  }

  private assertActive(): void {
    if (this.destroyed) throw new Error("MPV controller is destroyed");
  }

  private updatePlaybackActive(active: boolean): void {
    if (active === this.playbackActive) return;
    this.playbackActive = active;
    this.setPlaybackActive(active);
  }

  private recordTiming(event: PlaybackTimingEvent): void {
    if (event === "seek-requested") this.timingMarks.delete("seek-first-frame");
    const absolute = this.now();
    if (this.timingOrigin === null) this.timingOrigin = absolute;
    const elapsedMs = Math.max(0, Math.round(absolute - this.timingOrigin));
    this.timingMarks.set(event, elapsedMs);
    this.timeline.push({ event, elapsedMs });
    if (this.timeline.length > 40) this.timeline.shift();
  }

  private readDiagnostics(): MpvHostDiagnostics {
    const diagnostics = this.host.getDiagnostics();
    this.latestDiagnostics = diagnostics;
    return diagnostics;
  }
}
