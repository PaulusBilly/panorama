import { createHash, randomBytes } from "node:crypto";
import { mkdir, mkdtemp, readdir, readFile, rm, statfs, writeFile, lstat } from "node:fs/promises";
import path from "node:path";

export type MediaCacheOptions = { directory: string; maxMemoryBytes: number; maxDiskBytes: number; reserveFreeBytes: number };
export type MediaCache = {
  get(key: string): Promise<Buffer | null>;
  has(key: string): boolean;
  put(key: string, bytes: Buffer): Promise<void>;
  pin(key: string): void;
  unpin(key: string): void;
  reserve(key: string, bytes: number): boolean;
  release(key: string): void;
  removeSession(prefix: string): Promise<void>;
  stats(): { memoryBytes: number; diskBytes: number; reservedBytes: number };
  close(): Promise<void>;
};

type Entry = { data: Buffer | null; file: string | null; bytes: number; allocated: number };

export async function createMediaCache(options: MediaCacheOptions): Promise<MediaCache> {
  let directory: string | null = null;
  try {
    await mkdir(options.directory, { recursive: true, mode: 0o700 });
    for (const name of await readdir(options.directory)) {
      const match = /^owner-(\d+)-[a-f0-9]+-/.exec(name);
      if (!match || Number(match[1]) === process.pid) continue;
      try { process.kill(Number(match[1]), 0); } catch (error) {
        if ((error as NodeJS.ErrnoException).code === "ESRCH") await rm(path.join(options.directory, name), { recursive: true, force: true });
      }
    }
    directory = await mkdtemp(path.join(options.directory, `owner-${process.pid}-${randomBytes(8).toString("hex")}-`));
  } catch { directory = null; }
  const entries = new Map<string, Entry>();
  const pins = new Map<string, number>();
  const retired = new Set<string>();
  const reservations = new Map<string, number>();
  let memoryBytes = 0;
  let diskBytes = 4096;
  let reservedBytes = 0;
  let closed = false;
  let queue = Promise.resolve();
  const touch = (key: string, entry: Entry) => { entries.delete(key); entries.set(key, entry); };
  const trimMemory = (needed: number) => {
    for (const [key, entry] of entries) {
      if (memoryBytes + reservedBytes + needed <= options.maxMemoryBytes) break;
      if (!entry.data || pins.has(key)) continue;
      memoryBytes -= entry.bytes;
      entry.data = null;
      if (!entry.file) entries.delete(key);
    }
    return memoryBytes + reservedBytes + needed <= options.maxMemoryBytes;
  };
  const exclusive = <T>(work: () => Promise<T>): Promise<T> => {
    const operation = queue.then(work);
    queue = operation.then(() => undefined, () => undefined);
    return operation;
  };
  const remove = async (key: string, entry: Entry) => {
    if (entries.get(key) !== entry) return;
    if (entry.file) { await rm(entry.file, { force: true }); diskBytes -= entry.allocated; }
    if (entry.data) memoryBytes -= entry.bytes;
    entries.delete(key);
  };
  return {
    has(key) { return !closed && entries.has(key); },
    async get(key) {
        if (closed) return null;
        const entry = entries.get(key);
        if (!entry) return null;
        touch(key, entry);
        if (entry.data) return entry.data;
        if (!entry.file) return null;
        if (!reservations.has(key) && !trimMemory(entry.bytes)) return null;
        try { return await readFile(entry.file); } catch { void exclusive(() => remove(key, entry)).catch(() => undefined); return null; }
    },
    put(key, bytes) {
      const reservation = `write:${key}`;
      if (closed || reservations.has(reservation) || !trimMemory(bytes.length)) return Promise.reject(new Error("Media cache admission failed"));
      reservations.set(reservation, bytes.length);
      reservedBytes += bytes.length;
      return exclusive(async () => {
        if (closed) throw new Error("Media cache closed");
        if (entries.has(key)) return;
        const entry: Entry = { bytes: bytes.length, data: null, file: null, allocated: Math.ceil(bytes.length / 4096) * 4096 + 4096 };
        if (directory) {
          try {
            const volume = await statfs(directory);
            entry.allocated = Math.ceil(bytes.length / Number(volume.bsize)) * Number(volume.bsize) + Number(volume.bsize);
            const free = Number(volume.bavail) * Number(volume.bsize);
            if (free - entry.allocated >= options.reserveFreeBytes) {
              for (const [oldKey, oldEntry] of entries) {
                if (diskBytes + entry.allocated <= options.maxDiskBytes) break;
                if (!pins.has(oldKey)) await remove(oldKey, oldEntry);
              }
              if (diskBytes + entry.allocated <= options.maxDiskBytes) {
                const file = path.join(directory, createHash("sha256").update(key).digest("hex"));
                diskBytes += entry.allocated;
                try {
                  await writeFile(file, bytes, { flag: "wx", mode: 0o600 });
                  const metadata = await lstat(file);
                  const actual = Math.max(entry.allocated, metadata.blocks * 512 + 4096);
                  if (diskBytes - entry.allocated + actual > options.maxDiskBytes) throw new Error("Cache allocation exceeds budget");
                  diskBytes += actual - entry.allocated;
                  entry.allocated = actual;
                  entry.file = file;
                } catch { diskBytes -= entry.allocated; await rm(file, { force: true }).catch(() => undefined); }
              }
            }
          } catch {}
        }
        if (trimMemory(bytes.length)) { entry.data = bytes; memoryBytes += bytes.length; }
        if (!entry.data && !entry.file) throw new Error("Media cache admission failed");
        entries.set(key, entry);
      }).finally(() => { reservedBytes -= reservations.get(reservation) ?? 0; reservations.delete(reservation); });
    },
    pin(key) { pins.set(key, (pins.get(key) ?? 0) + 1); },
    unpin(key) {
      const count = pins.get(key) ?? 0;
      if (count <= 1) {
        pins.delete(key);
        if ([...retired].some((prefix) => key.startsWith(prefix))) void exclusive(async () => { const entry = entries.get(key); if (entry) await remove(key, entry); }).catch(() => undefined);
      } else pins.set(key, count - 1);
    },
    reserve(key, bytes) {
      if (closed || !Number.isSafeInteger(bytes) || bytes < 0) return false;
      if (reservations.has(key)) return true;
      if (!trimMemory(bytes)) return false;
      reservations.set(key, bytes);
      reservedBytes += bytes;
      return true;
    },
    release(key) { reservedBytes -= reservations.get(key) ?? 0; reservations.delete(key); },
    removeSession(prefix) {
      retired.add(prefix);
      return exclusive(async () => { for (const [key, entry] of entries) if (key.startsWith(prefix) && !pins.has(key)) await remove(key, entry); });
    },
    stats() { return { memoryBytes, diskBytes: directory ? diskBytes : 0, reservedBytes }; },
    async close() {
      closed = true;
      await queue;
      entries.clear(); pins.clear(); retired.clear(); reservations.clear(); memoryBytes = 0; reservedBytes = 0; diskBytes = 0;
      if (directory) await rm(directory, { recursive: true, force: true });
    },
  };
}
