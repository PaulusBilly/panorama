import { describe, expect, it, vi } from "vitest";
import { StremioCoreRuntime } from "@/runtime/stremio-core-runtime";
import type { RuntimeSnapshot } from "@/runtime/types";

describe("installed addon refresh", () => {
  it("does not reopen the current film when the addon catalog is unchanged", async () => {
    const runtime = new StremioCoreRuntime();
    const internals = runtime as unknown as {
      snapshot: RuntimeSnapshot;
      transport: { getState<T>(model: string): Promise<T> };
      refreshAddons(): Promise<void>;
    };
    internals.snapshot = {
      ...runtime.getSnapshot(),
      details: { ...runtime.getSnapshot().details, filmId: "tmdb:100" },
    };
    internals.transport = {
      getState: async <T,>(model: string) => {
        expect(model).toBe("installed_addons");
        return {
          catalog: [{
            transportUrl: "https://example.com/manifest.json",
            manifest: { id: "org.example.streams", name: "Example Streams" },
          }],
        } as T;
      },
    };
    const open = vi.spyOn(runtime, "openFilmDetails").mockResolvedValue();

    await internals.refreshAddons();
    await internals.refreshAddons();

    expect(open).toHaveBeenCalledTimes(1);
    expect(open).toHaveBeenCalledWith("tmdb:100");
  });
});
