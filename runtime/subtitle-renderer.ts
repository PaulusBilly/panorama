import { subtitleAppearanceCss } from "./subtitle-appearance";
import type { SubtitlePreferences } from "./subtitle-preferences";
import type { SubtitleCueStore } from "./subtitle-cue-store";
import type { PanoramaSubtitleStyle } from "./types";

export function normalizeSubtitleText(text: string): string {
  return text.replace(/\r\n?/g, "\n").replace(/\u0000/g, "");
}

export function browserSubtitleText(cue: TextTrackCue): string | null {
  if (!("getCueAsHTML" in cue) || typeof cue.getCueAsHTML !== "function") return null;
  const fragment = cue.getCueAsHTML() as DocumentFragment;
  for (const element of fragment.querySelectorAll("br")) element.replaceWith(document.createTextNode("\n"));
  return normalizeSubtitleText(fragment.textContent ?? "");
}

export function createSubtitleRenderer(container: HTMLElement, store: SubtitleCueStore, preferences: SubtitlePreferences) {
  const layer = document.createElement("div");
  layer.className = "panorama-subtitle-layer";
  layer.setAttribute("aria-hidden", "true");
  const box = document.createElement("div");
  box.className = "panorama-subtitle-box";
  box.dir = "auto";
  layer.append(box);
  container.append(layer);
  let verticalPosition = preferences.verticalPosition;
  let style = preferences.style;
  const position = () => {
    const height = container.clientHeight;
    const boxHeight = box.getBoundingClientRect().height;
    const inset = Math.max(16, height * verticalPosition / 100);
    layer.style.bottom = `${Math.max(16, Math.min(inset, Math.max(16, height - boxHeight - 16)))}px`;
  };
  const appearance = () => {
    const css = subtitleAppearanceCss(style);
    Object.assign(box.style, css);
    position();
  };
  const render = () => {
    const cue = store.getSnapshot();
    const text = cue?.kind === "text" ? normalizeSubtitleText(cue.text) : "";
    if (box.textContent !== text) { box.textContent = text; position(); }
    layer.hidden = text.length === 0;
  };
  const unsubscribe = store.subscribe(render);
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(position);
  observer?.observe(container);
  observer?.observe(box);
  appearance(); render();
  return {
    updateAppearance(next: PanoramaSubtitleStyle, nextPosition: number) { style = next; verticalPosition = nextPosition; appearance(); },
    destroy() { unsubscribe(); observer?.disconnect(); layer.remove(); },
  };
}
