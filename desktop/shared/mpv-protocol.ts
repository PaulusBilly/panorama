export type ShellSendChannel =
  | "mpv-command"
  | "mpv-set-prop"
  | "mpv-observe-prop"
  | "mpv-set-gpu-video-processing";

export type ShellEventChannel =
  | "mpv-prop-change"
  | "mpv-event-ended"
  | "mpv-event-video-ready"
  | "mpv-event-subtitle-cue";

export type VideoSurfaceBounds = {
  visible: boolean;
  x: number;
  y: number;
  width: number;
  height: number;
  scaleFactor: number;
};

const observedProperties = new Set([
  "path",
  "time-pos",
  "volume",
  "pause",
  "seeking",
  "eof-reached",
  "duration",
  "metadata",
  "video-params",
  "track-list",
  "paused-for-cache",
  "cache-buffering-state",
  "demuxer-cache-time",
  "aid",
  "vid",
  "sid",
  "sub-scale",
  "sub-pos",
  "sub-delay",
  "speed",
  "mpv-version",
  "ffmpeg-version",
]);

const writableProperties = new Set([
  "pause",
  "time-pos",
  "speed",
  "keepaspect",
  "panscan",
  "mute",
  "volume",
  "aid",
  "sid",
  "sub-visibility",
  "sub-scale",
  "sub-pos",
  "sub-delay",
  "sub-color",
  "sub-back-color",
  "sub-border-color",
  "sub-font",
  "sub-bold",
  "sub-shadow-offset",
  "sub-ass-override",
  "hwdec",
  "vo",
  "osc",
  "input-default-bindings",
  "input-vo-keyboard",
]);

function isPlainArray(value: unknown): value is unknown[] {
  if (!Array.isArray(value) || Object.getPrototypeOf(value) !== Array.prototype) return false;
  return Object.keys(value).every((key) => key === "length" || /^\d+$/.test(key));
}

function isSafeMediaUrl(value: unknown): value is string {
  if (typeof value !== "string") return false;
  try {
    const url = new URL(value);
    return (url.protocol === "http:" || url.protocol === "https:") && url.username === "" && url.password === "";
  } catch {
    return false;
  }
}

function isShellValue(value: unknown): boolean {
  return value === null || typeof value === "string" || typeof value === "boolean" ||
    (typeof value === "number" && Number.isFinite(value));
}

function isSafeMpvText(value: unknown, maxLength: number): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= maxLength &&
    !/[\u0000-\u001f\u007f]/.test(value);
}

export function parseShellSend(channel: unknown, payload: unknown): { channel: ShellSendChannel; payload: unknown } {
  if (channel === "mpv-command") {
    if (!isPlainArray(payload) || payload.some((entry) => typeof entry !== "string")) {
      throw new Error("Invalid MPV command payload");
    }
    const command = payload as string[];
    if (command.length === 1 && command[0] === "stop") return { channel, payload: command };
    if (
      command[0] === "loadfile" &&
      isSafeMediaUrl(command[1]) &&
      (command.length === 2 || (
        (command.length === 4 || command.length === 5) &&
        command[2] === "replace" &&
        command.slice(3).every((entry) => entry === "-1" || /^start=\+\d+$/.test(entry))
      ))
    ) return { channel, payload: command };
    if (
      command.length === 5 &&
      command[0] === "sub-add" &&
      isSafeMediaUrl(command[1]) &&
      command[2] === "cached" &&
      isSafeMpvText(command[3], 240) &&
      isSafeMpvText(command[4], 32)
    ) return { channel, payload: command };
    throw new Error("Unsupported MPV command");
  }
  if (channel === "mpv-observe-prop") {
    if (typeof payload !== "string" || !observedProperties.has(payload)) {
      throw new Error("Unsupported MPV observed property");
    }
    return { channel, payload };
  }
  if (channel === "mpv-set-prop") {
    if (
      !isPlainArray(payload) || payload.length !== 2 ||
      typeof payload[0] !== "string" || !writableProperties.has(payload[0]) ||
      !isShellValue(payload[1])
    ) throw new Error("Unsupported MPV property write");
    if (payload[0] === "sub-visibility" && typeof payload[1] !== "boolean") throw new Error("Invalid subtitle visibility");
    return { channel, payload };
  }
  if (channel === "mpv-set-gpu-video-processing") {
    if (typeof payload !== "boolean") throw new Error("Invalid MPV GPU processing value");
    return { channel, payload };
  }
  throw new Error("Unsupported MPV channel");
}

function finiteNonNegative(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

export function parseVideoSurface(value: unknown): VideoSurfaceBounds {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid video surface");
  const candidate = value as Partial<VideoSurfaceBounds>;
  if (
    typeof candidate.visible !== "boolean" ||
    !finiteNonNegative(candidate.x) || !finiteNonNegative(candidate.y) ||
    !finiteNonNegative(candidate.width) || !finiteNonNegative(candidate.height) ||
    !finiteNonNegative(candidate.scaleFactor) || candidate.scaleFactor === 0
  ) throw new Error("Invalid video surface");
  if (!candidate.visible || candidate.width === 0 || candidate.height === 0) {
    return { visible: false, x: 0, y: 0, width: 0, height: 0, scaleFactor: candidate.scaleFactor };
  }
  return candidate as VideoSurfaceBounds;
}
