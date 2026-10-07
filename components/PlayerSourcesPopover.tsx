"use client";

import { RadioGroup } from "@base-ui/react/radio-group";
import { forwardRef } from "react";
import type { PanoramaSource, PanoramaSourceGroup } from "@/runtime/types";
import {
  PlayerOptionRow,
  PlayerPopoverPanel,
  PlayerSectionLabel,
  type PlayerPopoverHandle,
} from "./PlayerTrackPopover";

export const playerSourcesPopoverId = "player-sources";

function sourceTags(source: PanoramaSource) {
  const tags: string[] = [];
  if (source.quality) tags.push(source.quality === "4k" ? "4K" : "HD");
  if (source.audioChannels) tags.push(source.audioChannels);
  return tags;
}

type Props = {
  groups: PanoramaSourceGroup[];
  currentSourceId: string | null;
  serviceOnline: boolean;
  onSelect(sourceId: string): void;
  onOpenChange?(open: boolean): void;
  onWillOpen?(): void;
};

// The panel is mounted outside the auto-hiding controls so error and ended states can open it too;
// its trigger lives in the control bar and shares the player panel surface through context.
export const PlayerSourcesPopover = forwardRef<PlayerPopoverHandle, Props>(function PlayerSourcesPopover(
  { groups, currentSourceId, serviceOnline, onSelect, onOpenChange, onWillOpen },
  ref,
) {
  return (
    <PlayerPopoverPanel
      ref={ref}
      id={playerSourcesPopoverId}
      title="Sources"
      width="min(26rem,calc(100vw - 1rem))"
      onOpenChange={onOpenChange}
      onWillOpen={onWillOpen}
    >
      {groups.length === 0 ? (
        <p className="type-caption px-3 pb-2 text-player-ink/55">No sources available.</p>
      ) : null}
      <RadioGroup name="player-source" value={currentSourceId ?? ""} onValueChange={(value) => onSelect(String(value))}>
      {groups.map((group) => {
        const labelId = `${playerSourcesPopoverId}-${group.id}`;
        return (
          <section className="pb-1" key={group.id} aria-labelledby={labelId}>
            <PlayerSectionLabel id={labelId} className="sticky top-9 z-10 bg-player-canvas pt-3">
              {group.addonName}
            </PlayerSectionLabel>
            {group.status === "loading" && group.items.length === 0 ? (
              <p className="type-caption px-3 py-2 text-player-ink/55" role="status">Loading sources…</p>
            ) : group.status === "error" && group.items.length === 0 ? (
              <p className="type-caption px-3 py-2 text-danger">{group.error ?? "Sources could not be loaded."}</p>
            ) : group.items.length === 0 ? (
              <p className="type-caption px-3 py-2 text-player-ink/55">No sources from this addon.</p>
            ) : (
              <fieldset className="m-0 border-0 p-0">
                <legend className="sr-only">{`Sources from ${group.addonName}`}</legend>
                {group.items.map((source) => {
                  const current = source.id === currentSourceId;
                  const playable = source.playbackSupport === "internal" && serviceOnline;
                  return (
                    <PlayerOptionRow
                      key={source.id}
                      value={source.id}
                      label={source.name}
                      description={source.unavailableReason ?? source.description}
                      tags={sourceTags(source)}
                      selected={current}
                      disabled={!current && !playable}
                      multiline
                    />
                  );
                })}
              </fieldset>
            )}
          </section>
        );
      })}
      </RadioGroup>
    </PlayerPopoverPanel>
  );
});
