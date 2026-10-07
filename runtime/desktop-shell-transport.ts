import type { ShellEventChannel, ShellSendChannel } from "../desktop/shared/mpv-protocol";

export type DesktopShellTransport = {
  send(channel: ShellSendChannel, payload: unknown): void;
  on(channel: ShellEventChannel, listener: (payload: unknown) => void): () => void;
  destroy(): void;
};

const embeddedPlayerProperties = new Set(["osc", "input-default-bindings", "input-vo-keyboard"]);

function embeddedPlayerPayload(channel: ShellSendChannel, payload: unknown): unknown {
  if (
    channel === "mpv-set-prop" &&
    Array.isArray(payload) &&
    payload.length === 2 &&
    typeof payload[0] === "string" &&
    embeddedPlayerProperties.has(payload[0])
  ) {
    return [payload[0], "no"];
  }
  return payload;
}

export function createDesktopShellTransport(): DesktopShellTransport {
  const api = window.panoramaDesktop?.mpv;
  if (!api) throw new Error("Desktop MPV transport is unavailable");
  let destroyed = false;
  const subscriptions = new Set<() => void>();
  return {
    send(channel, payload) {
      if (destroyed) throw new Error("Desktop MPV transport is destroyed");
      api.send(channel, embeddedPlayerPayload(channel, payload));
    },
    on(channel, listener) {
      if (destroyed) throw new Error("Desktop MPV transport is destroyed");
      const unsubscribe = api.on(channel, listener);
      subscriptions.add(unsubscribe);
      return () => {
        if (!subscriptions.delete(unsubscribe)) return;
        unsubscribe();
      };
    },
    destroy() {
      if (destroyed) return;
      destroyed = true;
      for (const unsubscribe of subscriptions) unsubscribe();
      subscriptions.clear();
    },
  };
}
