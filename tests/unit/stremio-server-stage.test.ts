import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { stageStremioServer, validateStremioServerManifest } from "../../desktop/scripts/stage-stremio-server.mjs";

const temporaryRoots: string[] = [];
const body = Buffer.from("console.log('server');\n");
const sha256 = createHash("sha256").update(body).digest("hex");
const manifest = {
  schemaVersion: 1,
  version: "4.21.2",
  source: { url: "https://example.test/server/v4.21.2/desktop/server.js", sha256 },
};

afterEach(async () => {
  await Promise.all(temporaryRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function createProject(candidate: unknown = manifest): Promise<{ projectRoot: string; target: string }> {
  const projectRoot = await mkdtemp(path.join(os.tmpdir(), "panorama-stremio-stage-"));
  temporaryRoots.push(projectRoot);
  await mkdir(path.join(projectRoot, "desktop/stremio-server"), { recursive: true });
  await writeFile(path.join(projectRoot, "desktop/stremio-server/stremio-server.json"), JSON.stringify(candidate));
  await writeFile(path.join(projectRoot, "desktop/stremio-server/NOTICE.md"), "notice");
  await writeFile(path.join(projectRoot, "desktop/stremio-server/launch.cjs"), "launch");
  return { projectRoot, target: path.join(projectRoot, "desktop-resources/stremio-server") };
}

const respond = (bytes: Buffer, url = manifest.source.url) => vi.fn(async () => ({
  ok: true,
  url,
  arrayBuffer: async () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
}) as unknown as Response);

describe("Stremio server staging", () => {
  it("accepts the pinned manifest", () => {
    expect(validateStremioServerManifest(manifest)).toEqual(manifest);
  });

  it.each([
    null,
    { ...manifest, schemaVersion: 2 },
    { ...manifest, version: "../4.21.2" },
    { ...manifest, source: { ...manifest.source, url: "http://example.test/server/v4.21.2/desktop/server.js" } },
    { ...manifest, source: { ...manifest.source, url: "https://example.test/server/latest/server.js" } },
    { ...manifest, source: { ...manifest.source, sha256: "bad" } },
  ])("rejects unsafe manifest variants", (candidate) => {
    expect(() => validateStremioServerManifest(candidate)).toThrow();
  });

  it("downloads, verifies, and stages the server with its launcher and notice", async () => {
    const { projectRoot, target } = await createProject();
    const fetchSource = respond(body);

    await stageStremioServer(target, projectRoot, fetchSource);

    expect(await readFile(path.join(target, "server.js"))).toEqual(body);
    expect(await readFile(path.join(target, "NOTICE.md"), "utf8")).toBe("notice");
    expect(await readFile(path.join(target, "launch.cjs"), "utf8")).toBe("launch");
    expect(fetchSource).toHaveBeenCalledWith(manifest.source.url, expect.anything());
  });

  it("reuses a verified cached download without fetching again", async () => {
    const { projectRoot, target } = await createProject();
    await stageStremioServer(target, projectRoot, respond(body));
    const second = respond(body);

    await stageStremioServer(target, projectRoot, second);

    expect(second).not.toHaveBeenCalled();
  });

  it("fails on a checksum mismatch and stages nothing", async () => {
    const { projectRoot, target } = await createProject();

    await expect(stageStremioServer(target, projectRoot, respond(Buffer.from("tampered")))).rejects.toThrow("SHA-256 mismatch");

    await expect(readdir(target)).rejects.toThrow();
    // The bad download is discarded so the next build fetches again.
    const retry = respond(body);
    await stageStremioServer(target, projectRoot, retry);
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it("rejects a redirect to a mutable or insecure location", async () => {
    const { projectRoot, target } = await createProject();

    await expect(stageStremioServer(target, projectRoot, respond(body, "http://example.test/server.js"))).rejects.toThrow("immutable HTTPS");
  });
});
