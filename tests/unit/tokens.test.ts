import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const globals = readFileSync(join(process.cwd(), "app/globals.css"), "utf8");
const componentSource = [
  "components/AccountMenu.tsx",
  "components/FilmCard.tsx",
  "components/FilmDetailsPage.tsx",
  "components/FilmWatchPage.tsx",
  "components/LoginDialog.tsx",
  "components/PanoramaHome.tsx",
  "components/PanoramaHeaderRow.tsx",
  "components/PlayerDialog.tsx",
  "components/PlayerSubtitlesPopover.tsx",
  "components/PlayerTrackPopover.tsx",
  "components/PlayerSourcesPopover.tsx",
].map((path) => readFileSync(join(process.cwd(), path), "utf8")).join("\n");

describe("Panorama tokens", () => {
  it("exposes semantic OKLCH roles and a dark-ready override", () => {
    for (const role of [
      "canvas",
      "ink",
      "ink-muted",
      "surface",
      "surface-strong",
      "rule",
      "danger",
      "inverse",
      "scrim",
      "focus-inner",
      "focus-outer",
      "hover",
      "selected",
      "disabled",
      "shadow",
      "artwork-empty",
    ]) {
      expect(globals).toContain(`--theme-${role}:`);
    }
    expect(globals).toContain("[data-theme=\"dark\"]");
    expect(globals).toContain("oklch(");
  });

  it("keeps raw Tailwind palette utilities out of components", () => {
    expect(componentSource).not.toMatch(/(?:bg|text|border|ring|from|to)-(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)-/);
    expect(componentSource).not.toContain("transition-all");
    expect(componentSource).not.toMatch(/tracking-|letter-spacing/);
    const letterSpacingValues = [...globals.matchAll(/letter-spacing:\s*([^;]+)/g)].map((match) => match[1]?.trim());
    expect(new Set(letterSpacingValues)).toEqual(new Set(["normal"]));
    expect(componentSource).not.toContain(".module.css");
  });
});
