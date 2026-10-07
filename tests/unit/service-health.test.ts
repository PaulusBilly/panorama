import { afterEach, describe, expect, it, vi } from "vitest";
import {
  isFreshServiceHealth,
  SERVICE_HEALTH_FRESHNESS_MS,
  StremioCoreRuntime,
} from "@/runtime/stremio-core-runtime";
import { initialRuntimeSnapshot } from "@/runtime/snapshot";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("service health", () => {
  it("reuses only a recent online result for the same endpoint", () => {
    const now = 50_000;
    expect(isFreshServiceHealth(
      "online",
      "http://127.0.0.1:11470",
      "http://127.0.0.1:11470",
      now - SERVICE_HEALTH_FRESHNESS_MS + 1,
      now,
    )).toBe(true);
    expect(isFreshServiceHealth(
      "online",
      "http://127.0.0.1:11470",
      "http://127.0.0.1:11470",
      now - SERVICE_HEALTH_FRESHNESS_MS,
      now,
    )).toBe(false);
    expect(isFreshServiceHealth(
      "online",
      "http://127.0.0.1:11470",
      "http://127.0.0.1:12470",
      now,
      now,
    )).toBe(false);
  });

  it("checks the CORS-enabled service stats endpoint", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const runtime = new StremioCoreRuntime();

    await expect(runtime.checkService()).resolves.toBe("online");
    expect(fetchMock).toHaveBeenCalledWith("http://127.0.0.1:11470/stats.json", {
      signal: expect.any(AbortSignal),
    });
  });

  it("discovers a standalone service that moved to the next bounded loopback port", async () => {
    const fetchMock = vi.fn((input: string | URL | Request) => {
      const url = String(input);
      return Promise.resolve(new Response("{}", { status: url.includes(":11471/") ? 200 : 503 }));
    });
    vi.stubGlobal("fetch", fetchMock);

    const runtime = new StremioCoreRuntime();

    await expect(runtime.checkService()).resolves.toBe("online");
    expect(runtime.getSnapshot().service.endpoint).toBe("http://127.0.0.1:11471");
    expect(fetchMock).toHaveBeenCalledWith("http://127.0.0.1:11471/stats.json", {
      signal: expect.any(AbortSignal),
    });
  });

  it("does not tear down preparation after one transient health timeout", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("timeout")));
    const runtime = new StremioCoreRuntime() as unknown as {
      snapshot: typeof initialRuntimeSnapshot;
      playerGeneration: number;
      checkPreparationService(generation: number): Promise<void>;
    };
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      service: { status: "online", endpoint: "http://127.0.0.1:11470" },
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status: "preparing",
        stage: "loadingVideo",
        sourceId: "source-2",
        time: 1078,
      },
    };
    runtime.playerGeneration = 1;

    await runtime.checkPreparationService(1);

    expect(runtime.snapshot.service.status).toBe("online");
    expect(runtime.snapshot.player).toMatchObject({ status: "preparing", time: 1078 });
  });

  it("reports a sustained preparation health outage", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("timeout")));
    const runtime = new StremioCoreRuntime() as unknown as {
      snapshot: typeof initialRuntimeSnapshot;
      playerGeneration: number;
      checkPreparationService(generation: number): Promise<void>;
    };
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      service: { status: "online", endpoint: "http://127.0.0.1:11470" },
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status: "preparing",
        stage: "loadingVideo",
        sourceId: "source-2",
        time: 1078,
      },
    };
    runtime.playerGeneration = 1;

    await runtime.checkPreparationService(1);
    await runtime.checkPreparationService(1);
    await runtime.checkPreparationService(1);

    expect(runtime.snapshot.service.status).toBe("offline");
    expect(runtime.snapshot.player).toMatchObject({
      status: "error",
      time: 1078,
      error: "Stremio Service is offline.",
    });
  });
});
