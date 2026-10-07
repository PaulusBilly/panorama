import { nativeVideoSurfaceBounds } from "../desktop/shared/video-surface";

export type NativeVideoSurfaceBinding = {
  setVisible(visible: boolean): void;
  destroy(): void;
};

export function bindNativeVideoSurface(container: HTMLElement): NativeVideoSurfaceBinding {
  const api = window.panoramaDesktop?.mpv;
  if (!api) return { setVisible() {}, destroy() {} };
  let visible = false;
  let destroyed = false;
  let previous = "";
  const sync = () => {
    if (destroyed) return;
    const bounds = nativeVideoSurfaceBounds(container.getBoundingClientRect(), window.devicePixelRatio, visible);
    const serialized = JSON.stringify(bounds);
    if (serialized === previous) return;
    previous = serialized;
    api.setVideoSurface(bounds);
  };
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(sync);
  observer?.observe(container);
  window.addEventListener("resize", sync);
  document.addEventListener("fullscreenchange", sync);
  return {
    setVisible(nextVisible) {
      visible = nextVisible;
      document.documentElement.classList.toggle("native-video-surface-active", visible);
      sync();
    },
    destroy() {
      if (destroyed) return;
      visible = false;
      document.documentElement.classList.remove("native-video-surface-active");
      previous = "";
      sync();
      destroyed = true;
      observer?.disconnect();
      window.removeEventListener("resize", sync);
      document.removeEventListener("fullscreenchange", sync);
    },
  };
}
