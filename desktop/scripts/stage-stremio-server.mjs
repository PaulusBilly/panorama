import { createHash } from "node:crypto";
import { cp, mkdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const defaultProjectRoot = path.resolve(scriptDirectory, "../..");

function assertImmutableHttps(url) {
  if (url.protocol !== "https:" || /(?:^|[/-])latest(?:[/-]|$)/i.test(url.pathname)) {
    throw new Error("The Stremio server source URL must be immutable HTTPS");
  }
}

export function validateStremioServerManifest(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid Stremio server manifest");
  if (value.schemaVersion !== 1) throw new Error("Unsupported Stremio server manifest");
  if (typeof value.version !== "string" || !/^\d+\.\d+\.\d+$/.test(value.version)) {
    throw new Error("Invalid Stremio server version");
  }
  assertImmutableHttps(new URL(value.source?.url ?? ""));
  if (!/^[a-f0-9]{64}$/i.test(value.source?.sha256 ?? "")) throw new Error("Invalid Stremio server SHA-256");
  return value;
}

async function sha256File(file) {
  try {
    return createHash("sha256").update(await readFile(file)).digest("hex");
  } catch {
    return null;
  }
}

export async function stageStremioServer(targetDirectory, projectRoot = defaultProjectRoot, fetchSource = fetch) {
  const source = path.join(projectRoot, "desktop/stremio-server");
  const manifest = validateStremioServerManifest(JSON.parse(await readFile(path.join(source, "stremio-server.json"), "utf8")));
  const expected = manifest.source.sha256.toLowerCase();
  const cacheDirectory = path.join(projectRoot, ".cache", "panorama", "stremio-server", expected);
  const cached = path.join(cacheDirectory, "server.js");

  if (await sha256File(cached) !== expected) {
    const response = await fetchSource(manifest.source.url, { redirect: "follow" });
    if (!response.ok) throw new Error(`Stremio server download failed: ${response.status}`);
    assertImmutableHttps(new URL(response.url));
    const bytes = Buffer.from(await response.arrayBuffer());
    if (createHash("sha256").update(bytes).digest("hex") !== expected) throw new Error("Stremio server SHA-256 mismatch");
    await mkdir(cacheDirectory, { recursive: true });
    await writeFile(`${cached}.download`, bytes);
    await rename(`${cached}.download`, cached);
  }

  await rm(targetDirectory, { recursive: true, force: true });
  await mkdir(targetDirectory, { recursive: true });
  await cp(cached, path.join(targetDirectory, "server.js"));
  for (const file of ["launch.cjs", "NOTICE.md"]) await cp(path.join(source, file), path.join(targetDirectory, file));
  return manifest;
}

const invokedPath = process.argv[1] ? pathToFileURL(path.resolve(process.argv[1])).href : "";
if (import.meta.url === invokedPath) {
  await stageStremioServer(path.join(defaultProjectRoot, "desktop-resources", "stremio-server"));
}
