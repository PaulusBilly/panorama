declare module "@stremio/stremio-video" {
  type VideoAction = Record<string, unknown>;
  type VideoOptions = {
    containerElement?: HTMLElement;
    shellTransport?: {
      send(channel: string, payload: unknown): void;
      on(channel: string, listener: (payload: unknown) => void): void;
    };
    mpvSeparateWindow?: boolean;
  };

  export default class StremioVideo {
    on(eventName: string, listener: (...args: unknown[]) => void): void;
    dispatch(action: VideoAction, options?: VideoOptions): void;
    destroy(): void;
  }
}
