import type { StremioRuntime } from "./types";

export async function createRuntime(): Promise<StremioRuntime> {
  const mode = process.env.NEXT_PUBLIC_PANORAMA_RUNTIME;
  if (mode === "fake" || mode === "preview") {
    const { FakeRuntime } = await import("./fake-runtime");
    const runtime = new FakeRuntime();
    if (mode === "preview") {
      // Design preview: fixture data that starts signed in with Stremio Service online, so screens
      // like the player open directly. `fake` keeps the signed-out start the e2e suite relies on.
      const initialize = runtime.initialize;
      runtime.initialize = async () => {
        await initialize();
        await runtime.login("preview@panorama.local", "");
        await runtime.checkService();
      };
    }
    return runtime;
  }

  const { StremioCoreRuntime } = await import("./stremio-core-runtime");
  return new StremioCoreRuntime();
}
