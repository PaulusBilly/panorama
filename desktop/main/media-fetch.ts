import { net } from "electron";
import { Readable } from "node:stream";

export type MediaFetchResult = { response: Response; finalUrl: string };
export type MediaFetch = (url: string, init: RequestInit) => Promise<MediaFetchResult>;

export function createMediaFetch(): MediaFetch {
  return (url, init) => new Promise((resolve, reject) => {
    const safeUrl = (value: string, base?: string) => {
      const parsed = new URL(value, base);
      if (!["http:", "https:"].includes(parsed.protocol) || parsed.username || parsed.password) throw new Error("Invalid media destination");
      return parsed.href;
    };
    let finalUrl = safeUrl(url);
    let hops = 0;
    const request = net.request({ url: finalUrl, method: init.method ?? "GET", redirect: "manual", cache: "no-store", credentials: init.credentials ?? "same-origin", origin: new URL(finalUrl).origin });
    const cleanup = () => init.signal?.removeEventListener("abort", abort);
    const abort = () => { request.abort(); cleanup(); reject(init.signal?.reason ?? new Error("Media request aborted")); };
    request.on("error", () => { cleanup(); reject(new Error("Media transport failed")); });
    request.on("redirect", (_status, _method, destination) => {
      try {
        if (init.signal?.aborted || ++hops > 10) throw new Error("Media redirect limit exceeded");
        finalUrl = safeUrl(destination, finalUrl);
        request.followRedirect();
      } catch (error) { request.abort(); cleanup(); reject(error); }
    });
    request.on("response", (incoming) => {
      const headers = new Headers();
      for (const [name, values] of Object.entries(incoming.headers)) {
        for (const value of Array.isArray(values) ? values : [values]) if (value !== undefined) headers.append(name, value);
      }
      incoming.on("error", () => undefined);
      incoming.once("end", cleanup);
      (incoming as unknown as Readable).once("close", () => { request.abort(); cleanup(); });
      const body = init.method === "HEAD" || [204, 205, 304].includes(incoming.statusCode) ? null : Readable.toWeb(incoming as unknown as Readable, { strategy: { highWaterMark: 65536, size: (chunk: Uint8Array) => chunk.byteLength } }) as ReadableStream<Uint8Array>;
      if (!body) (incoming as unknown as Readable).resume();
      resolve({ response: new Response(body, { status: incoming.statusCode, headers }), finalUrl });
    });
    new Headers(init.headers).forEach((value, name) => request.setHeader(name, value));
    if (init.signal?.aborted) { abort(); return; }
    init.signal?.addEventListener("abort", abort, { once: true });
    request.end();
  });
}
