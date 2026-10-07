import type { CSSProperties } from "react";
import type { PanoramaSubtitleStyle } from "./types";

const finite = (value: unknown, fallback: number, min: number, max: number, step = 1) =>
  typeof value === "number" && Number.isFinite(value) ? Math.min(max, Math.max(min, Math.round(value / step) * step)) : fallback;
const color = (value: unknown, fallback: string): string => typeof value === "string" && /^(?:#(?:[\da-f]{3}|[\da-f]{4}|[\da-f]{6}|[\da-f]{8})|rgba?\([\d\s.,%]+\)|hsla?\([\d\s.,%]+\)|transparent)$/i.test(value.trim()) ? value.trim() : fallback;

export function normalizeSubtitleAppearance(value: unknown): PanoramaSubtitleStyle {
  const record = value && typeof value === "object" && !Array.isArray(value) ? value as Partial<PanoramaSubtitleStyle> : {};
  const size = finite(record.size, 100, 12 / 38 * 100, 96 / 38 * 100, 0.01);
  const fontSizePx = finite(record.fontSizePx, Math.round(38 * size / 100), 12, 96);
  const background = color(record.backgroundColor, "rgba(0, 0, 0, 0.68)");
  const rgba = /^(?:rgba|hsla)\([^,]+,[^,]+,[^,]+,\s*([\d.]+)(%)?\s*\)$/i.exec(background);
  const hex = /^#[\da-f]{6}([\da-f]{2})$/i.exec(background);
  const shortHex = /^#[\da-f]{3}([\da-f])$/i.exec(background);
  const alpha = background === "transparent" ? 0 : rgba ? Number(rgba[1]) * (rgba[2] ? 1 : 100) : hex ? parseInt(hex[1], 16) / 255 * 100 : shortHex ? parseInt(shortHex[1], 16) / 15 * 100 : background.startsWith("rgba") || background.length === 9 ? 68 : 100;
  const fontWeight = record.fontWeight === "bold" || record.fontWeight === "regular" ? record.fontWeight : record.fontWeight === "medium" || record.fontWeight === "semibold" ? "medium" : "regular";
  return {
    size: Math.round(fontSizePx / 38 * 10000) / 100,
    fontSizePx,
    lineHeight: finite(record.lineHeight, 1.24, 1, 2, 0.01),
    backgroundOpacity: finite(record.backgroundOpacity, Math.round(alpha), 0, 100),
    borderRadius: finite(record.borderRadius, 0, 0, 32),
    paddingX: finite(record.paddingX, 20, 0, 64),
    paddingY: finite(record.paddingY, 0, 0, 64),
    fontWeight,
    textColor: color(record.textColor, "#ffffff"),
    textOpacity: finite(record.textOpacity, 100, 0, 100),
    backgroundColor: background,
    outlineColor: color(record.outlineColor, "rgba(0, 0, 0, 0.78)"),
  };
}

export function updateSubtitleAppearance(current: PanoramaSubtitleStyle, patch: Partial<PanoramaSubtitleStyle>): PanoramaSubtitleStyle {
  const merged = { ...current, ...patch };
  if (patch.fontSizePx === undefined && patch.size !== undefined) delete (merged as Partial<PanoramaSubtitleStyle>).fontSizePx;
  if (patch.backgroundColor !== undefined && patch.backgroundOpacity === undefined) delete (merged as Partial<PanoramaSubtitleStyle>).backgroundOpacity;
  return normalizeSubtitleAppearance(merged);
}

export function subtitleAppearanceCss(value: PanoramaSubtitleStyle): CSSProperties {
  const style = normalizeSubtitleAppearance(value);
  return {
    fontFamily: 'var(--font-dm-sans), "Apple Symbols", "Segoe UI Symbol", Arial, sans-serif',
    fontSize: `${style.fontSizePx}px`,
    fontWeight: style.fontWeight === "bold" ? 700 : style.fontWeight === "medium" ? 500 : 400,
    fontSynthesis: "none",
    lineHeight: style.lineHeight,
    padding: `${style.paddingY}px ${style.paddingX}px`,
    color: style.textOpacity === 100 ? style.textColor : `color-mix(in srgb, ${style.textColor} ${style.textOpacity}%, transparent)`,
    backgroundColor: `color-mix(in srgb, ${opaqueColor(style.backgroundColor)} ${style.backgroundOpacity}%, transparent)`,
    borderRadius: `${style.borderRadius}px`,
    textShadow: "none",
    whiteSpace: "pre-wrap",
    overflowWrap: "anywhere",
    letterSpacing: "normal",
    overflow: "visible",
  };
}

function opaqueColor(value: string): string {
  if (value === "transparent") return "#000000";
  const hex = /^#([\da-f]{6})[\da-f]{2}$/i.exec(value);
  if (hex) return `#${hex[1]}`;
  const shortHex = /^#([\da-f]{3})[\da-f]$/i.exec(value);
  if (shortHex) return `#${shortHex[1]}`;
  const rgba = /^rgba\(([^,]+),([^,]+),([^,]+),[^)]+\)$/i.exec(value);
  if (rgba) return `rgb(${rgba[1]},${rgba[2]},${rgba[3]})`;
  const hsla = /^hsla\(([^,]+),([^,]+),([^,]+),[^)]+\)$/i.exec(value);
  return hsla ? `hsl(${hsla[1]},${hsla[2]},${hsla[3]})` : value;
}
