import { afterEach, expect, it, vi } from "vitest";
import { createRuntime } from "../../runtime/create-runtime";

afterEach(() => vi.unstubAllEnvs());

it("starts the preview runtime signed in with Stremio Service online", async () => {
  vi.stubEnv("NEXT_PUBLIC_PANORAMA_RUNTIME", "preview");
  const runtime = await createRuntime();
  await runtime.initialize();
  expect(runtime.getSnapshot().account.status).toBe("signedIn");
  expect(runtime.getSnapshot().service.status).toBe("online");
});

it("keeps the fake runtime signed out for the e2e suite", async () => {
  vi.stubEnv("NEXT_PUBLIC_PANORAMA_RUNTIME", "fake");
  const runtime = await createRuntime();
  await runtime.initialize();
  expect(runtime.getSnapshot().account.status).toBe("loggedOut");
});
