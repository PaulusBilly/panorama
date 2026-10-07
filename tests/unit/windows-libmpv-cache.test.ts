import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { mkdtemp, mkdir, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { expect, it } from "vitest";
import { stageWindowsLibmpv } from "../../desktop/scripts/stage-windows-libmpv.mjs";

const { path7za } = createRequire(import.meta.url)("7zip-bin");

it("repairs incomplete or stale extraction caches from the verified archive", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "panorama-libmpv-"));
  try {
    const source = path.join(root, "source");
    await mkdir(path.join(source, "include/mpv"), { recursive: true });
    await writeFile(path.join(source, "include/mpv/client.h"), "header");
    await writeFile(path.join(source, "libmpv.dll.a"), "rejected");
    const dll = Buffer.alloc(128);
    dll.write("MZ"); dll.writeUInt32LE(64, 0x3c); dll.write("PE\0\0", 64); dll.writeUInt16LE(0x8664, 68);
    await writeFile(path.join(source, "libmpv-2.dll"), dll);
    const archive = path.join(root, "source.7z");
    execFileSync(path7za, ["a", archive, "."], { cwd: source, windowsHide: true, stdio: "pipe" });
    const bytes = await readFile(archive);
    const hash = createHash("sha256").update(bytes).digest("hex");
    const immutableRoot = path.join(root, ".cache/panorama/windows-libmpv", hash);
    await mkdir(immutableRoot, { recursive: true });
    await writeFile(path.join(immutableRoot, "libmpv.7z"), bytes);
    const manifestDirectory = path.join(root, "desktop/native/mpv-host");
    await mkdir(manifestDirectory, { recursive: true });
    await writeFile(path.join(manifestDirectory, "windows-libmpv.json"), JSON.stringify({
      schemaVersion: 1, architecture: "x64", source: { url: "https://example.test/20260814/mpv.7z", sha256: hash },
      files: { headers: ["include/mpv/client.h"], runtime: ["libmpv-2.dll"], rejectedLinkInputs: ["libmpv.dll.a"] },
      noticesDirectory: "licenses",
    }));
    const staged = await stageWindowsLibmpv(root);
    const extractedDll = path.join(immutableRoot, "extracted/libmpv-2.dll");
    await writeFile(extractedDll, "corrupt");
    await stageWindowsLibmpv(root);
    expect(await readFile(extractedDll)).toEqual(dll);
    await writeFile(path.join(immutableRoot, ".complete.json"), JSON.stringify({ schemaVersion: 1, sha256: "0".repeat(64) }));
    await stageWindowsLibmpv(root);
    expect(JSON.parse(await readFile(path.join(immutableRoot, ".complete.json"), "utf8")).sha256).toBe(hash);
    expect(await readFile(path.join(staged.current, "libmpv-2.dll"))).toEqual(dll);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 15_000);
