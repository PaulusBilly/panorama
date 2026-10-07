"use client";

import { Radio } from "@base-ui/react/radio";
import { Input } from "@base-ui/react/input";
import { RadioGroup } from "@base-ui/react/radio-group";
import { Tabs } from "@base-ui/react/tabs";
import { IconCheck } from "@tabler/icons-react";

import { forwardRef, useMemo, useRef, useState, type ReactNode } from "react";
import {
  subtitleLanguageId,
  subtitleLanguageName,
  type SubtitleLanguageId,
} from "@/runtime/normalize";
import { subtitleAppearanceCss } from "@/runtime/subtitle-appearance";
import type { PanoramaSubtitleStyle, PanoramaSubtitleTrack } from "@/runtime/types";
import {
  PlayerOptionRow,
  PlayerPopoverPanel,
  PlayerPopoverTrigger,
  PlayerSectionLabel,
  PlayerStepperControl,
  type PlayerPopoverHandle,
  type PlayerTrackStepper,
} from "./PlayerTrackPopover";

type LanguageChoice = "off" | SubtitleLanguageId;
type NarrowTab = "languages" | "variants" | "settings";
type FontWeight = "regular" | "medium" | "semibold" | "bold";

const fontWeights: Array<{ id: FontWeight; className: string }> = [
  { id: "regular", className: "font-normal" },
  { id: "medium", className: "font-medium" },
  { id: "bold", className: "font-bold" },
];

const textColors = [
  { value: "#ffffff", label: "White" },
  { value: "#ffe066", label: "Yellow" },
  { value: "#a3e6a1", label: "Green" },
  { value: "#8de4ff", label: "Blue" },
  { value: "#ffb5d5", label: "Pink" },
];

const sections: Array<[NarrowTab, string]> = [
  ["languages", "Language"],
  ["variants", "Track"],
  ["settings", "Appearance"],
];

type Props = {
  id: string;
  triggerLabel: string;
  triggerIcon: ReactNode;
  triggerActiveIcon?: ReactNode;
  tracks: PanoramaSubtitleTrack[];
  selectedId: string | null;
  emptyText: string;
  error?: string | null;
  busy?: boolean;
  steppers: PlayerTrackStepper[];
  fontWeight: FontWeight;
  appearance?: PanoramaSubtitleStyle;
  appearanceLimitation?: string | null;
  onTextColorChange?(color: string): void;
  onFontWeightChange(weight: FontWeight): void;
  onSelect(id: string | null): void;
  onOpenChange?(open: boolean): void;
  onWillOpen?(): void;
};

export const PlayerSubtitlesPopover = forwardRef<PlayerPopoverHandle, Props>(function PlayerSubtitlesPopover(
  {
    id,
    triggerLabel,
    triggerIcon,
    triggerActiveIcon,
    tracks,
    selectedId,
    emptyText,
    error,
    busy,
    steppers,
    fontWeight,
    appearance,
    appearanceLimitation,
    onTextColorChange,
    onFontWeightChange,
    onSelect,
    onOpenChange,
    onWillOpen,
  },
  ref,
) {
  const [languageChoice, setLanguageChoice] = useState<LanguageChoice | null>(null);
  const [narrowTab, setNarrowTab] = useState<NarrowTab>("languages");
  // ponytail: session-only memory of the last track per language; resets with the player.
  const lastTrackByLanguage = useRef<Partial<Record<SubtitleLanguageId, string>>>({});
  const panelId = (tab: NarrowTab) => `${id}-${tab}`;

  const languageOptions = useMemo(() => {
    const present = new Set(
      tracks.flatMap((track) => {
        const language = subtitleLanguageId(track.language, track.label);
        return language ? [language] : [];
      }),
    );
    const items: Array<{ id: LanguageChoice; label: string }> = [{ id: "off", label: "Off" }];
    if (present.has("en")) items.push({ id: "en", label: subtitleLanguageName("en") });
    if (present.has("id")) items.push({ id: "id", label: subtitleLanguageName("id") });
    return items;
  }, [tracks]);

  const selectedTrack = tracks.find((track) => track.id === selectedId) ?? null;
  const selectedLanguage = selectedTrack
    ? subtitleLanguageId(selectedTrack.language, selectedTrack.label)
    : null;
  const activeLanguage: LanguageChoice = languageChoice ?? selectedLanguage ?? "off";
  const variants = tracks.filter((track) => subtitleLanguageId(track.language, track.label) === activeLanguage);

  // Addon tracks often share a source name, so repeat names get a number to stay distinguishable.
  const variantRows = (() => {
    const totals = new Map<string, number>();
    for (const track of variants) totals.set(track.sourceLabel, (totals.get(track.sourceLabel) ?? 0) + 1);
    const seen = new Map<string, number>();
    return variants.map((track) => {
      const index = (seen.get(track.sourceLabel) ?? 0) + 1;
      seen.set(track.sourceLabel, index);
      const languageName = activeLanguage === "off" ? "" : subtitleLanguageName(activeLanguage);
      return {
        track,
        label: (totals.get(track.sourceLabel) ?? 0) > 1 ? `${track.sourceLabel} · ${index}` : track.sourceLabel,
        description: track.label && track.label !== languageName ? track.label : null,
      };
    });
  })();

  const chooseLanguage = (language: LanguageChoice) => {
    if (selectedTrack && selectedLanguage) lastTrackByLanguage.current[selectedLanguage] = selectedTrack.id;
    setLanguageChoice(language);
    if (language === "off") {
      onSelect(null);
      return;
    }
    if (language === selectedLanguage) return;
    const remembered = lastTrackByLanguage.current[language];
    const candidates = tracks.filter((track) => subtitleLanguageId(track.language, track.label) === language);
    const next = candidates.find((track) => track.id === remembered) ?? candidates[0];
    if (next) onSelect(next.id);
  };

  const showSection = (tab: NarrowTab) =>
    `${narrowTab === tab ? "block" : "hidden"} @min-[36rem]:block`;

  return (
    <>
      <PlayerPopoverTrigger id={id} label={triggerLabel} icon={triggerIcon} activeIcon={triggerActiveIcon} />
      <PlayerPopoverPanel
        ref={ref}
        id={id}
        title="Subtitles"
        width="min(46rem,calc(100vw - 1rem))"
        busy={busy}
        error={error}
        onWillOpen={onWillOpen}
        onOpenChange={onOpenChange}
      >
        <Tabs.Root className="flex min-h-0 flex-col" value={narrowTab} onValueChange={(value) => setNarrowTab(value as NarrowTab)}>
        <Tabs.List activateOnFocus className="mb-1 flex gap-1 rounded-xl bg-player-ink/10 p-1 @min-[36rem]:hidden" aria-label="Subtitles">
          {sections.map(([tab, label]) => (
            <Tabs.Tab key={tab} value={tab} id={`${id}-tab-${tab}`} aria-controls={panelId(tab)} className={`focus-ring min-h-10 flex-1 rounded-lg border-0 px-2 text-sm transition-colors duration-fast ${narrowTab === tab ? "bg-player-ink/15" : "bg-transparent text-player-ink/70"}`}>
              {label}
            </Tabs.Tab>
          ))}
        </Tabs.List>
        {/* Wide layout: the panel stops scrolling as a whole and each column scrolls on its own. */}
        <div className="grid @min-[36rem]:min-h-0 @min-[36rem]:grid-cols-[minmax(0,0.8fr)_minmax(0,1.1fr)_minmax(0,1.1fr)] @min-[36rem]:grid-rows-[minmax(0,1fr)] @min-[36rem]:divide-x @min-[36rem]:divide-player-ink/10">
          <Tabs.Panel render={<section />} keepMounted hidden={false} inert={false} value="languages"
            className={`${showSection("languages")} min-w-0 @min-[36rem]:overflow-y-auto @min-[36rem]:overscroll-contain @min-[36rem]:pr-1`}
            role="tabpanel"
            id={panelId("languages")}
            aria-labelledby={`${id}-tab-languages`}
          >
            <PlayerSectionLabel className="sticky top-0 z-10 hidden bg-player-canvas @min-[36rem]:block">Language</PlayerSectionLabel>
            <RadioGroup name={`${id}-language`} value={activeLanguage} onValueChange={(value) => chooseLanguage(value as LanguageChoice)} render={<fieldset />} className="m-0 border-0 p-0">
              <legend className="sr-only">Language</legend>
              {languageOptions.map((item) => (
                <PlayerOptionRow
                  key={item.id}
                  value={item.id}
                  label={item.label}
                  selected={activeLanguage === item.id}
                />
              ))}
            </RadioGroup>
          </Tabs.Panel>
          <Tabs.Panel render={<section />} keepMounted hidden={false} inert={false} value="variants"
            className={`${showSection("variants")} min-w-0 @min-[36rem]:overflow-y-auto @min-[36rem]:overscroll-contain @min-[36rem]:px-1`}
            role="tabpanel"
            id={panelId("variants")}
            aria-labelledby={`${id}-tab-variants`}
          >
            <PlayerSectionLabel className="sticky top-0 z-10 hidden bg-player-canvas @min-[36rem]:block">Track</PlayerSectionLabel>
            {activeLanguage === "off" ? (
              <p className="type-caption px-3 py-3 text-player-ink/55">Subtitles are off.</p>
            ) : variantRows.length === 0 ? (
              <p className="type-caption px-3 py-3 text-player-ink/55">{emptyText}</p>
            ) : (
              <RadioGroup name={`${id}-variant`} value={selectedId ?? ""} onValueChange={(value) => { lastTrackByLanguage.current[activeLanguage] = String(value); onSelect(String(value)); }} render={<fieldset />} className="m-0 border-0 p-0">
                <legend className="sr-only">Track</legend>
                {variantRows.map(({ track, label, description }) => (
                  <PlayerOptionRow
                    key={track.id}
                    value={track.id}
                    label={label}
                    description={description}
                    selected={track.id === selectedId}
                  />
                ))}
              </RadioGroup>
            )}
          </Tabs.Panel>
          <Tabs.Panel render={<section />} keepMounted hidden={false} inert={false} value="settings"
            className={`${showSection("settings")} min-w-0 @min-[36rem]:overflow-y-auto @min-[36rem]:overscroll-contain @min-[36rem]:pl-1`}
            role="tabpanel"
            id={panelId("settings")}
            aria-labelledby={`${id}-tab-settings`}
          >
            <PlayerSectionLabel className="sticky top-0 z-10 hidden bg-player-canvas @min-[36rem]:block">Appearance</PlayerSectionLabel>
            <div className="grid gap-4 px-3 pb-2 pt-1">
              {appearanceLimitation ? <p className="type-caption text-player-ink/70">{appearanceLimitation}</p> : null}
              {appearance && !appearanceLimitation ? (
                <div className="min-w-0 text-center" aria-label="Subtitle appearance sample">
                  <span className="type-caption mb-2 block text-player-ink/55">Sample</span>
                  <div className="inline-block max-w-full" style={subtitleAppearanceCss(appearance)} aria-hidden="true">♪ A song begins ♪</div>
                </div>
              ) : null}
              {appearance && onTextColorChange ? (
                <fieldset className="m-0 grid gap-2 border-0 p-0" disabled={Boolean(appearanceLimitation)}>
                  <legend className="type-caption mb-2 text-player-ink/55">Text color</legend>
                  <RadioGroup name={`${id}-text-color`} aria-label="Text color" value={appearance.textColor.toLowerCase()} disabled={Boolean(appearanceLimitation)} onValueChange={(value) => onTextColorChange(String(value))} className="grid grid-cols-5 gap-1">
                    {textColors.map(({ value, label }) => (
                      <Radio.Root key={value} value={value} aria-label={label} title={label} className="focus-ring grid min-h-11 cursor-pointer place-items-center rounded-lg bg-player-ink/10 active:scale-[0.96] data-checked:bg-player-ink/20 data-disabled:cursor-default data-disabled:opacity-40">
                        <span className="grid size-6 place-items-center rounded-full" style={{ backgroundColor: value }} aria-hidden="true">
                          <Radio.Indicator className="text-player-canvas"><IconCheck size={16} stroke={2} /></Radio.Indicator>
                        </span>
                      </Radio.Root>
                    ))}
                  </RadioGroup>
                  <label className="relative flex min-h-11 cursor-pointer items-center gap-3 rounded-full bg-player-ink/10 px-3 text-sm focus-within:outline-2 focus-within:-outline-offset-2 focus-within:outline-player-ink has-disabled:cursor-default has-disabled:opacity-40">
                    <span className="size-5 shrink-0 rounded-full border border-player-ink/25" style={{ backgroundColor: appearance.textColor }} aria-hidden="true" />
                    Choose color
                    <Input type="color" aria-label="Custom text color" value={/^#[\da-f]{6}$/i.test(appearance.textColor) ? appearance.textColor : "#ffffff"} onChange={(event) => onTextColorChange(event.target.value)} className="absolute inset-0 size-full cursor-pointer opacity-0" />
                  </label>
                </fieldset>
              ) : null}
              {steppers.map((stepper) => {
                const stepperId = `${id}-${stepper.label.toLowerCase().replace(/\s+/g, "-")}`;
                return <PlayerStepperControl id={stepperId} key={stepper.label} stepper={{ ...stepper, disabled: Boolean(appearanceLimitation) && stepper.label !== "Delay" && stepper.label !== "Vertical Position" }} />;
              })}
              <fieldset className="m-0 grid gap-2 border-0 p-0" disabled={Boolean(appearanceLimitation)}>
                <legend className="type-caption mb-2 text-player-ink/55">Font weight</legend>
                <RadioGroup name={`${id}-font-weight`} value={fontWeight} disabled={Boolean(appearanceLimitation)} onValueChange={(value) => onFontWeightChange(value as FontWeight)} className="grid grid-cols-3 gap-1 rounded-xl bg-player-ink/10 p-1">
                  {fontWeights.map(({ id: weight, className }) => (
                    <Radio.Root value={weight}
                      className={`relative grid min-h-9 cursor-pointer place-items-center rounded-lg px-1 text-xs capitalize transition-colors duration-fast focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-player-ink ${className} ${fontWeight === weight ? "bg-player-ink/15" : "text-player-ink/70 hover:bg-player-ink/5"}`}
                      key={weight}
                    >
                      {weight}
                    </Radio.Root>
                  ))}
                </RadioGroup>
              </fieldset>
            </div>
          </Tabs.Panel>
        </div>
        </Tabs.Root>
      </PlayerPopoverPanel>
    </>
  );
});
