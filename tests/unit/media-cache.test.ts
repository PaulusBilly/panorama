import { mkdtemp, writeFile, rm, readdir, chmod } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createMediaCache, type MediaCache } from "../../desktop/main/media-cache";

let directory: string;
let cache: MediaCache | null = null;
afterEach(async () => { await cache?.close(); cache = null; if (directory) await rm(directory, { recursive: true, force: true }); });
async function memoryCache() {
  directory = await mkdtemp(path.join(tmpdir(), "panorama-cache-test-"));
  const file = path.join(directory, "not-a-directory");
  await writeFile(file, "fixture");
  return createMediaCache({ directory: file, maxMemoryBytes: 16, maxDiskBytes: 16384, reserveFreeBytes: 0 });
}
describe("bounded media cache", () => {
  it("reserves memory before admission and evicts unpinned completed entries", async () => {
    cache = await memoryCache();
    expect(cache.reserve("inflight", 16)).toBe(true);
    expect(cache.reserve("overflow", 1)).toBe(false);
    expect(cache.stats().reservedBytes).toBe(16);
    cache.release("inflight");
    await cache.put("a", Buffer.alloc(8, 1));
    await cache.put("b", Buffer.alloc(8, 2));
    expect(await cache.get("a")).toBeNull();
    expect(await cache.get("b")).toEqual(Buffer.alloc(8, 2));
    expect(cache.stats().memoryBytes + cache.stats().reservedBytes).toBeLessThanOrEqual(16);
  });
  it("preserves reference-counted pins and rejects writes under pressure", async () => {
    cache = await memoryCache();
    await cache.put("a", Buffer.alloc(8));
    cache.pin("a"); cache.pin("a");
    await expect(cache.put("b", Buffer.alloc(8))).rejects.toThrow("admission");
    cache.unpin("a");
    expect(cache.reserve("pressure", 9)).toBe(false);
    cache.unpin("a");
    expect(cache.reserve("pressure", 9)).toBe(true);
    cache.release("pressure");
  });
  it("removes a retired session after its final reader releases the pin", async () => {
    cache = await memoryCache();
    await cache.put("session:a", Buffer.alloc(8));
    cache.pin("session:a");
    await cache.removeSession("session:");
    expect(cache.has("session:a")).toBe(true);
    cache.unpin("session:a");
    await cache.removeSession("session:");
    expect(cache.has("session:a")).toBe(false);
    expect(cache.stats().memoryBytes).toBe(0);
  });
  it("caps allocated disk bytes and removes only its owned directory", async () => {
    directory = await mkdtemp(path.join(tmpdir(), "panorama-cache-test-"));
    await writeFile(path.join(directory, "foreign"), "keep");
    cache = await createMediaCache({ directory, maxMemoryBytes: 16384, maxDiskBytes: 20480, reserveFreeBytes: 0 });
    for (let index = 0; index < 5; index++) await cache.put(String(index), Buffer.alloc(4096, index));
    expect(cache.stats().diskBytes).toBeLessThanOrEqual(20480);
    await cache.close(); cache = null;
    expect(await readdir(directory)).toEqual(["foreign"]);
  });
  it.skipIf(process.platform === "win32")("keeps validated bytes in bounded memory when its disk write fails", async () => {
    directory = await mkdtemp(path.join(tmpdir(), "panorama-cache-test-"));
    cache = await createMediaCache({ directory, maxMemoryBytes: 16384, maxDiskBytes: 20480, reserveFreeBytes: 0 });
    const owned = path.join(directory, (await readdir(directory))[0]);
    await chmod(owned, 0o500);
    try {
      await cache.put("fallback", Buffer.alloc(4096, 7));
      expect(await cache.get("fallback")).toEqual(Buffer.alloc(4096, 7));
      expect(cache.stats()).toEqual({ memoryBytes: 4096, diskBytes: 4096, reservedBytes: 0 });
    } finally { await chmod(owned, 0o700); }
  });
  it("preserves the free-space reserve without losing foreground bytes", async () => {
    directory = await mkdtemp(path.join(tmpdir(), "panorama-cache-test-"));
    cache = await createMediaCache({ directory, maxMemoryBytes: 16384, maxDiskBytes: 20480, reserveFreeBytes: Number.MAX_SAFE_INTEGER });
    await cache.put("foreground", Buffer.alloc(4096, 3));
    expect(await cache.get("foreground")).toEqual(Buffer.alloc(4096, 3));
    expect(cache.stats().diskBytes).toBe(4096);
    expect(cache.stats().reservedBytes).toBe(0);
  });
});
