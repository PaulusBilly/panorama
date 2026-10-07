import localFont from "next/font/local";

export const dmSans = localFont({
  src: [
    {
      path: "../public/fonts/DMSans-Regular.woff2",
      weight: "400",
      style: "normal",
    },
    {
      path: "../public/fonts/DMSans-Medium.woff2",
      weight: "500",
      style: "normal",
    },
    {
      path: "../public/fonts/DMSans-Bold.woff2",
      weight: "700",
      style: "normal",
    },
  ],
  display: "swap",
  fallback: ["Arial", "Helvetica", "sans-serif"],
  adjustFontFallback: "Arial",
  variable: "--font-dm-sans",
});
