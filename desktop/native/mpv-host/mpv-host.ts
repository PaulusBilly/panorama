import type { NativeSubtitleCue } from "../../shared/subtitle-cue";
import type {
  MpvHostCommand,
  MpvHostDiagnostics,
  NativeMpvBinding,
} from "./types";
import { parseShellSend } from "../../shared/mpv-protocol";

function isFiniteNonNegative(value: number): boolean {
  return Number.isFinite(value) && value >= 0;
}

function hasExactOrigin(target: string, expectedOrigin: string): boolean {
  try {
    return new URL(target).origin === new URL(expectedOrigin).origin;
  } catch {
    return false;
  }
}

function isExactUrl(target: string, expected: string | null): boolean {
  if (!expected) return false;
  try {
    return new URL(target).href === new URL(expected).href;
  } catch {
    return false;
  }
}

export class MpvHost {
  private destroyed = false;

  constructor(
    private readonly binding: NativeMpvBinding,
    serviceOrigins: string | readonly string[],
    private readonly authorizedExternalUrl: string | null = null,
  ) {
    this.serviceOrigins = typeof serviceOrigins === "string" ? [serviceOrigins] : [...serviceOrigins];
  }

  private readonly serviceOrigins: readonly string[];

  dispatch(command: MpvHostCommand): void {
    this.assertActive();
    if (command.type === "load") {
      if (
        !this.serviceOrigins.some((origin) => hasExactOrigin(command.url, origin)) &&
        !isExactUrl(command.url, this.authorizedExternalUrl)
      ) {
        throw new Error("Invalid media URL.");
      }
      if (!isFiniteNonNegative(command.startSeconds)) {
        throw new Error("Invalid playback start time.");
      }
    }
    if (command.type === "setBounds") {
      const values = [command.x, command.y, command.width, command.height, command.scaleFactor];
      if (!values.every(isFiniteNonNegative)) {
        throw new Error("Invalid video bounds.");
      }
      if (command.scaleFactor === 0) throw new Error("Invalid video bounds.");
    }
    if (command.type === "seek" && !isFiniteNonNegative(command.seconds)) {
      throw new Error("Invalid seek time.");
    }
    if (command.type === "shellCommand") {
      parseShellSend("mpv-command", command.args);
    }
    if (command.type === "setProperty") {
      parseShellSend("mpv-set-prop", [command.name, command.value]);
    }
    this.binding.dispatch(command);
  }

  setPlaybackProperty(name: string, value: string | number): void {
    this.assertActive();
    const valid = name === "cache-pause-wait" ? typeof value === "number" && Number.isFinite(value) && value >= 2 && value <= 15
      : name === "audio-device" ? typeof value === "string" && value.length > 0 && value.length <= 4096 && !/[\u0000-\u001f]/.test(value)
      : name === "audio-channels" ? ["auto-safe", "stereo", "5.1", "7.1"].includes(String(value))
      : name === "audio-spdif" ? typeof value === "string" && (value === "" || value.split(",").every((codec) => ["ac3", "dts", "eac3", "truehd", "dts-hd"].includes(codec)))
      : name === "target-prim" ? ["auto", "bt.709"].includes(String(value))
      : name === "target-trc" ? ["auto", "bt.1886"].includes(String(value))
      : name === "target-colorspace-hint" ? ["auto", "no"].includes(String(value))
      : name === "volume" && typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 200;
    if (!valid) throw new Error("Invalid playback property");
    this.binding.dispatch({ type: "setProperty", name, value });
  }

  onSubtitleCue(listener: (cue: NativeSubtitleCue) => void): () => void {
    this.assertActive();
    return this.binding.onSubtitleCue?.(listener) ?? (() => undefined);
  }

  getDiagnostics(): MpvHostDiagnostics {
    this.assertActive();
    return this.binding.getDiagnostics();
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.binding.destroy();
  }

  private assertActive(): void {
    if (this.destroyed) throw new Error("MPV host is destroyed.");
  }
}
