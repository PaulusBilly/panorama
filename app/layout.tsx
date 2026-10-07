import type { Metadata } from "next";
import { RuntimeProvider } from "@/components/RuntimeMount";
import { DesktopTitlebar } from "@/components/DesktopTitlebar";
import { dmSans } from "./fonts";
import "./globals.css";

export const metadata: Metadata = {
  title: "Panorama",
  description: "An editorial Stremio client for film.",
  manifest: "/site.webmanifest",
};

const desktopBuild = process.env.PANORAMA_DESKTOP_BUILD === "1";

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en" className={`${dmSans.className} ${dmSans.variable}`}>
      <body className={desktopBuild ? "panorama-desktop" : undefined}>
        {desktopBuild ? <DesktopTitlebar /> : null}
        <div className="app-viewport"><RuntimeProvider>{children}</RuntimeProvider></div>
      </body>
    </html>
  );
}
