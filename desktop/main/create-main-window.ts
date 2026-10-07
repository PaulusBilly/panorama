import path from "node:path";
import { BrowserWindow, shell, type BrowserWindowConstructorOptions } from "electron";
import type { DesktopExternalTarget } from "../shared/desktop-api";

const approvedExternalUrls: Record<DesktopExternalTarget, string> = {
  "stremio-service-download": "https://www.stremio.com/download-service",
};

export function createMainWindowOptions(
  preload: string,
  platform: NodeJS.Platform = process.platform,
): BrowserWindowConstructorOptions {
  const platformOptions: BrowserWindowConstructorOptions = platform === "win32"
    ? {
        title: "",
        backgroundColor: "#00000000",
        transparent: true,
        frame: false,
        thickFrame: true,
        autoHideMenuBar: true,
      }
    : {
        backgroundColor: "#00000000",
        transparent: true,
        titleBarStyle: "hiddenInset",
        trafficLightPosition: { x: 14, y: 13 },
      };
  return {
    width: 1440,
    height: 900,
    minWidth: 960,
    minHeight: 640,
    title: "Panorama",
    ...platformOptions,
    show: false,
    webPreferences: {
      preload,
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: true,
      webSecurity: true,
    },
  };
}

export function isOwnedRendererNavigation(target: string, ownedOrigin: string): boolean {
  try {
    return new URL(target).origin === new URL(ownedOrigin).origin;
  } catch {
    return false;
  }
}

export function isAllowedRendererPermission(
  permission: string,
  requestingUrl: string,
  ownedOrigin: string,
): boolean {
  return permission === "fullscreen" && isOwnedRendererNavigation(requestingUrl, ownedOrigin);
}

export function isAllowedDevelopmentOrigin(target: string): boolean {
  try {
    const url = new URL(target);
    return url.protocol === "http:"
      && url.hostname === "127.0.0.1"
      && url.port !== ""
      && url.username === ""
      && url.password === ""
      && url.pathname === "/"
      && url.search === ""
      && url.hash === "";
  } catch {
    return false;
  }
}

export function getApprovedExternalUrl(target: DesktopExternalTarget): string {
  const url = approvedExternalUrls[target];
  if (!url) throw new Error("Unsupported external target");
  return url;
}

export async function createMainWindow(
  origin: string,
  beforeLoad?: (window: BrowserWindow) => Promise<void>,
): Promise<BrowserWindow> {
  const window = new BrowserWindow(createMainWindowOptions(path.join(__dirname, "../preload/preload.js"), process.platform));
  if (process.platform === "win32") {
    window.setMenuBarVisibility(false);
    window.removeMenu();
    window.on("page-title-updated", (event) => event.preventDefault());
  }
  window.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
  window.webContents.on("will-navigate", (event, target) => {
    if (!isOwnedRendererNavigation(target, origin)) event.preventDefault();
  });
  window.webContents.session.setPermissionCheckHandler((webContents, permission, requestingOrigin) => (
    webContents === window.webContents
    && isAllowedRendererPermission(permission, requestingOrigin, origin)
  ));
  window.webContents.session.setPermissionRequestHandler((webContents, permission, callback, details) => {
    callback(
      webContents === window.webContents
      && isAllowedRendererPermission(permission, details.requestingUrl, origin),
    );
  });
  window.once("ready-to-show", () => window.show());
  await beforeLoad?.(window);
  await window.loadURL(origin);
  return window;
}

export async function openApprovedExternal(target: DesktopExternalTarget): Promise<void> {
  await shell.openExternal(getApprovedExternalUrl(target));
}
