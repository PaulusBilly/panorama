export type SpikeDiagnostics = {
  mpvVersion: string | null;
  videoCodec: string | null;
  hardwareDecoder: string | null;
  buffering: boolean;
  cacheSeconds: number | null;
  timeSeconds: number | null;
  durationSeconds: number | null;
  tracks: Array<{
    id: number | null;
    type: string | null;
    language: string | null;
    title: string | null;
    selected: boolean;
    forced: boolean;
    hearingImpaired: boolean;
  }>;
};

export type PanoramaSpike = {
  play(): Promise<void>;
  pause(): Promise<void>;
  getDiagnostics(): Promise<SpikeDiagnostics>;
  setVideoBounds(bounds: {
    visible: boolean;
    x: number;
    y: number;
    width: number;
    height: number;
    scaleFactor: number;
  }): void;
};
