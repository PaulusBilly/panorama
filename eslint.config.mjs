import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTypeScript from "eslint-config-next/typescript";

export default defineConfig([
  ...nextVitals,
  ...nextTypeScript,
  {
    rules: {
      "@next/next/no-img-element": "off",
    },
  },
  {
    files: ["runtime/vtt-js-compat.cjs", "desktop/scripts/*.cjs", "desktop/stremio-server/*.cjs"],
    rules: {
      "@typescript-eslint/no-require-imports": "off",
    },
  },
  globalIgnores([".cache/**", ".next/**", ".next-*/**", "coverage/**", "desktop-dist/**", "desktop-resources/**", "out/**", "playwright-report/**", "test-results/**"]),
]);
