import type { NativeSubtitleCue } from "../../shared/subtitle-cue";
import type { MpvHostCommand, MpvHostDiagnostics, NativeMpvBinding } from "./types";

export type NativeAddonHost = {
  onSubtitleCue?(listener: ((cue: NativeSubtitleCue) => void) | null): void;
  load(url: string, startSeconds: number): void;
  setPaused(paused: boolean): void;
  seek(seconds: number): void;
  setBounds(x: number, y: number, width: number, height: number, scaleFactor: number): void;
  command(args: string[]): void;
  setProperty(name: string, value: string | number | boolean | null): void;
  stop(): void;
  getDiagnostics(): MpvHostDiagnostics;
  destroy(): void;
};

export type NativeAddon = {
  NativeMpvHost: new (nativeWindowHandle: Buffer, runtimeDirectory?: string) => NativeAddonHost;
};

function dispatchHost(host: NativeAddonHost, command: MpvHostCommand): void {
  if (command.type === "load") host.load(command.url, command.startSeconds);
  if (command.type === "setPaused") host.setPaused(command.paused);
  if (command.type === "seek") host.seek(command.seconds);
  if (command.type === "setBounds") {
    host.setBounds(command.x, command.y, command.width, command.height, command.scaleFactor);
  }
  if (command.type === "shellCommand") host.command(command.args);
  if (command.type === "setProperty") host.setProperty(command.name, command.value);
  if (command.type === "stop") host.stop();
}

export class MacNativeMpvBinding implements NativeMpvBinding {
  private readonly host: NativeAddonHost;

  constructor(nativeWindowHandle: Buffer, addon: NativeAddon) {
    this.host = new addon.NativeMpvHost(nativeWindowHandle);
  }

  dispatch(command: MpvHostCommand): void {
    dispatchHost(this.host, command);
  }

  onSubtitleCue(listener: (cue: NativeSubtitleCue) => void): () => void {
    this.host.onSubtitleCue?.(listener);
    return () => this.host.onSubtitleCue?.(null);
  }

  getDiagnostics(): MpvHostDiagnostics {
    return this.host.getDiagnostics();
  }

  destroy(): void {
    this.host.destroy();
  }
}

export class WindowsNativeMpvBinding implements NativeMpvBinding {
  private readonly host: NativeAddonHost;

  constructor(nativeWindowHandle: Buffer, runtimeDirectory: string, addon: NativeAddon) {
    this.host = new addon.NativeMpvHost(nativeWindowHandle, runtimeDirectory);
  }

  dispatch(command: MpvHostCommand): void {
    dispatchHost(this.host, command);
  }

  onSubtitleCue(listener: (cue: NativeSubtitleCue) => void): () => void {
    this.host.onSubtitleCue?.(listener);
    return () => this.host.onSubtitleCue?.(null);
  }

  getDiagnostics(): MpvHostDiagnostics {
    return this.host.getDiagnostics();
  }

  destroy(): void {
    this.host.destroy();
  }
}
