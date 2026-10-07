"use client";

import { Checkbox } from "@base-ui/react/checkbox";
import { Select } from "@base-ui/react/select";
import { useId } from "react";
import { IconCheck, IconChevronDown, IconSettings } from "@tabler/icons-react";
import { forwardRef, useEffect, useState } from "react";
import type { PlaybackSettings, PlaybackSettingsState } from "@/desktop/shared/playback-settings";
import type { DiscordSettings } from "@/desktop/shared/discord-presence";
import { PlayerTrackPopover, type PlayerPopoverHandle } from "./PlayerTrackPopover";

function PlayerSelectControl({ label, value, items, disabled, onChange }: { label: string; value: string; items: { value: string; label: string }[]; disabled: boolean; onChange(value: string): void }) {
  const id = useId();
  const [container, setContainer] = useState<HTMLElement | null>(null);
  return <div className="grid gap-2 text-sm">
    <label htmlFor={id}>{label}</label>
    <Select.Root value={value} items={items} disabled={disabled} modal={false} onValueChange={(next) => { if (next !== null) onChange(next); }}>
      <Select.Trigger id={id} ref={(node) => { if (node) setContainer(node.closest<HTMLElement>("[data-player-surface]")); }} className="focus-ring flex min-h-11 w-full items-center justify-between gap-3 rounded-lg border border-player-ink/25 bg-player-canvas px-3 text-left text-sm text-player-ink">
        <Select.Value /><Select.Icon><IconChevronDown size={16} aria-hidden="true" /></Select.Icon>
      </Select.Trigger>
      <Select.Portal container={container}>
        <Select.Positioner alignItemWithTrigger={false} sideOffset={4} collisionPadding={20} className="z-50">
          <Select.Popup className="max-h-[var(--available-height)] min-w-[var(--anchor-width)] overflow-y-auto rounded-lg border border-player-ink/25 bg-player-canvas p-1 text-player-ink shadow-lg" data-player-select="">
            <Select.List>{items.map((item) => <Select.Item key={item.value} value={item.value} className="focus-ring flex min-h-10 cursor-pointer items-center justify-between gap-3 rounded-md px-3 text-sm data-highlighted:bg-player-ink/10"><Select.ItemText>{item.label}</Select.ItemText><Select.ItemIndicator><IconCheck size={16} aria-hidden="true" /></Select.ItemIndicator></Select.Item>)}</Select.List>
          </Select.Popup>
        </Select.Positioner>
      </Select.Portal>
    </Select.Root>
  </div>;
}

export const PlayerSettingsPopover = forwardRef<PlayerPopoverHandle, {
  onOpenChange?(open: boolean): void;
  onWillOpen?(): void;
}>(function PlayerSettingsPopover({ onOpenChange, onWillOpen }, ref) {
  const [settings, setSettings] = useState<PlaybackSettingsState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const [discord, setDiscord] = useState<DiscordSettings | null>(null);
  const [discordBusy, setDiscordBusy] = useState(false);
  const [discordError, setDiscordError] = useState<string | null>(null);
  useEffect(() => {
    if (!open) return;
    let active = true;
    void window.panoramaDesktop?.getDiscordSettings?.().then((state) => {
      if (active) { setDiscord(state); setDiscordError(null); }
    }).catch(() => { if (active) setDiscordError("Discord settings are unavailable. Reopen to retry."); });
    return () => { active = false; };
  }, [open]);
  const updateDiscord = async (enabled: boolean) => {
    setDiscordBusy(true);
    setDiscordError(null);
    try {
      const state = await window.panoramaDesktop?.setDiscordEnabled?.(enabled);
      if (state) setDiscord(state);
    } catch { setDiscordError("Discord setting could not be saved. Try again."); }
    finally { setDiscordBusy(false); }
  };
  useEffect(() => {
    if (!open) return;
    let active = true;
    const refresh = () => window.panoramaDesktop?.getPlaybackSettings?.().then((state) => {
      if (active) setSettings(state);
    }).catch(() => { if (active) setError("Playback settings are unavailable. Reopen to retry."); });
    void refresh();
    const timer = setInterval(() => { if (!busy) void refresh(); }, 1000);
    return () => { active = false; clearInterval(timer); };
  }, [open, busy]);
  const update = async (patch: Partial<PlaybackSettings>) => {
    if (!settings || busy) return;
    setBusy(true);
    setError(null);
    try {
      const result = await window.panoramaDesktop?.setPlaybackSettings?.({ deviceId: settings.deviceId, channels: settings.channels, passthrough: settings.passthrough, video: settings.video, ...patch });
      if (result) setSettings(result);
    } catch { setError("Playback settings could not be applied. Try again."); }
    finally { setBusy(false); }
  };
  const device = settings?.devices.find((entry) => entry.id === settings.deviceId);
  return (
    <PlayerTrackPopover ref={ref} id="player-settings" title="Playback settings" triggerLabel="Playback settings" triggerIcon={<IconSettings size={20} stroke={1.8} />} name="playback-settings" items={[]} selectedId={null} emptyText="" onSelect={() => undefined} busy={busy} error={error} onWillOpen={onWillOpen} onOpenChange={(value) => { setOpen(value); onOpenChange?.(value); }}>
      {settings ? <div className="grid gap-4 px-3 pb-2" onKeyDown={(event) => { if (event.key !== "Escape") event.stopPropagation(); }}>
        <PlayerSelectControl label="Audio output" value={settings.deviceId} disabled={busy} items={settings.devices.map((entry) => ({ value: entry.id, label: entry.label }))} onChange={(value) => void update({ deviceId: value, passthrough: settings.devices.find((entry) => entry.id === value)?.passthroughEnabled ?? false })} />
        <PlayerSelectControl label="Decoded channels" value={settings.channels} disabled={busy} items={[{ value: "auto", label: "Auto" }, { value: "stereo", label: "Stereo" }, { value: "5.1", label: "5.1" }, { value: "7.1", label: "7.1" }]} onChange={(value) => void update({ channels: value as PlaybackSettings["channels"] })} />
        <label className="flex min-h-11 items-center gap-3 text-sm">
          <Checkbox.Root className="focus-ring grid size-4 shrink-0 place-items-center rounded-sm border border-player-ink/50 data-checked:bg-player-ink data-checked:text-player-canvas data-disabled:opacity-50" checked={settings.passthrough} disabled={busy || !device?.passthroughAvailable} onCheckedChange={(checked) => void update({ passthrough: checked })}><Checkbox.Indicator><IconCheck size={13} aria-hidden="true" /></Checkbox.Indicator></Checkbox.Root>
          Receiver passthrough
        </label>
        <p className="type-caption text-player-ink/65">{device?.passthroughAvailable ? "For a compatible receiver. Adjust volume on the receiver." : "Choose an output with confirmed receiver support to enable passthrough."}</p>
        <PlayerSelectControl label="Video output" value={settings.video} disabled={busy} items={[{ value: "auto", label: "Auto" }, { value: "sdr", label: "SDR" }]} onChange={(value) => void update({ video: value as PlaybackSettings["video"] })} />
        <p className="type-caption text-player-ink/65" role="status">{settings.notice ?? (settings.effectivePassthrough ? "Receiver passthrough active" : "Decoded audio")}</p>
      </div> : <p className="type-caption px-3 pb-2" role="status">Loading settings…</p>}
      {(discord || discordError) && <div className="grid gap-2 px-3 pb-2" onKeyDown={(event) => { if (event.key !== "Escape") event.stopPropagation(); }}>
        <label className="flex min-h-11 items-center gap-3 text-sm">
          <Checkbox.Root className="focus-ring grid size-4 shrink-0 place-items-center rounded-sm border border-player-ink/50 data-checked:bg-player-ink data-checked:text-player-canvas data-disabled:opacity-50" checked={discord?.enabled ?? false} disabled={!discord?.available || discordBusy} aria-describedby="discord-sharing-description" onCheckedChange={(checked) => void updateDiscord(checked)}><Checkbox.Indicator><IconCheck size={13} aria-hidden="true" /></Checkbox.Indicator></Checkbox.Root>
          Share watching activity on Discord
        </label>
        <p id="discord-sharing-description" className="type-caption text-player-ink/65">{discord?.available ? "Shows the film title, artwork, and playback timing while Discord is open." : "Discord sharing is unavailable in this build."}</p>
        <p className="type-caption text-player-ink/65" role="status">{discordError}</p>
      </div>}
    </PlayerTrackPopover>
  );
});
