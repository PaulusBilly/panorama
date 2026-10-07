import { access, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const defaultProjectRoot = path.resolve(scriptDirectory, "../..");

export async function requiredWindowsPackageFiles(projectRoot = defaultProjectRoot) {
  const manifest = JSON.parse(await readFile(
    path.join(projectRoot, "desktop/native/mpv-host/windows-libmpv.json"),
    "utf8",
  ));
  const resourceRoot = "resources/desktop-resources";
  return [
    "panorama.exe",
    `${resourceRoot}/standalone/server.js`,
    `${resourceRoot}/standalone/.next-desktop/static`,
    `${resourceRoot}/standalone/public/fonts/DMSans-Regular.woff2`,
    `${resourceRoot}/standalone/public/fonts/DMSans-Medium.woff2`,
    `${resourceRoot}/standalone/public/fonts/DMSans-Bold.woff2`,
    `${resourceRoot}/standalone/public/fonts/DMSans-Regular.ttf`,
    `${resourceRoot}/standalone/public/fonts/DMSans-Medium.ttf`,
    `${resourceRoot}/standalone/public/fonts/DMSans-Bold.ttf`,
    `${resourceRoot}/standalone/public/fonts/OFL.txt`,
    `${resourceRoot}/native/win32-x64/mpv_host.node`,
    ...manifest.files.runtime.map((entry) => (
      `${resourceRoot}/native/win32-x64/${path.posix.basename(entry)}`
    )),
    `${resourceRoot}/native/win32-x64/licenses/README.md`,
    `${resourceRoot}/stremio-server/server.js`,
    `${resourceRoot}/stremio-server/launch.cjs`,
    `${resourceRoot}/stremio-server/NOTICE.md`,
  ];
}

export async function verifyWindowsPackageInventory(packageRoot, projectRoot = defaultProjectRoot) {
  const missing = [];
  for (const relative of await requiredWindowsPackageFiles(projectRoot)) {
    try {
      await access(path.join(packageRoot, ...relative.split("/")));
    } catch {
      missing.push(relative);
    }
  }
  if (missing.length > 0) throw new Error(`Windows package is missing:\n${missing.join("\n")}`);
}

const invokedPath = process.argv[1] ? pathToFileURL(path.resolve(process.argv[1])).href : "";
if (import.meta.url === invokedPath) {
  const packageArgument = process.argv.slice(2).find((argument) => argument !== "--");
  const packageRoot = path.resolve(packageArgument ?? path.join(defaultProjectRoot, "out/panorama-win32-x64"));
  await verifyWindowsPackageInventory(packageRoot);
  console.log(`Verified Windows package inventory: ${packageRoot}`);
}
