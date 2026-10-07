import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import type { MpvHost } from "../native/mpv-host/mpv-host";
import type { MpvHostDiagnostics } from "../native/mpv-host/types";
import { parsePlaybackSettings, type PlaybackSettings, type PlaybackSettingsState } from "../shared/playback-settings";

type Preferences = { device: string; channels: PlaybackSettings["channels"]; video: PlaybackSettings["video"]; receivers: string[] };
const defaults: Preferences = { device: "auto", channels: "auto", video: "auto", receivers: [] };
const codecs = new Set(["ac3", "dts", "eac3", "truehd", "dts-hd"]);

export class PlaybackSettingsController {
  private preferences: Preferences = { ...defaults, receivers: [] };
  private readonly ids = new Map<string, string>([["auto", "default"]]);
  private diagnostics: MpvHostDiagnostics | null = null;
  private encoded = false;
  private pcmVolume = 100;
  private speed = 1;
  private failed = false;
  private errorSequence = 0;
  private notice: string | null = null;
  private appliedAudio = "";
  private lastAudioId: string | null = null;

  resetSource(): void {
    this.lastAudioId = null;
  }

  constructor(private readonly host: MpvHost, private readonly file?: string) {
    if (!file) return;
    try {
      const saved = JSON.parse(readFileSync(file, "utf8")) as Preferences;
      parsePlaybackSettings({ deviceId: "default", channels: saved.channels, video: saved.video, passthrough: false });
      if (typeof saved.device !== "string" || saved.device.length > 4096 || !Array.isArray(saved.receivers) ||
        saved.receivers.length > 100 || !saved.receivers.every((entry) => typeof entry === "string" && entry !== "auto" && entry.length <= 4096)) return;
      this.preferences = { device: saved.device, channels: saved.channels, video: saved.video, receivers: saved.receivers };
    } catch {}
  }

  private devices() {
    return (this.diagnostics?.audioDevices ?? []).filter((device) => device.name !== "auto");
  }

  getState(): PlaybackSettingsState {
    const devices = this.devices().map((device) => {
      if (!this.ids.has(device.name)) this.ids.set(device.name, randomUUID());
      return { id: this.ids.get(device.name)!, label: device.description || "Audio output", passthroughEnabled: this.preferences.receivers.includes(device.name), passthroughAvailable: (device.supportedPassthroughCodecs ?? []).some((codec) => codecs.has(codec)) };
    });
    return {
      deviceId: this.ids.get(this.preferences.device) ?? "default",
      channels: this.preferences.channels,
      passthrough: this.preferences.receivers.includes(this.preferences.device),
      video: this.preferences.video,
      devices: [{ id: "default", label: "System default", passthroughAvailable: false, passthroughEnabled: false }, ...devices],
      effectivePassthrough: this.encoded && Boolean(this.diagnostics?.audioOutputFormat?.startsWith("spdif")),
      notice: this.notice,
    };
  }

  initialize(diagnostics: MpvHostDiagnostics): void {
    this.diagnostics = diagnostics;
    if (diagnostics.selectedAudioId && /^\d+$/.test(diagnostics.selectedAudioId)) this.lastAudioId = diagnostics.selectedAudioId;
    if (this.preferences.device !== "auto" && !this.devices().some((device) => device.name === this.preferences.device)) this.preferences.device = "auto";
    this.apply();
  }

  set(value: unknown): PlaybackSettingsState {
    const input = parsePlaybackSettings(value);
    this.getState();
    const device = [...this.ids].find(([, id]) => id === input.deviceId)?.[0];
    if (!device || (device !== "auto" && !this.devices().some((entry) => entry.name === device))) throw new Error("Audio output is unavailable");
    const available = this.devices().find((entry) => entry.name === device)?.supportedPassthroughCodecs ?? [];
    if (input.passthrough && (device === "auto" || !available.some((codec) => codecs.has(codec)))) throw new Error("Receiver passthrough is unavailable for this output");
    const previous = this.preferences;
    const previousFailed = this.failed;
    const previousNotice = this.notice;
    this.preferences = { device, channels: input.channels, video: input.video, receivers: [...previous.receivers.filter((name) => name !== device), ...(input.passthrough ? [device] : [])] };
    this.failed = false;
    this.notice = null;
    try {
      this.apply();
      this.save();
    } catch {
      this.preferences = previous;
      this.failed = previousFailed;
      this.notice = previousNotice;
      this.appliedAudio = "";
      try { this.apply(); } catch { this.fallback("Audio settings could not be applied. Using decoded audio."); }
      throw new Error("Playback settings could not be saved. Try again.");
    }
    return this.getState();
  }

  poll(diagnostics: MpvHostDiagnostics): void {
    this.diagnostics = diagnostics;
    if (diagnostics.selectedAudioId && /^\d+$/.test(diagnostics.selectedAudioId)) this.lastAudioId = diagnostics.selectedAudioId;
    if (this.preferences.device !== "auto" && !this.devices().some((entry) => entry.name === this.preferences.device)) {
      this.preferences.device = "auto";
      this.fallback("Audio output disconnected. Using system default with decoded audio.");
      try { this.save(); } catch { this.notice = "Using system default. The audio preference could not be saved."; }
    }
    const format = diagnostics.audioOutputFormat;
    if (format && diagnostics.volume !== undefined && diagnostics.volume !== null) {
      const volume = this.encoded && format.startsWith("spdif-") ? 100 : this.pcmVolume;
      if (diagnostics.volume !== volume) this.host.setPlaybackProperty("volume", volume);
    }
    const sequence = diagnostics.audioOutputErrorSequence ?? 0;
    if (sequence > this.errorSequence) {
      this.errorSequence = sequence;
      if (!this.failed) this.fallback("Audio output failed. Retrying with decoded audio.");
    }
  }

  setSpeed(speed: number): void {
    if (this.speed === speed) return;
    const previous = this.speed;
    this.speed = speed;
    try { this.applyAudio(); } catch (error) {
      this.speed = previous;
      this.appliedAudio = "";
      try { this.applyAudio(); } catch { this.fallback("Audio output is unavailable. Choose another output."); }
      throw error;
    }
  }

  volume(value: number): number {
    if (this.encoded && (!this.diagnostics?.audioOutputFormat || this.diagnostics.audioOutputFormat.startsWith("spdif-"))) return 100;
    this.pcmVolume = value;
    return value;
  }

  private fallback(notice: string): void {
    this.failed = true;
    this.preferences.device = "auto";
    this.appliedAudio = "";
    this.notice = notice;
    try {
      this.applyAudio();
      if (this.lastAudioId) this.host.dispatch({ type: "setProperty", name: "aid", value: this.lastAudioId });
    } catch { this.notice = "Audio output is unavailable. Choose another output."; }
  }

  private applyAudio(): void {
    const device = this.devices().find((entry) => entry.name === this.preferences.device);
    const allowed = (device?.supportedPassthroughCodecs ?? []).filter((codec) => codecs.has(codec));
    const encoded = !this.failed && this.speed === 1 && this.preferences.receivers.includes(this.preferences.device) && allowed.length > 0;
    const signature = JSON.stringify([this.preferences.device, this.preferences.channels, encoded ? allowed : []]);
    if (signature === this.appliedAudio) return;
    this.appliedAudio = "";
    this.host.setPlaybackProperty("audio-spdif", "");
    this.host.setPlaybackProperty("audio-device", this.preferences.device);
    this.host.setPlaybackProperty("audio-channels", this.preferences.channels === "auto" ? "auto-safe" : this.preferences.channels);
    this.host.setPlaybackProperty("volume", encoded ? 100 : this.pcmVolume);
    this.host.setPlaybackProperty("audio-spdif", encoded ? allowed.join(",") : "");
    this.encoded = encoded;
    this.appliedAudio = signature;
    if (this.notice === "Decoded audio is used at this playback speed." && this.speed === 1) this.notice = null;
    if (this.speed !== 1 && this.preferences.receivers.includes(this.preferences.device)) this.notice = "Decoded audio is used at this playback speed.";
  }

  private apply(): void {
    this.applyAudio();
    if (this.diagnostics?.rendererBackend === "macvk" || this.diagnostics?.rendererBackend === "d3d11") {
      this.host.setPlaybackProperty("target-colorspace-hint", this.preferences.video === "sdr" ? "no" : "auto");
    }
    this.host.setPlaybackProperty("target-prim", this.preferences.video === "sdr" ? "bt.709" : "auto");
    this.host.setPlaybackProperty("target-trc", this.preferences.video === "sdr" ? "bt.1886" : "auto");
  }

  private save(): void {
    if (!this.file) return;
    mkdirSync(dirname(this.file), { recursive: true });
    const temporary = `${this.file}.${randomUUID()}.tmp`;
    try {
      writeFileSync(temporary, JSON.stringify(this.preferences), { mode: 0o600 });
      renameSync(temporary, this.file);
    } finally { rmSync(temporary, { force: true }); }
  }
}
