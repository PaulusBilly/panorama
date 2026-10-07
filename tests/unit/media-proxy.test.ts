import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MediaProxy } from "../../desktop/main/media-proxy";

const media = Buffer.alloc(9 * 1_048_576 + 123);
for (let index = 0; index < media.length; index += 1) media[index] = index % 251;

let origin: Server | null = null;
let proxy: MediaProxy | null = null;

async function startOrigin(ranges: boolean, onRange?: (header: string) => void): Promise<string> {
  origin = createServer((request, response) => {
    if (request.url === "/redirect") {
      response.writeHead(302, { Location: "/film.mkv" });
      response.end();
      return;
    }
    const header = request.headers.range;
    const match = ranges && header ? /^bytes=(\d+)-(\d*)$/.exec(header) : null;
    if (!match) {
      response.writeHead(200, { "Content-Type": "video/x-matroska", "Content-Length": media.length });
      response.end(request.method === "HEAD" ? undefined : media);
      return;
    }
    onRange?.(header!);
    const start = Number(match[1]);
    const end = match[2] ? Math.min(Number(match[2]), media.length - 1) : media.length - 1;
    response.writeHead(206, { ETag: '"fixture-v1"',
      "Content-Type": "video/x-matroska",
      "Content-Length": end - start + 1,
      "Content-Range": `bytes ${start}-${end}/${media.length}`,
    });
    response.end(media.subarray(start, end + 1));
  });
  await new Promise<void>((resolve) => origin!.listen(0, "127.0.0.1", resolve));
  return `http://127.0.0.1:${(origin.address() as AddressInfo).port}`;
}

afterEach(async () => {
  proxy?.destroy();
  proxy = null;
  await new Promise<void>((resolve) => (origin ? origin.close(() => resolve()) : resolve()));
  origin = null;
});

describe("media proxy", () => {
  it("serves exact byte ranges assembled from parallel upstream requests", async () => {
    const ranges: string[] = [];
    const base = await startOrigin(true, (header) => ranges.push(header));
    proxy = new MediaProxy();
    await proxy.start();
    const url = proxy.open(`${base}/redirect`);

    const whole = Buffer.from(await (await fetch(url)).arrayBuffer());
    expect(whole.equals(media)).toBe(true);

    const middle = await fetch(url, { headers: { Range: "bytes=4194000-6300000" } });
    expect(middle.status).toBe(206);
    expect(middle.headers.get("content-range")).toBe(`bytes 4194000-6300000/${media.length}`);
    expect(Buffer.from(await middle.arrayBuffer()).equals(media.subarray(4_194_000, 6_300_001))).toBe(true);

    expect(ranges.filter((range) => !range.startsWith("bytes=0-")).length).toBeGreaterThan(3);
    expect(proxy.stats().sizeBytes).toBe(media.length);
    expect(proxy.stats().transferDemanded).toBe(false);
    expect(proxy.stats().downloadMbps).toBeNull();
  });

  it("abandons silently stalled upstream requests and still delivers every byte", async () => {
    let held = 0;
    origin = createServer((request, response) => {
      const match = /^bytes=(\d+)-(\d*)$/.exec(request.headers.range ?? "");
      const start = Number(match?.[1] ?? 0);
      const end = match?.[2] ? Math.min(Number(match[2]), media.length - 1) : media.length - 1;
      // Hold the first few data requests open without a response, like a server
      // over its connection limit.
      if (!request.headers.range?.startsWith("bytes=0-") && held < 3) { held += 1; return; }
      response.writeHead(206, { ETag: '"fixture-v1"',
        "Content-Length": end - start + 1,
        "Content-Range": `bytes ${start}-${end}/${media.length}`,
      });
      response.end(media.subarray(start, end + 1));
    });
    await new Promise<void>((resolve) => origin!.listen(0, "127.0.0.1", resolve));
    const base = `http://127.0.0.1:${(origin.address() as AddressInfo).port}`;
    proxy = new MediaProxy(fetch, Date.now, 300);
    await proxy.start();

    const whole = Buffer.from(await (await fetch(proxy.open(`${base}/film.mkv`))).arrayBuffer());
    expect(held).toBe(3);
    expect(whole.equals(media)).toBe(true);
    origin.closeAllConnections();
  });

  it("recovers a throttled probe within the same playback request", async () => {
    let probes = 0;
    origin = createServer((request, response) => {
      if (request.headers.range?.startsWith("bytes=0-")) probes += 1;
      if (probes === 1) { response.writeHead(503); response.end(); return; }
      const match = /^bytes=(\d+)-(\d*)$/.exec(request.headers.range ?? "");
      const start = Number(match?.[1] ?? 0);
      const end = match?.[2] ? Math.min(Number(match[2]), media.length - 1) : media.length - 1;
      response.writeHead(206, { ETag: '"fixture-v1"', "Content-Length": end - start + 1, "Content-Range": `bytes ${start}-${end}/${media.length}` });
      response.end(media.subarray(start, end + 1));
    });
    await new Promise<void>((resolve) => origin!.listen(0, "127.0.0.1", resolve));
    const events: unknown[] = [];
    proxy = new MediaProxy(fetch, Date.now, 300, (event) => events.push(event));
    await proxy.start();
    const url = proxy.open(`http://127.0.0.1:${(origin.address() as AddressInfo).port}/film.mkv`);

    const response = await fetch(url);
    expect(response.status).toBe(200);
    const whole = Buffer.from(await response.arrayBuffer());
    expect(whole.equals(media)).toBe(true);
    expect(probes).toBe(2);
    expect(events).toContainEqual(expect.objectContaining({ event: "probe", result: "ranged" }));
  });

  it("refreshes an expired redirect target once instead of per range", async () => {
    let resolverHits = 0;
    let expired = true;
    origin = createServer((request, response) => {
      if (request.url === "/resolve") {
        resolverHits += 1;
        response.writeHead(302, { Location: resolverHits === 1 ? "/old.mkv" : "/new.mkv" });
        response.end();
        return;
      }
      const match = /^bytes=(\d+)-(\d*)$/.exec(request.headers.range ?? "");
      const start = Number(match?.[1] ?? 0);
      if (request.url === "/old.mkv" && start > 0 && expired) { response.writeHead(403); response.end(); return; }
      const end = match?.[2] ? Math.min(Number(match[2]), media.length - 1) : media.length - 1;
      response.writeHead(206, { ETag: '"fixture-v1"', "Content-Length": end - start + 1, "Content-Range": `bytes ${start}-${end}/${media.length}` });
      response.end(media.subarray(start, end + 1));
    });
    await new Promise<void>((resolve) => origin!.listen(0, "127.0.0.1", resolve));
    proxy = new MediaProxy();
    await proxy.start();
    const url = proxy.open(`http://127.0.0.1:${(origin.address() as AddressInfo).port}/resolve`);

    const whole = Buffer.from(await (await fetch(url)).arrayBuffer());
    expired = false;
    expect(whole.equals(media)).toBe(true);
    expect(resolverHits).toBe(2);
  });

  it("uses one connection when the player has plenty buffered and several when it is short", async () => {
    let open = 0;
    let peak = 0;
    origin = createServer((request, response) => {
      const match = /^bytes=(\d+)-(\d*)$/.exec(request.headers.range ?? "");
      const start = Number(match?.[1] ?? 0);
      const end = match?.[2] ? Math.min(Number(match[2]), media.length - 1) : media.length - 1;
      open += 1;
      peak = Math.max(peak, open);
      setTimeout(() => {
        response.writeHead(206, { ETag: '"fixture-v1"', "Content-Length": end - start + 1, "Content-Range": `bytes ${start}-${end}/${media.length}` });
        response.end(media.subarray(start, end + 1), () => { open -= 1; });
      }, 20);
    });
    await new Promise<void>((resolve) => origin!.listen(0, "127.0.0.1", resolve));
    const url = `http://127.0.0.1:${(origin.address() as AddressInfo).port}/film.mkv`;
    proxy = new MediaProxy();
    await proxy.start();

    const relaxed = proxy.open(url);
    proxy.setReadAhead(300, false);
    expect(Buffer.from(await (await fetch(relaxed)).arrayBuffer()).equals(media)).toBe(true);
    expect(peak).toBe(1);

    peak = 0;
    const urgent = proxy.open(url);
    proxy.setReadAhead(2, true);
    expect(Buffer.from(await (await fetch(urgent)).arrayBuffer()).equals(media)).toBe(true);
    expect(peak).toBeGreaterThan(2);
  });

  it("streams the first bytes before a range completes and resumes a dropped range", async () => {
    const ranges: string[] = [];
    let dropped = false;
    origin = createServer((request, response) => {
      ranges.push(request.headers.range ?? "");
      const match = /^bytes=(\d+)-(\d*)$/.exec(request.headers.range ?? "");
      const start = Number(match?.[1] ?? 0);
      const end = match?.[2] ? Math.min(Number(match[2]), media.length - 1) : media.length - 1;
      response.writeHead(206, { ETag: '"fixture-v1"', "Content-Length": end - start + 1, "Content-Range": `bytes ${start}-${end}/${media.length}` });
      // The first range arrives slowly; a later one drops halfway once.
      if (start === 0) {
        response.write(media.subarray(0, 65_536));
        setTimeout(() => response.end(media.subarray(65_536, end + 1)), 400);
      } else if (start === 2 * 1_048_576 && !dropped) {
        dropped = true;
        response.write(media.subarray(start, start + 1_000_000), () => response.destroy());
      } else {
        response.end(media.subarray(start, end + 1));
      }
    });
    await new Promise<void>((resolve) => origin!.listen(0, "127.0.0.1", resolve));
    proxy = new MediaProxy();
    await proxy.start();
    const url = proxy.open(`http://127.0.0.1:${(origin.address() as AddressInfo).port}/film.mkv`);

    const started = Date.now();
    const response = await fetch(url);
    const reader = response.body!.getReader();
    const first = await reader.read();
    expect(first.value!.length).toBeGreaterThan(0);
    expect(Date.now() - started).toBeLessThan(300);
    const parts: Uint8Array[] = [first.value!];
    for (let next = await reader.read(); !next.done; next = await reader.read()) parts.push(next.value);
    expect(Buffer.concat(parts).equals(media)).toBe(true);
    expect(ranges).toContain(`bytes=${2 * 1_048_576 + 1_000_000}-${4 * 1_048_576 - 1}`);
  });

  it("warms the opening and tail and reuses both when playback opens the same source", async () => {
    const ranges: string[] = [];
    const base = await startOrigin(true, (header) => ranges.push(header));
    proxy = new MediaProxy();
    await proxy.start();

    expect(await proxy.warm(`${base}/film.mkv`)).toEqual({ status: "ready", error: null, unreachable: false });
    await new Promise((resolve) => setTimeout(resolve, 100));
    const warmedRanges = [...ranges];
    expect(warmedRanges.toSorted()).toEqual(["bytes=0-2097151", "bytes=2097152-4194303", "bytes=4194304-6291455", `bytes=8388608-${media.length - 1}`]);

    const url = proxy.open(`${base}/film.mkv`);
    const head = await fetch(url, { headers: { Range: "bytes=0-6291455" } });
    expect(Buffer.from(await head.arrayBuffer()).equals(media.subarray(0, 6_291_456))).toBe(true);
    // Playback continues ahead of the warmed opening without fetching it again.
    const tail = await fetch(url, { headers: { Range: `bytes=8388608-${media.length - 1}` } });
    expect(Buffer.from(await tail.arrayBuffer()).equals(media.subarray(8_388_608))).toBe(true);
    for (const range of warmedRanges) expect(ranges.filter((entry) => entry === range)).toHaveLength(1);
  });

  it("cancels an unfinished tail warm-up without retrying it", async () => {
    const base = await startOrigin(true);
    let tailSignal: AbortSignal | null = null;
    let tailRequests = 0;
    let started: () => void = () => undefined;
    const tailStarted = new Promise<void>((resolve) => { started = resolve; });
    const fetchImpl: typeof fetch = async (input, init) => {
      if (new Headers(init?.headers).get("range")?.startsWith("bytes=8388608-")) {
        tailRequests += 1;
        tailSignal = init!.signal!;
        started();
        return new Promise<Response>((_, reject) => {
          tailSignal!.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true });
        });
      }
      return fetch(input, init);
    };
    proxy = new MediaProxy(fetchImpl);
    await proxy.start();
    expect(await proxy.warm(`${base}/film.mkv`)).toMatchObject({ status: "ready" });
    await tailStarted;
    proxy.destroy();
    expect((tailSignal as AbortSignal | null)?.aborted).toBe(true);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(tailRequests).toBe(1);
  });

  it("cancels a queued tail warm-up within the connection cap", async () => {
    vi.stubEnv("PANORAMA_ADAPTIVE_BUFFERING", "1");
    const ranges: string[] = [];
    const fetchImpl: typeof fetch = async (_, init) => {
      const range = new Headers(init?.headers).get("range")!;
      ranges.push(range);
      const match = /^bytes=(\d+)-(\d+)$/.exec(range)!;
      const start = Number(match[1]);
      const end = Math.min(Number(match[2]), media.length - 1);
      return new Response(new ReadableStream({ start(controller) { controller.enqueue(media.subarray(start, start + 1)); } }), {
        status: 206, headers: { ETag: '"fixture-v1"', "Content-Length": String(end - start + 1), "Content-Range": `bytes ${start}-${end}/${media.length}` },
      });
    };
    try {
      proxy = new MediaProxy(fetchImpl);
      await proxy.start();
      expect(await proxy.warm("https://media.example/film.mkv")).toMatchObject({ status: "ready" });
      await expect.poll(() => ranges.length).toBe(3);
      expect(ranges.some((range) => range.startsWith("bytes=8388608-"))).toBe(false);
      proxy.destroy();
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(ranges).toHaveLength(3);
    } finally { vi.unstubAllEnvs(); }
  });

  it("reuses an in-progress range across a brief gap between player reads", async () => {
    let openingRequests = 0;
    origin = createServer((request, response) => {
      const match = /^bytes=(\d+)-(\d*)$/.exec(request.headers.range ?? "");
      const start = Number(match?.[1] ?? 0);
      const end = match?.[2] ? Math.min(Number(match[2]), media.length - 1) : media.length - 1;
      if (start === 0) openingRequests += 1;
      response.writeHead(206, { ETag: '"fixture-v1"', "Content-Length": end - start + 1, "Content-Range": `bytes ${start}-${end}/${media.length}` });
      response.write(media.subarray(start, Math.min(end + 1, start + 65_536)));
      const timer = setTimeout(() => response.end(media.subarray(start + 65_536, end + 1)), 150);
      response.on("close", () => clearTimeout(timer));
    });
    await new Promise<void>((resolve) => origin!.listen(0, "127.0.0.1", resolve));
    proxy = new MediaProxy();
    await proxy.start();
    const url = proxy.open(`http://127.0.0.1:${(origin.address() as AddressInfo).port}/film.mkv`);
    const first = await fetch(url, { headers: { Range: "bytes=0-65535" } });
    expect(Buffer.from(await first.arrayBuffer()).equals(media.subarray(0, 65_536))).toBe(true);
    await new Promise((resolve) => setTimeout(resolve, 25));
    const next = await fetch(url, { headers: { Range: "bytes=65536-131071" } });
    expect(Buffer.from(await next.arrayBuffer()).equals(media.subarray(65_536, 131_072))).toBe(true);
    expect(openingRequests).toBe(1);
  });

  it("remembers a source whose server refuses connections and fails fast", async () => {
    await startOrigin(true);
    proxy = new MediaProxy();
    await proxy.start();
    const dead = "http://127.0.0.1:1/film.mkv";
    const result = await proxy.warm(dead);
    expect(result).toMatchObject({ status: "failed", unreachable: true });
    expect(result.error).toBeTruthy();

    const started = Date.now();
    expect(await proxy.warm(dead)).toMatchObject({ status: "failed", unreachable: true });
    const playback = await fetch(proxy.open(dead));
    expect(playback.status).toBe(502);
    expect(Date.now() - started).toBeLessThan(500);
    expect(proxy.stats().unreachable).toBe(true);
  });

  it("passes through sources that do not support range requests", async () => {
    const base = await startOrigin(false);
    proxy = new MediaProxy();
    await proxy.start();
    const response = await fetch(proxy.open(`${base}/film.mkv`));
    expect(response.status).toBe(200);
    expect(Buffer.from(await response.arrayBuffer()).equals(media)).toBe(true);
    expect(proxy.stats().sizeBytes).toBeNull();
  });

  it("rejects unknown and replaced sessions", async () => {
    const base = await startOrigin(true);
    proxy = new MediaProxy();
    await proxy.start();
    const first = proxy.open(`${base}/film.mkv`);
    proxy.open(`${base}/film.mkv`);
    expect((await fetch(first)).status).toBe(404);
    expect((await fetch(first.replace(/[0-9a-f]{32}$/, "0".repeat(32)))).status).toBe(404);
  });
});
