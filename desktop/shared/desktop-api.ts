import type { PlaybackSettings, PlaybackSettingsState } from "./playback-settings";
import type { DiscordPlayback, DiscordSettings } from "./discord-presence";
import type {
  ShellEventChannel,
  ShellSendChannel,
  VideoSurfaceBounds,
} from "./mpv-protocol";

export type NativePlaybackCapability =
  | { status: "ready"; device: "ShellVideo"; mpvVersion: string }
  | { status: "unavailable"; reason: "missing-host" | "unsupported-platform" | "initialization-failed" };

export type DesktopCapabilities = {
  platform: "darwin" | "win32";
  architecture: "arm64" | "x64";
  nativePlayback: NativePlaybackCapability;
  appVersion: string;
};

export type DesktopExternalTarget = "stremio-service-download";
export type DesktopWindowControl = "minimize" | "toggle-maximize" | "close";

export type PlaybackRendererTimingEvent =
  | "play-requested"
  | "preparation-ready"
  | "video-load-requested"
  | "seek-requested";

export type PlaybackTimingEvent = PlaybackRendererTimingEvent
  | "loadfile-issued"
  | "first-frame"
  | "seek-dispatched"
  | "seek-first-frame"
  | "playback-restart"
  | "cache-exit"
  | "playback-moving"
  | "seek-restart"
  | "seek-cache-exit"
  | "seek-moving";

export type DesktopPlaybackDiagnostics = {
  mpvVersion: string | null;
  videoCodec: string | null;
  hardwareDecoder: string | null;
  rendererBackend: "opengl" | "macvk" | "d3d11" | null;
  rendererFallbackReason: "initialization-failed" | null;
  audioOutputFormat: string | null;
  audioOutputDriver: string | null;
  videoOutputPrimaries: string | null;
  videoOutputTransferFunction: string | null;
  buffering: boolean;
  ownedEntryId?: number | null;
  loadGeneration?: number;
  fileStarted?: boolean;
  fileLoaded?: boolean;
  decodedReady?: boolean;
  presented?: boolean;
  presentationEvidence?: "native-swap" | "gpu-render-pass";
  moving?: boolean;
  seekGeneration?: number;
  restartedSeekGeneration?: number;
  presentedSeekGeneration?: number;
  proxy?: import("../main/media-proxy").MediaProxyStats | null;
  resumeBufferSeconds?: number;
  rebufferCount: number;
  rebufferMilliseconds: number;
  cacheSeconds: number | null;
  cacheEndSeconds: number | null;
  cacheBufferingPercent: number | null;
  cacheForwardBytes: number | null;
  inputBytesPerSecond: number | null;
  downloadMbps: number | null;
  sourceBitrateMbps: number | null;
  // The remote source's server refused or dropped the connection recently.
  sourceUnreachable: boolean;
  audioSampleRate: number | null;
  audioOutputSampleRate: number | null;
  audioCodec: string | null;
  audioChannels: string | null;
  audioOutputChannels: string | null;
  videoPixelFormat: string | null;
  videoColorPrimaries: string | null;
  videoTransferFunction: string | null;
  sourceFps: number | null;
  displayFps: number | null;
  frameDropCount: number;
  decoderFrameDropCount: number;
  mistimedFrameCount: number;
  delayedFrameCount: number;
  videoWidth: number | null;
  videoHeight: number | null;
  renderUpdates: number | null;
  renderedFrames: number | null;
  reportedSwaps: number | null;
  sourceKind: "loopback-service" | "remote" | null;
  startupTiming: {
    totalMs: number | null;
    preparationMs: number | null;
    videoDispatchMs: number | null;
    nativeFirstFrameMs: number | null;
  };
  lastSeekToFirstFrameMs: number | null;
  timeline: Array<{ event: PlaybackTimingEvent; elapsedMs: number }>;
};

export type PanoramaDesktopApi = {
  getDiscordSettings?(): Promise<DiscordSettings>;
  setDiscordEnabled?(enabled: boolean): Promise<DiscordSettings>;
  updateDiscordPlayback?(playback: DiscordPlayback | null): void;
  getPlaybackSettings?(): Promise<PlaybackSettingsState>;
  setPlaybackSettings?(settings: PlaybackSettings): Promise<PlaybackSettingsState>;
  getCapabilities(): Promise<DesktopCapabilities>;
  getPlaybackDiagnostics?(): Promise<DesktopPlaybackDiagnostics | null>;
  recordPlaybackTiming?(event: PlaybackRendererTimingEvent): void;
  // Resolves a remote source and fetches its opening ahead of playback.
  warmMedia?(url: string): Promise<{ status: "ready" | "failed"; error: string | null; unreachable: boolean }>;
  setFullscreen?(fullscreen: boolean): Promise<void>;
  onFullscreenChange?(listener: (fullscreen: boolean) => void): () => void;
  controlWindow?(action: DesktopWindowControl): Promise<void>;
  openExternal(target: DesktopExternalTarget): Promise<void>;
  mpv?: {
    send(channel: ShellSendChannel, payload: unknown): void;
    on(channel: ShellEventChannel, listener: (payload: unknown) => void): () => void;
    setVideoSurface(bounds: VideoSurfaceBounds): void;
  };
};
