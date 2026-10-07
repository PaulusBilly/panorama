import type { NativeMpvBinding } from "../native/mpv-host/types";
import {
  MacNativeMpvBinding,
  WindowsNativeMpvBinding,
  type NativeAddon,
} from "../native/mpv-host/native-binding";

export type NativeBindingFactoryInput = {
  platform: NodeJS.Platform;
  architecture: string;
  nativeWindowHandle: Buffer;
  runtimeDirectory: string;
  addon: NativeAddon;
};

export function createNativeBinding(input: NativeBindingFactoryInput): NativeMpvBinding | null {
  if (input.platform === "darwin" && input.architecture === "arm64") {
    return new MacNativeMpvBinding(input.nativeWindowHandle, input.addon);
  }
  if (input.platform === "win32" && input.architecture === "x64") {
    return new WindowsNativeMpvBinding(input.nativeWindowHandle, input.runtimeDirectory, input.addon);
  }
  return null;
}
