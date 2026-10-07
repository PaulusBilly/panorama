import path from "node:path";

export function desktopExecutablePath(): string {
  if (process.env.PANORAMA_DESKTOP_EXECUTABLE) return process.env.PANORAMA_DESKTOP_EXECUTABLE;
  if (process.platform === "win32") {
    return path.join(process.cwd(), "out/panorama-win32-x64/panorama.exe");
  }
  return path.join(process.cwd(), "out/panorama-darwin-arm64/panorama.app/Contents/MacOS/panorama");
}

export function desktopLaunchOptions(): { executablePath: string; args?: string[] } {
  const userDataDirectory = process.env.PANORAMA_DESKTOP_USER_DATA_DIR;
  return {
    executablePath: desktopExecutablePath(),
    ...(userDataDirectory ? { args: [`--user-data-dir=${path.resolve(userDataDirectory)}`] } : {}),
  };
}
