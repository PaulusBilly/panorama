declare module "@stremio/stremio-core-web/bridge" {
  export default class Bridge {
    constructor(scope: Window, handler: Worker);
    call(path: string[], args: unknown[]): Promise<unknown>;
  }
}

declare module "@stremio/stremio-core-web/worker.js";

interface Window {
  onCoreEvent: ((event: { name: "NewState" | "CoreEvent"; args: unknown }) => void) | null;
}
