import type { DesktopCapabilities, PanoramaDesktopApi } from "../desktop/shared/desktop-api";

declare global {
  interface Window {
    panoramaDesktop?: PanoramaDesktopApi;
  }
}

function isDesktopCapabilities(value: unknown): value is DesktopCapabilities {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<DesktopCapabilities>;
  return (candidate.platform === "darwin" || candidate.platform === "win32")
    && (candidate.architecture === "arm64" || candidate.architecture === "x64")
    && Boolean(candidate.nativePlayback)
    && typeof candidate.nativePlayback === "object"
    && (
      (candidate.nativePlayback as { status?: unknown; device?: unknown; mpvVersion?: unknown }).status === "ready"
        ? (candidate.nativePlayback as { device?: unknown; mpvVersion?: unknown }).device === "ShellVideo"
          && typeof (candidate.nativePlayback as { mpvVersion?: unknown }).mpvVersion === "string"
        : (candidate.nativePlayback as { status?: unknown; reason?: unknown }).status === "unavailable"
          && ["missing-host", "unsupported-platform", "initialization-failed"].includes(
            String((candidate.nativePlayback as { reason?: unknown }).reason),
          )
    )
    && typeof candidate.appVersion === "string";
}

export async function getDesktopCapabilities(): Promise<DesktopCapabilities | null> {
  if (typeof window === "undefined" || !window.panoramaDesktop) return null;
  const result = await window.panoramaDesktop.getCapabilities();
  if (!isDesktopCapabilities(result)) throw new Error("Invalid desktop capabilities");
  return result;
}
