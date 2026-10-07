import type { NextConfig } from "next";
import path from "node:path";

const isDesktopBuild = process.env.PANORAMA_DESKTOP_BUILD === "1";

const nextConfig: NextConfig = {
  distDir: process.env.PANORAMA_DIST_DIR ?? ".next",
  output: isDesktopBuild ? "standalone" : undefined,
  reactStrictMode: true,
  // Hide the floating dev-tools badge; compile and runtime errors still surface.
  devIndicators: false,
  webpack(config) {
    config.resolve.alias["vtt.js$"] = path.resolve(process.cwd(), "runtime/vtt-js-compat.cjs");
    config.module.rules.push({
      test: /\.wasm$/,
      type: "asset/resource",
    });

    return config;
  },
};

export default nextConfig;
