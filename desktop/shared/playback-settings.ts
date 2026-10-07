export type PlaybackSettings = {
  deviceId: string;
  channels: "auto" | "stereo" | "5.1" | "7.1";
  passthrough: boolean;
  video: "auto" | "sdr";
};

export type PlaybackSettingsState = PlaybackSettings & {
  devices: Array<{ id: string; label: string; passthroughAvailable: boolean; passthroughEnabled: boolean }>;
  effectivePassthrough: boolean;
  notice: string | null;
};

export function parsePlaybackSettings(value: unknown): PlaybackSettings {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid playback settings");
  const input = value as Record<string, unknown>;
  if (Object.keys(input).length !== 4 || typeof input.deviceId !== "string" ||
    !/^(default|[a-f0-9-]{36})$/.test(input.deviceId) ||
    !["auto", "stereo", "5.1", "7.1"].includes(input.channels as string) ||
    typeof input.passthrough !== "boolean" || !["auto", "sdr"].includes(input.video as string)) {
    throw new Error("Invalid playback settings");
  }
  return { deviceId: input.deviceId, channels: input.channels as PlaybackSettings["channels"], passthrough: input.passthrough, video: input.video as PlaybackSettings["video"] };
}
