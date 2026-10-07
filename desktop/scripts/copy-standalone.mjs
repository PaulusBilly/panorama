import { access, cp, mkdir, readFile, rm } from "node:fs/promises";
import path from "node:path";
import { stageMacRuntime } from "./stage-macos-libmpv.mjs";
import { stageStremioServer } from "./stage-stremio-server.mjs";

const projectRoot = process.cwd();
const nextDirectory = path.join(projectRoot, ".next-desktop");
const standaloneSource = path.join(nextDirectory, "standalone");
const serverSource = path.join(standaloneSource, "server.js");
const resourceRoot = path.join(projectRoot, "desktop-resources");
const standaloneTarget = path.join(resourceRoot, "standalone");
const nativeSource = path.join(projectRoot, "desktop/native/mpv-host/build/Release/mpv_host.node");
const nativeTarget = path.join(resourceRoot, "native", `${process.platform}-${process.arch}`);

await access(serverSource);
await rm(resourceRoot, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
await mkdir(resourceRoot, { recursive: true });
await cp(standaloneSource, standaloneTarget, { recursive: true });
await cp(path.join(projectRoot, "public"), path.join(standaloneTarget, "public"), { recursive: true });
await mkdir(path.join(standaloneTarget, ".next-desktop"), { recursive: true });
await cp(path.join(nextDirectory, "static"), path.join(standaloneTarget, ".next-desktop", "static"), { recursive: true });
await mkdir(nativeTarget, { recursive: true });
await cp(nativeSource, path.join(nativeTarget, "mpv_host.node"));
await stageStremioServer(path.join(resourceRoot, "stremio-server"), projectRoot);

if (process.platform === "darwin") await stageMacRuntime(nativeTarget, projectRoot);

if (process.platform === "win32") {
  if (process.arch !== "x64") throw new Error("Windows desktop packaging supports x64 only");
  const manifest = JSON.parse(await readFile(
    path.join(projectRoot, "desktop/native/mpv-host/windows-libmpv.json"),
    "utf8",
  ));
  const stagedRuntime = path.join(projectRoot, ".cache/panorama/windows-libmpv/current");
  for (const runtime of manifest.files.runtime) {
    await access(path.join(stagedRuntime, path.basename(runtime)));
    await cp(path.join(stagedRuntime, path.basename(runtime)), path.join(nativeTarget, path.basename(runtime)));
  }
  const notices = path.resolve(projectRoot, manifest.noticesDirectory);
  await access(notices);
  await cp(notices, path.join(nativeTarget, "licenses"), { recursive: true });
}
