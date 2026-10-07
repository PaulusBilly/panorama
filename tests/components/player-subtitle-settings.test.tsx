import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PlayerSubtitlesPopover } from "../../components/PlayerSubtitlesPopover";
import { PlayerPanelHost } from "./player-panel-host";
import { normalizeSubtitleAppearance } from "../../runtime/subtitle-appearance";

describe("player subtitle settings", () => {
  it("selects preset and custom colors and disables them for unsupported tracks", () => {
    const onTextColorChange = vi.fn();
    const props = {
      id: "subtitle-color-test",
      triggerLabel: "Subtitles",
      triggerIcon: <span>CC</span>,
      tracks: [],
      selectedId: null,
      emptyText: "No subtitles",
      steppers: [],
      fontWeight: "regular" as const,
      appearance: normalizeSubtitleAppearance({}),
      onTextColorChange,
      onFontWeightChange: vi.fn(),
      onSelect: vi.fn(),
    };
    const view = render(<PlayerPanelHost><PlayerSubtitlesPopover {...props} /></PlayerPanelHost>);
    fireEvent.click(screen.getByRole("button", { name: "Subtitles" }));
    expect(screen.getByRole("radio", { name: "White" })).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: "Yellow" }));
    expect(onTextColorChange).toHaveBeenLastCalledWith("#ffe066");
    fireEvent.change(screen.getByLabelText("Custom text color"), { target: { value: "#123456" } });
    expect(onTextColorChange).toHaveBeenLastCalledWith("#123456");
    view.rerender(<PlayerPanelHost><PlayerSubtitlesPopover {...props} appearanceLimitation="This track uses image subtitles." /></PlayerPanelHost>);
    expect(screen.getByLabelText("Custom text color")).toBeDisabled();
    expect(screen.getByRole("radio", { name: "Yellow" })).toHaveAttribute("aria-disabled", "true");
  });
  it("edits size inline, clamps commits, and cancels with Escape", () => {
    const commitSize = vi.fn();
    render(
      <PlayerPanelHost>
        <PlayerSubtitlesPopover
          id="subtitle-settings-test"
          triggerLabel="Subtitles"
          triggerIcon={<span>CC</span>}
          tracks={[]}
          selectedId={null}
          emptyText="No subtitles"
          steppers={[{
            label: "Size",
            valueText: "100%",
            decreaseLabel: "Decrease size",
            increaseLabel: "Increase size",
            onDecrease: vi.fn(),
            onIncrease: vi.fn(),
            editable: { value: 100, min: 50, max: 300, suffix: "%", onCommit: commitSize },
          }]}
          fontWeight="medium"
          onFontWeightChange={vi.fn()}
          onSelect={vi.fn()}
          onOpenChange={vi.fn()}
          onWillOpen={vi.fn()}
        />
      </PlayerPanelHost>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Subtitles" }));

    fireEvent.click(screen.getByRole("button", { name: "Edit size" }));
    const input = screen.getByRole("textbox", { name: "Edit size" });
    fireEvent.change(input, { target: { value: "450" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(commitSize).toHaveBeenCalledWith(300);

    fireEvent.click(screen.getByRole("button", { name: "Edit size" }));
    const cancelledInput = screen.getByRole("textbox", { name: "Edit size" });
    fireEvent.change(cancelledInput, { target: { value: "175" } });
    fireEvent.keyDown(cancelledInput, { key: "Escape" });
    expect(commitSize).toHaveBeenCalledTimes(1);
  });

  it("offers the three supplied font weights", () => {
    const onFontWeightChange = vi.fn();
    render(
      <PlayerPanelHost>
        <PlayerSubtitlesPopover
          id="subtitle-weight-test"
          triggerLabel="Subtitles"
          triggerIcon={<span>CC</span>}
          tracks={[]}
          selectedId={null}
          emptyText="No subtitles"
          steppers={[]}
          fontWeight="regular"
          onFontWeightChange={onFontWeightChange}
          onSelect={vi.fn()}
          onOpenChange={vi.fn()}
          onWillOpen={vi.fn()}
        />
      </PlayerPanelHost>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Subtitles" }));

    expect(screen.getByRole("radio", { name: "regular" })).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: "medium" }));
    expect(onFontWeightChange).toHaveBeenCalledWith("medium");
  });

  it("turns subtitles on with one language choice and remembers the track per language", () => {
    const onSelect = vi.fn();
    const tracks = [
      { id: "en-embedded", label: "English", language: "en", origin: "embedded" as const, sourceLabel: "Embedded 1" },
      { id: "en-addon", label: "English", language: "en", origin: "addon" as const, sourceLabel: "OpenSubtitles v3" },
      { id: "id-addon", label: "Bahasa Indonesia", language: "id", origin: "addon" as const, sourceLabel: "OpenSubtitles v3" },
    ];
    const view = render(
      <PlayerPanelHost>
        <PlayerSubtitlesPopover
          id="subtitle-language-test"
          triggerLabel="Subtitles"
          triggerIcon={<span>CC</span>}
          tracks={tracks}
          selectedId={null}
          emptyText="No subtitles"
          steppers={[]}
          fontWeight="regular"
          onFontWeightChange={vi.fn()}
          onSelect={onSelect}
          onOpenChange={vi.fn()}
        />
      </PlayerPanelHost>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Subtitles" }));

    fireEvent.click(screen.getByRole("radio", { name: "English" }));
    expect(onSelect).toHaveBeenLastCalledWith("en-embedded");

    view.rerender(
      <PlayerPanelHost>
        <PlayerSubtitlesPopover
          id="subtitle-language-test"
          triggerLabel="Subtitles"
          triggerIcon={<span>CC</span>}
          tracks={tracks}
          selectedId="en-addon"
          emptyText="No subtitles"
          steppers={[]}
          fontWeight="regular"
          onFontWeightChange={vi.fn()}
          onSelect={onSelect}
          onOpenChange={vi.fn()}
        />
      </PlayerPanelHost>,
    );
    expect(screen.getByRole("radio", { name: "OpenSubtitles v3" })).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: "Indonesian" }));
    expect(onSelect).toHaveBeenLastCalledWith("id-addon");
    view.rerender(
      <PlayerPanelHost>
        <PlayerSubtitlesPopover
          id="subtitle-language-test"
          triggerLabel="Subtitles"
          triggerIcon={<span>CC</span>}
          tracks={tracks}
          selectedId="id-addon"
          emptyText="No subtitles"
          steppers={[]}
          fontWeight="regular"
          onFontWeightChange={vi.fn()}
          onSelect={onSelect}
          onOpenChange={vi.fn()}
        />
      </PlayerPanelHost>,
    );
    fireEvent.click(screen.getByRole("radio", { name: "English" }));
    expect(onSelect).toHaveBeenLastCalledWith("en-addon");
    fireEvent.click(screen.getByRole("radio", { name: "Off" }));
    expect(onSelect).toHaveBeenLastCalledWith(null);
  });
});
