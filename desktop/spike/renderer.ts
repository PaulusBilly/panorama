type SpikeRendererApi = {
  play(): Promise<void>;
  pause(): Promise<void>;
  getDiagnostics(): Promise<unknown>;
  setVideoBounds(bounds: {
    visible: boolean;
    x: number;
    y: number;
    width: number;
    height: number;
    scaleFactor: number;
  }): void;
};

const spikeWindow = window as unknown as Window & { panoramaSpike: SpikeRendererApi };

const statusElement = document.querySelector<HTMLElement>("[data-status]");
const diagnostics = document.querySelector<HTMLElement>("[data-diagnostics]");
const play = document.querySelector<HTMLButtonElement>("[data-play]");
const pause = document.querySelector<HTMLButtonElement>("[data-pause]");
const videoSurface = document.querySelector<HTMLElement>("[data-video-surface]");

function syncVideoBounds(): void {
  if (!videoSurface) return;
  const bounds = videoSurface.getBoundingClientRect();
  spikeWindow.panoramaSpike.setVideoBounds({
    visible: bounds.width > 0 && bounds.height > 0,
    x: Math.max(0, bounds.left),
    y: Math.max(0, bounds.top),
    width: Math.max(0, bounds.width),
    height: Math.max(0, bounds.height),
    scaleFactor: window.devicePixelRatio,
  });
}

const boundsObserver = videoSurface && typeof ResizeObserver !== "undefined"
  ? new ResizeObserver(syncVideoBounds)
  : null;
boundsObserver?.observe(videoSurface!);
window.addEventListener("resize", syncVideoBounds);

async function refreshDiagnostics(): Promise<void> {
  if (!diagnostics) return;
  diagnostics.textContent = JSON.stringify(await spikeWindow.panoramaSpike.getDiagnostics(), null, 2);
}

play?.addEventListener("click", async () => {
  try {
    await spikeWindow.panoramaSpike.play();
    if (statusElement) statusElement.textContent = "Playing";
  } catch (error) {
    if (statusElement) statusElement.textContent = error instanceof Error ? error.message : "Playback failed";
  }
  await refreshDiagnostics();
});

pause?.addEventListener("click", async () => {
  await spikeWindow.panoramaSpike.pause();
  if (statusElement) statusElement.textContent = "Paused";
  await refreshDiagnostics();
});

void refreshDiagnostics();
syncVideoBounds();
