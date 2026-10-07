import { createHash } from "node:crypto";
import { chmodSync } from "node:fs";
import { access, cp, mkdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const defaultProjectRoot = path.resolve(scriptDirectory, "../..");
const require = createRequire(import.meta.url);
const { path7za: sevenZipPath } = require("7zip-bin");
// 7zip-bin ships its macOS/Linux binaries without the execute bit.
if (process.platform !== "win32") chmodSync(sevenZipPath, 0o755);

function assertSafeManifestPath(value, label) {
  const normalized = value.replaceAll("\\", "/");
  if (
    normalized !== value || normalized.startsWith("/") || /^[a-z]:\//i.test(normalized) ||
    normalized.split("/").some((part) => part === "" || part === "." || part === "..")
  ) throw new Error(`Unsafe libmpv ${label} path`);
}

export function validateWindowsLibmpvManifest(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid libmpv manifest");
  if (value.schemaVersion !== 1 || value.architecture !== "x64") throw new Error("Unsupported libmpv manifest");
  const url = new URL(value.source?.url ?? "");
  if (url.protocol !== "https:" || /(?:^|[/-])latest(?:[/-]|$)/i.test(url.pathname)) {
    throw new Error("The libmpv source URL must be immutable HTTPS");
  }
  if (!/^[a-f0-9]{64}$/i.test(value.source?.sha256 ?? "")) throw new Error("Invalid libmpv SHA-256");
  for (const field of ["headers", "runtime", "rejectedLinkInputs"]) {
    if (!Array.isArray(value.files?.[field]) || value.files[field].some((entry) => typeof entry !== "string" || !entry)) {
      throw new Error(`Invalid libmpv ${field} inventory`);
    }
    for (const entry of value.files[field]) assertSafeManifestPath(entry, field);
  }
  if (value.files.rejectedLinkInputs.some((entry) => !entry.endsWith(".dll.a"))) {
    throw new Error("Unexpected rejected link input");
  }
  if (typeof value.noticesDirectory !== "string" || !value.noticesDirectory) throw new Error("Missing notices directory");
  assertSafeManifestPath(value.noticesDirectory, "notices");
  return value;
}

export function assertSafeArchiveMembers(members) {
  for (const member of members) {
    const normalized = member.replaceAll("\\", "/");
    if (!normalized || normalized.startsWith("/") || /^[a-z]:\//i.test(normalized)) {
      throw new Error("Unsafe archive member");
    }
    const parts = normalized.split("/");
    if (parts.includes("..") || parts.includes("")) throw new Error("Unsafe archive member");
  }
}

export function verifySha256(expected, actual) {
  if (!/^[a-f0-9]{64}$/i.test(expected) || !/^[a-f0-9]{64}$/i.test(actual)) {
    throw new Error("Invalid SHA-256 value");
  }
  if (expected.toLowerCase() !== actual.toLowerCase()) throw new Error("libmpv SHA-256 mismatch");
}

export function assertPeX64(bytes) {
  const data = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes);
  if (data.length < 64 || data[0] !== 0x4d || data[1] !== 0x5a) throw new Error("Invalid Windows PE runtime");
  const peOffset = data.readUInt32LE(0x3c);
  if (
    peOffset < 0x40 || peOffset + 6 > data.length ||
    data[peOffset] !== 0x50 || data[peOffset + 1] !== 0x45 ||
    data[peOffset + 2] !== 0 || data[peOffset + 3] !== 0
  ) throw new Error("Invalid Windows PE runtime");
  if (data.readUInt16LE(peOffset + 4) !== 0x8664) throw new Error("Windows native runtime must be x64");
}

function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: ["ignore", "pipe", "inherit"], ...options });
    let stdout = "";
    child.stdout?.on("data", (chunk) => { stdout += String(chunk); });
    child.once("error", reject);
    child.once("exit", (code) => code === 0 ? resolve(stdout) : reject(new Error(`${command} exited with ${code}`)));
  });
}

async function sha256File(file) {
  const bytes = await readFile(file);
  return createHash("sha256").update(bytes).digest("hex");
}

async function download(url, target) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) throw new Error(`libmpv download failed with HTTP ${response.status}`);
  const finalUrl = new URL(response.url);
  if (finalUrl.protocol !== "https:" || /(?:^|[/-])latest(?:[/-]|$)/i.test(finalUrl.pathname)) {
    throw new Error("The resolved libmpv source URL must be immutable HTTPS");
  }
  const temporary = `${target}.download`;
  await writeFile(temporary, Buffer.from(await response.arrayBuffer()));
  await rename(temporary, target);
}

async function validateExtractedRuntime(extracted, manifest) {
  for (const entry of [...manifest.files.headers, ...manifest.files.rejectedLinkInputs]) {
    await access(path.join(extracted, entry));
  }
  for (const entry of manifest.files.runtime) {
    const runtimePath = path.join(extracted, entry);
    await access(runtimePath);
    assertPeX64(await readFile(runtimePath));
  }
}

async function extractVerifiedArchive(archive, extracted, completed, manifest, hash) {
  const listingOutput = String(await run(sevenZipPath, ["l", "-slt", archive]));
  const listingSection = listingOutput.split("----------").slice(1).join("----------");
  const listing = listingSection.split(/\r?\n/)
    .filter((line) => line.startsWith("Path = "))
    .map((line) => line.slice("Path = ".length).replaceAll("\\", "/"));
  assertSafeArchiveMembers(listing);
  const required = [...manifest.files.headers, ...manifest.files.runtime, ...manifest.files.rejectedLinkInputs];
  for (const entry of required) {
    if (!listing.includes(entry)) throw new Error(`Missing libmpv archive member: ${entry}`);
  }
  await rm(extracted, { recursive: true, force: true });
  await rm(completed, { force: true });
  await mkdir(extracted, { recursive: true });
  await run(sevenZipPath, ["x", "-y", `-o${extracted}`, archive]);
  await validateExtractedRuntime(extracted, manifest);
  await writeFile(completed, `${JSON.stringify({ schemaVersion: 1, sha256: hash }, null, 2)}\n`);
}

export async function stageWindowsLibmpv(projectRoot = defaultProjectRoot) {
  const manifestPath = path.join(projectRoot, "desktop/native/mpv-host/windows-libmpv.json");
  const manifest = validateWindowsLibmpvManifest(JSON.parse(await readFile(manifestPath, "utf8")));
  const hash = manifest.source.sha256.toLowerCase();
  const cacheRoot = path.join(projectRoot, ".cache", "panorama", "windows-libmpv");
  const immutableRoot = path.join(cacheRoot, hash);
  const archive = path.join(immutableRoot, "libmpv.7z");
  const extracted = path.join(immutableRoot, "extracted");
  const current = path.join(cacheRoot, "current");
  const completed = path.join(immutableRoot, ".complete.json");
  await mkdir(immutableRoot, { recursive: true });

  try {
    await access(archive);
  } catch {
    await download(manifest.source.url, archive);
  }
  verifySha256(hash, await sha256File(archive));

  let cacheValid = false;
  try {
    const marker = JSON.parse(await readFile(completed, "utf8"));
    if (marker.schemaVersion !== 1 || marker.sha256 !== hash) throw new Error("Invalid libmpv cache marker");
    await validateExtractedRuntime(extracted, manifest);
    cacheValid = true;
  } catch {
    cacheValid = false;
  }
  if (!cacheValid) await extractVerifiedArchive(archive, extracted, completed, manifest, hash);

  await rm(current, { recursive: true, force: true });
  await mkdir(path.join(current, "include", "mpv"), { recursive: true });
  for (const header of manifest.files.headers) {
    await cp(path.join(extracted, header), path.join(current, header));
  }
  for (const runtime of manifest.files.runtime) {
    await cp(path.join(extracted, runtime), path.join(current, path.basename(runtime)));
  }
  return { manifest, immutableRoot, current };
}

const invokedPath = process.argv[1] ? pathToFileURL(path.resolve(process.argv[1])).href : "";
if (import.meta.url === invokedPath) {
  await stageWindowsLibmpv();
}
