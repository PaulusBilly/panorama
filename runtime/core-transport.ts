import Bridge from "@stremio/stremio-core-web/bridge";
import { installCoreStorage } from "./core-storage";

type CoreEventPayload = {
  name: "NewState" | "CoreEvent";
  args: unknown;
};

export type CoreTransport = {
  init(args: { appVersion: string; shellVersion: null }): Promise<void>;
  getState<T>(model: string): Promise<T>;
  dispatch(action: unknown, model?: string): Promise<void>;
};

export function createCoreTransport(onCoreEvent: (event: CoreEventPayload) => void): {
  transport: CoreTransport;
  worker: Worker;
} {
  installCoreStorage(window);

  const worker = new Worker(
    new URL("@stremio/stremio-core-web/worker.js", import.meta.url),
    { name: "panorama-stremio-core" },
  );
  const bridge = new Bridge(window, worker);

  window.onCoreEvent = onCoreEvent;

  return {
    worker,
    transport: {
      init: async (args) => {
        await bridge.call(["init"], [args]);
      },
      getState: async <T,>(model: string) =>
        (await bridge.call(["getState"], [model])) as T,
      dispatch: async (action, model) => {
        await bridge.call(["dispatch"], [action, model, window.location.hash]);
      },
    },
  };
}
