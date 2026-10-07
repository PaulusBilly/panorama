import { afterEach, describe, expect, it, vi } from "vitest";
import { discoverService } from "../../runtime/service-discovery";

afterEach(() => vi.useRealTimers());
describe("bounded service discovery", () => {
  it("returns the configured service without waiting for a hanging fallback", async () => {
    let fallbackSignal: AbortSignal | null = null;
    const result = await discoverService(["configured", "fallback"], null, (endpoint, signal) => {
      if (endpoint === "configured") return Promise.resolve(true);
      fallbackSignal = signal;
      return new Promise(() => undefined);
    });
    expect(result).toBe("configured");
    expect((fallbackSignal as unknown as AbortSignal).aborted).toBe(true);
  });
  it("preserves configured priority when a preferred endpoint answers sooner", async () => {
    let configured: (value: boolean) => void = () => undefined;
    const result = discoverService(["configured", "fallback", "preferred"], "preferred", (endpoint) => endpoint === "configured" ? new Promise((resolve) => { configured = resolve; }) : Promise.resolve(true));
    await Promise.resolve();
    configured(false);
    expect(await result).toBe("preferred");
  });
  it("bounds hanging higher-priority candidates to one shared deadline", async () => {
    vi.useFakeTimers();
    const result = discoverService(["hanging", "online"], null, (endpoint) => endpoint === "online" ? Promise.resolve(true) : new Promise(() => undefined));
    await vi.advanceTimersByTimeAsync(2500);
    expect(await result).toBe("online");
    expect(vi.getTimerCount()).toBe(0);
  });
  it("cancels outstanding probes and returns no stale endpoint", async () => {
    const controller = new AbortController();
    const result = discoverService(["service"], null, () => new Promise(() => undefined), controller.signal);
    controller.abort();
    expect(await result).toBeNull();
  });
});
