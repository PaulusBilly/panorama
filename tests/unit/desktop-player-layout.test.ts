import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const playerSource = readFileSync("components/PlayerDialog.tsx", "utf8");
const globalStyles = readFileSync("app/globals.css", "utf8");

describe("desktop player layout", () => {
  it("keeps the complete player between the titlebar and window bottom", () => {
    expect(playerSource).not.toContain('className="desktop-player-page h-dvh');
    expect(globalStyles).toMatch(/\.desktop-player-page \{[\s\S]*?height: 100dvh;/);
    expect(globalStyles).toMatch(
      /body\.panorama-desktop \.desktop-player-page \{[\s\S]*?position: fixed;[\s\S]*?inset: 38px 0 0;[\s\S]*?height: auto;/,
    );
  });

  it("sizes hero content to the visible desktop viewport", () => {
    expect(globalStyles).toMatch(
      /body\.panorama-desktop \.desktop-hero-viewport \{[\s\S]*?min-height: calc\(100dvh - 38px\);/,
    );
    expect(globalStyles).toMatch(
      /body\.panorama-desktop\.panorama-desktop-fullscreen \.desktop-hero-viewport \{[\s\S]*?min-height: 100dvh;/,
    );
    expect(globalStyles.lastIndexOf("body.panorama-desktop .desktop-hero-viewport")).toBeGreaterThan(
      globalStyles.lastIndexOf("@media"),
    );
  });

  it("keeps fixed site headers below the desktop titlebar", () => {
    expect(globalStyles).toMatch(
      /\.panorama-site-header \{[\s\S]*?top: 0;[\s\S]*?transform: translate3d\(0, 0, 0\);[\s\S]*?transform 200ms ease-in-out;/,
    );
    expect(globalStyles).toMatch(
      /\.panorama-site-header\[data-hidden="true"\] \{[\s\S]*?transform: translate3d\(0, -100%, 0\);/,
    );
    expect(globalStyles).toMatch(
      /body\.panorama-desktop \.panorama-site-header \{[\s\S]*?top: 38px;/,
    );
    expect(globalStyles).toMatch(
      /body\.panorama-desktop \.panorama-site-header\[data-hidden="true"\] \{[\s\S]*?transform: translate3d\(0, calc\(-100% - 38px\), 0\);/,
    );
    expect(globalStyles).toMatch(
      /body\.panorama-desktop\.panorama-desktop-fullscreen \.panorama-site-header \{[\s\S]*?top: 0;/,
    );
    expect(globalStyles).toMatch(
      /body\.panorama-desktop\.panorama-desktop-fullscreen \.panorama-site-header\[data-hidden="true"\] \{[\s\S]*?transform: translate3d\(0, -100%, 0\);/,
    );
    expect(globalStyles.lastIndexOf(".panorama-site-header")).toBeGreaterThan(
      globalStyles.lastIndexOf("@media"),
    );
  });
});
