import type { NativeSubtitleCue } from "../../shared/subtitle-cue";
export type MpvHostCommand =
  | { type: "load"; url: string; startSeconds: number }
  | { type: "setPaused"; paused: boolean }
  | { type: "seek"; seconds: number }
  | { type: "setBounds"; x: number; y: number; width: number; height: number; scaleFactor: number }
  | { type: "shellCommand"; args: string[] }
  | { type: "setProperty"; name: string; value: string | number | boolean | null }
  | { type: "setGpuVideoProcessing"; enabled: boolean }
  | { type: "stop" };

export type MpvHostEvent =
  | { type: "ready" }
  | { type: "property"; name: string; value: unknown }
  | { type: "ended"; reason: string }
  | { type: "error"; code: string };

export type MpvHostDiagnostics = {
  mpvVersion: string | null;
  videoCodec: string | null;
  hardwareDecoder: string | null;
  buffering: boolean;
  cacheSeconds: number | null;
  cacheEndSeconds?: number | null;
  cacheBufferingPercent?: number | null;
  cacheForwardBytes?: number | null;
  inputBytesPerSecond?: number | null;
  audioDevices?: Array<{ name: string; description: string; supportedPassthroughCodecs?: string[] }>;
  audioOutputFormat?: string | null;
  audioOutputDriver?: string | null;
  audioOutputErrorSequence?: number;
  rendererBackend?: string | null;
  rendererFallbackReason?: string | null;
  videoOutputPrimaries?: string | null;
  videoOutputTransferFunction?: string | null;
  ownedEntryId?: number | null;
  loadGeneration?: number;
  fileStarted?: boolean;
  fileLoaded?: boolean;
  decodedReady?: boolean;
  presented?: boolean;
  presentationEvidence?: "native-swap" | "gpu-render-pass";
  moving?: boolean;
  restartSequence?: number;
  seekGeneration?: number;
  restartedSeekGeneration?: number;
  presentedSeekGeneration?: number;
  audioSampleRate?: number | null;
  audioOutputSampleRate?: number | null;
  audioCodec?: string | null;
  audioChannels?: string | null;
  audioOutputChannels?: string | null;
  videoPixelFormat?: string | null;
  videoColorPrimaries?: string | null;
  videoTransferFunction?: string | null;
  timeSeconds: number | null;
  durationSeconds: number | null;
  path?: string | null;
  ffmpegVersion?: string | null;
  paused?: boolean;
  seeking?: boolean;
  eofReached?: boolean;
  volume?: number | null;
  selectedAudioId?: string | null;
  selectedVideoId?: string | null;
  selectedSubtitleId?: string | null;
  subtitleScale?: number | null;
  subtitlePosition?: number | null;
  subtitleDelay?: number | null;
  speed?: number | null;
  videoWidth?: number | null;
  videoHeight?: number | null;
  endSequence?: number;
  endReason?: string | null;
  endError?: string | null;
  tracks: Array<{
    id: number | null;
    type: string | null;
    language: string | null;
    title: string | null;
    selected: boolean;
    forced: boolean;
    hearingImpaired: boolean;
    codec?: string | null;
  }>;
  renderReady?: boolean;
  renderedFrames?: number | null;
  sourceFps?: number | null;
  displayFps?: number | null;
  frameDropCount?: number | null;
  decoderFrameDropCount?: number | null;
  mistimedFrameCount?: number | null;
  delayedFrameCount?: number | null;
  renderUpdates?: number | null;
  reportedSwaps?: number | null;
};

export type NativeMpvBinding = {
  onSubtitleCue?(listener: (cue: NativeSubtitleCue) => void): () => void;
  dispatch(command: MpvHostCommand): void;
  getDiagnostics(): MpvHostDiagnostics;
  destroy(): void;
};
