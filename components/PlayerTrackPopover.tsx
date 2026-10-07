"use client";

import { Button } from "@base-ui/react/button";
import { NumberField } from "@base-ui/react/number-field";
import { Popover } from "@base-ui/react/popover";
import { Radio } from "@base-ui/react/radio";
import { RadioGroup } from "@base-ui/react/radio-group";

import { IconMinus, IconPlus } from "@tabler/icons-react";
import { createContext, forwardRef, useCallback, useContext, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { AnimatePresence, m } from "motion/react";

export type PlayerTrackItem = {
  id: string | null;
  label: string;
  description?: string | null;
};

export type PlayerTrackStepper = {
  label: string;
  valueText: string;
  disabled?: boolean;
  decreaseLabel: string;
  increaseLabel: string;
  onDecrease(): void;
  onIncrease(): void;
  editable?: {
    value: number;
    min: number;
    max: number;
    suffix: string;
    step?: number;
    onCommit(value: number): void;
  };
};

export function PlayerStepperControl({
  id,
  stepper,
}: {
  id: string;
  stepper: PlayerTrackStepper;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const beginEditing = () => {
    if (!stepper.editable) return;
    setDraft(String(stepper.editable.value));
    setEditing(true);
  };
  const cancelEditing = () => {
    setEditing(false);
    setDraft("");
  };
  const commitEditing = () => {
    if (!stepper.editable) return;
    const parsed = Number(draft);
    if (draft.trim() && Number.isFinite(parsed)) {
      stepper.editable.onCommit(Math.min(stepper.editable.max, Math.max(stepper.editable.min, parsed)));
    }
    cancelEditing();
  };

  return (
    <div className="grid gap-2">
      <span className="type-caption text-player-ink/55" id={id}>
        {stepper.label}
      </span>
      <div
        className="flex items-center rounded-full bg-player-ink/10"
        role="group"
        aria-labelledby={id}
      >
        <Button
          className="focus-ring grid min-h-11 min-w-11 place-items-center rounded-full border-0 bg-transparent text-base transition-colors duration-fast hover:bg-player-ink/10"
          type="button"
          aria-label={stepper.decreaseLabel}
          disabled={stepper.disabled}
          onClick={stepper.onDecrease}
        >
          <IconMinus aria-hidden="true" size={18} stroke={2} />
        </Button>
        {editing && stepper.editable ? (
          <span className="flex min-w-0 flex-1 items-center justify-center text-sm tabular-nums">
            <NumberField.Root value={draft.trim() ? Number(draft) : null} min={stepper.editable.min} max={stepper.editable.max} step={stepper.editable.step ?? 1} onValueChange={(value) => setDraft(value === null ? "" : String(value))}>
            <NumberField.Input
              className="focus-ring min-h-8 w-16 rounded-md border border-player-ink/25 bg-player-canvas px-2 text-center text-sm tabular-nums text-player-ink"
              aria-label={`Edit ${stepper.label.toLowerCase()}`}
              autoFocus
              onFocus={(event) => event.currentTarget.select()}
              onBlur={commitEditing}
              onKeyDown={(event) => {
                event.stopPropagation();
                if (event.key === "Enter") {
                  event.preventDefault();
                  commitEditing();
                } else if (event.key === "Escape") {
                  event.preventDefault();
                  cancelEditing();
                }
              }}
            />
            </NumberField.Root>
            <span aria-hidden="true">{stepper.editable.suffix}</span>
          </span>
        ) : stepper.editable ? (
          <Button
            className="focus-ring min-h-11 min-w-0 flex-1 border-0 bg-transparent text-center text-sm tabular-nums"
            type="button"
            aria-label={`Edit ${stepper.label.toLowerCase()}`}
            disabled={stepper.disabled}
            onClick={beginEditing}
          >
            {stepper.valueText}
          </Button>
        ) : (
          <span className="min-w-0 flex-1 text-center text-sm tabular-nums">{stepper.valueText}</span>
        )}
        <Button
          className="focus-ring grid min-h-11 min-w-11 place-items-center rounded-full border-0 bg-transparent text-base transition-colors duration-fast hover:bg-player-ink/10"
          type="button"
          aria-label={stepper.increaseLabel}
          disabled={stepper.disabled}
          onClick={stepper.onIncrease}
        >
          <IconPlus aria-hidden="true" size={18} stroke={2} />
        </Button>
      </div>
    </div>
  );
}

export type PlayerPopoverHandle = {
  hide(): void;
  show(): void;
};

type PanelPlacement = { bottom: number; originRight: number };
/** The displayed page's natural size; `animate` is false for the first measurement after opening. */
type PanelSize = { width: number; height: number; animate: boolean };

type PanelContextValue = {
  activeId: string | null;
  /** The panel whose content fills the surface; it outlives `activeId` until the close animation ends. */
  displayedId: string | null;
  /** Which way content travels when switching panels: 1 toward a trigger further right, -1 left, 0 on open. */
  direction: number;
  placement: PanelPlacement;
  size: PanelSize | null;
  reportSize(width: number, height: number): void;
  slot: HTMLElement | null;
  setSlot(node: HTMLElement | null): void;
  registerPage(id: string, page: ReactNode): () => void;
  getPage(id: string | null): ReactNode;
  subscribePages(listener: () => void): () => void;
  open(id: string): void;
  toggle(id: string): void;
  close(restoreFocus?: boolean): void;
  settle(): void;
  /** True once after a keyboard press opened the panel, so its content takes focus when it mounts. */
  takeFocusOnOpen(): boolean;
};

const PlayerPanelContext = createContext<PanelContextValue | null>(null);

const panelEase = [0.23, 1, 0.32, 1] as const;
// Switching panels copies YouTube's settings menu: the surface transitions its real width and height
// (no transform scaling, so nothing inside is ever squashed) while each page, absolutely positioned at
// its natural size, slides a full width out one side and in from the other with a cross-fade. Everything
// shares YouTube's 250ms cubic-bezier(0.4, 0, 0.2, 1).
const switchMotion = { duration: 0.25, ease: [0.4, 0, 0.2, 1] } as const;
const pageVariants = {
  enter: (direction: number) => ({ x: `${direction * 100}%`, opacity: 0 }),
  center: { x: "0%", opacity: 1, transition: switchMotion },
  exit: (direction: number) => ({ x: `${direction * -100}%`, opacity: 0, transition: switchMotion }),
};
const defaultPlacement: PanelPlacement = { bottom: 96, originRight: 20 };

/**
 * One shared settings panel for the player. Opening rises it out of the trigger, switching to another
 * trigger morphs the same surface to the next panel, and closing sinks it back. `measure` reports where
 * the panel sits above its triggers when it opens.
 */
export function usePlayerPanels(measure?: (id: string) => PanelPlacement | null) {
  const [activeId, setActiveId] = useState<string | null>(null);
  const [displayedId, setDisplayedId] = useState<string | null>(null);
  const [direction, setDirection] = useState(0);
  const [placement, setPlacement] = useState(defaultPlacement);
  const [slot, setSlot] = useState<HTMLElement | null>(null);
  const [size, setSize] = useState<PanelSize | null>(null);
  const focusOnOpen = useRef(false);
  const pages = useRef(new Map<string, ReactNode>());
  const pageListeners = useRef(new Set<() => void>());
  const registerPage = useCallback((id: string, page: ReactNode) => {
    pages.current.set(id, page);
    for (const listener of pageListeners.current) listener();
    return () => {
      if (pages.current.get(id) !== page) return;
      pages.current.delete(id);
      for (const listener of pageListeners.current) listener();
    };
  }, []);
  const getPage = useCallback((id: string | null) => id ? pages.current.get(id) ?? null : null, []);
  const subscribePages = useCallback((listener: () => void) => {
    pageListeners.current.add(listener);
    return () => { pageListeners.current.delete(listener); };
  }, []);
  const reportSize = useCallback((width: number, height: number) => {
    setSize((previous) => previous && previous.width === width && previous.height === height
      ? previous
      : { width, height, animate: previous !== null });
  }, []);

  const open = (id: string) => {
    if (!activeId && document.activeElement instanceof HTMLElement && document.activeElement.matches(":focus-visible")) {
      focusOnOpen.current = true;
    }
    const triggers = [...document.querySelectorAll("[data-player-panel-trigger]")].map((trigger) => trigger.getAttribute("aria-controls"));
    setDirection(activeId ? Math.sign(triggers.indexOf(id) - triggers.indexOf(activeId)) || 1 : 0);
    setPlacement(measure?.(id) ?? defaultPlacement);
    setActiveId(id);
    setDisplayedId(id);
  };
  const close = (restoreFocus = false) => {
    if (!activeId) return;
    if (restoreFocus && slot?.contains(document.activeElement)) {
      document.querySelector<HTMLElement>(`[aria-controls="${activeId}"]`)?.focus();
    }
    setActiveId(null);
  };

  const value: PanelContextValue = {
    activeId,
    displayedId,
    direction,
    placement,
    size,
    reportSize,
    slot,
    setSlot,
    registerPage,
    getPage,
    subscribePages,
    open,
    toggle: (id) => (activeId === id ? close() : open(id)),
    close,
    settle: () => {
      setDisplayedId(null);
      setSize(null);
      focusOnOpen.current = false;
    },
    takeFocusOnOpen: () => {
      const take = focusOnOpen.current;
      focusOnOpen.current = false;
      return take;
    },
  };
  return { activeId, close, open, value };
}

export function PlayerPanelProvider({ panels, children }: { panels: ReturnType<typeof usePlayerPanels>; children: ReactNode }) {
  return <PlayerPanelContext.Provider value={panels.value}>
    <Popover.Root open={panels.activeId !== null} triggerId={panels.activeId ? `${panels.activeId}-trigger` : null} onOpenChange={(open, details) => {
      if (open) {
        const id = details.trigger?.getAttribute("data-player-panel-id");
        if (id) panels.open(id);
      } else {
        if (details.reason === "focus-out") { details.cancel(); return; }
        details.preventUnmountOnClose();
        panels.close(details.reason === "escape-key");
      }
    }}>
      {children}
    </Popover.Root>
  </PlayerPanelContext.Provider>;
}

/** The single morphing surface every player panel renders into. Anchored right/bottom so it grows up and left. */
export function PlayerPanelSurface({ className = "" }: { className?: string }) {
  const panels = useContext(PlayerPanelContext);
  const [container, setContainer] = useState<HTMLElement | null>(null);
  const page = useSyncExternalStore(panels?.subscribePages ?? (() => () => {}), () => panels?.getPage(panels.displayedId) ?? null, () => null);
  if (!panels) return null;
  const { activeId, placement, size, setSlot, settle } = panels;
  const resize = size?.animate ? switchMotion : { duration: 0 };
  return (
    <>
    <span hidden ref={(node) => { if (node) setContainer(node.parentElement); }} />
    <Popover.Portal container={container} keepMounted>
    <Popover.Positioner className="contents" style={{ position: "static", transform: "none", top: "auto", left: "auto" }}>
    <AnimatePresence onExitComplete={settle}>
      {activeId ? (
        <Popover.Popup
          render={<m.div
          key="player-panel"
          ref={setSlot}
          // The hairline is an overlay above the pages so sticky headers and scrolled content never cover it.
          className={`absolute z-40 overflow-hidden bg-player-canvas text-player-ink after:pointer-events-none after:absolute after:inset-0 after:z-30 after:rounded-[inherit] after:inset-ring after:inset-ring-player-ink/10 ${className}`}
          style={{
            bottom: placement.bottom,
            borderRadius: 16,
            transformOrigin: `calc(100% - ${placement.originRight}px) 100%`,
          }}
          initial={{ opacity: 0, scale: 0.96, y: 6 }}
          animate={{
            opacity: 1,
            scale: 1,
            y: 0,
            ...(size ? { width: size.width, height: size.height } : {}),
            transition: { type: "spring", duration: 0.22, bounce: 0, width: resize, height: resize },
          }}
          exit={{ opacity: 0, scale: 0.96, y: 6, transition: { duration: 0.15, ease: panelEase } }}
        />}
          role="presentation"
          initialFocus={false}
          finalFocus={false}
        >{page}</Popover.Popup>
      ) : null}
    </AnimatePresence>
    </Popover.Positioner>
    </Popover.Portal>
    </>
  );
}

export function PlayerPopoverTrigger({
  id,
  label,
  icon,
  activeIcon,
  onClick,
}: {
  id: string;
  label: string;
  icon: ReactNode;
  activeIcon?: ReactNode;
  onClick?(): void;
}) {
  const panels = useContext(PlayerPanelContext);
  const open = panels?.activeId === id;
  return (
    <Popover.Trigger
      className={`focus-ring grid size-9 place-items-center rounded-full border-0 p-0 transition-[background-color,scale] duration-[120ms] ease-out hover:bg-player-ink/10 active:scale-97 ${open ? "bg-player-ink/15" : "bg-transparent"}`}
      id={`${id}-trigger`}
      data-player-panel-id={id}
      type="button"
      aria-label={label}
      title={label}
      aria-haspopup="dialog"
      aria-expanded={open}
      aria-controls={id}
      data-player-panel-trigger=""
      onClick={() => {
        onClick?.();
      }}
    >
      <span aria-hidden="true">{open && activeIcon ? activeIcon : icon}</span>
    </Popover.Trigger>
  );
}

type PanelProps = {
  id: string;
  title: string;
  titleHidden?: boolean;
  /** CSS width expression, e.g. `min(18rem,calc(100vw-1rem))`. */
  width: string;
  busy?: boolean;
  error?: string | null;
  children: ReactNode;
  onOpenChange?(open: boolean): void;
  onWillOpen?(): void;
};

export const PlayerPopoverPanel = forwardRef<PlayerPopoverHandle, PanelProps>(function PlayerPopoverPanel(
  { id, title, titleHidden, width, busy, error, children, onOpenChange, onWillOpen },
  ref,
) {
  const panels = useContext(PlayerPanelContext);
  const titleId = `${id}-title`;
  const active = panels?.activeId === id;
  const shown = panels?.displayedId === id;
  const pageRef = useRef<ResizeObserver | null>(null);
  const reportSize = panels?.reportSize;

  useImperativeHandle(ref, () => ({
    hide: () => {
      if (panels?.activeId === id) panels.close();
    },
    show: () => {
      if (panels && panels.activeId !== id) panels.open(id);
    },
  }));

  useEffect(() => {
    if (active) onWillOpen?.();
    onOpenChange?.(active);
    // Only the open state should notify; the callbacks are recreated by every parent render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  const page = panels ? (
    <AnimatePresence custom={panels.direction}>
      {shown ? (
        <m.div
          key={id}
          id={id}
          ref={(node: HTMLDivElement | null) => {
            pageRef.current?.disconnect();
            pageRef.current = null;
            if (node && reportSize) {
              const report = () => reportSize(node.offsetWidth, node.offsetHeight);
              report();
              pageRef.current = new ResizeObserver(report);
              pageRef.current.observe(node);
            }
            // Keyboard openings move focus into the panel; pointer openings leave it on the trigger.
            if (!node || !panels.takeFocusOnOpen()) return;
            node.querySelector<HTMLElement>("input:checked, button, input, [tabindex]:not([tabindex='-1'])")?.focus({ preventScroll: true });
          }}
          role="dialog"
          aria-labelledby={titleId}
          aria-busy={busy}
          className="@container absolute left-0 top-0 flex max-h-[min(28rem,calc(100dvh-10rem))] max-w-[calc(100vw-40px)] flex-col overflow-y-auto overscroll-contain p-2"
          style={{ width }}
          custom={panels.direction}
          variants={pageVariants}
          initial={panels.direction ? "enter" : false}
          animate="center"
          exit="exit"
        >
          {/* Sticky, edge-to-edge header (the negative margins cancel the page's p-2) so long panels scroll beneath it. */}
          <h3 className={titleHidden ? "sr-only" : "type-label sticky -top-2 z-20 -mx-2 -mt-2 mb-1 flex h-11 shrink-0 items-center bg-player-canvas px-5"} id={titleId}>
            {title}
          </h3>
          {children}
          {error ? (
            <p className="type-caption px-3 pb-2 pt-2 text-danger" role="alert">
              {error}
            </p>
          ) : null}
        </m.div>
      ) : null}
    </AnimatePresence>
  ) : null;
  useLayoutEffect(() => {
    if (!panels) return;
    return panels.registerPage(id, page);
  });
  return null;
});

export function PlayerSectionLabel({ id, children, className = "" }: { id?: string; children: ReactNode; className?: string }) {
  return (
    <h4 className={`type-caption px-3 pb-1 pt-2 text-player-ink/55 ${className}`} id={id}>
      {children}
    </h4>
  );
}

export function PlayerOptionRow({
  value,
  label,
  description,
  tags,
  selected,
  disabled,
  multiline,
}: {
  name?: string;
  value: string;
  label: string;
  description?: string | null;
  tags?: string[];
  selected: boolean;
  disabled?: boolean;
  multiline?: boolean;
  onSelect?(): void;
}) {
  const clamp = multiline ? "line-clamp-2 break-words" : "truncate";
  return (
    <Radio.Root value={value} disabled={disabled}
      className={`relative flex min-h-11 items-center gap-3 rounded-lg px-3 py-2 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-player-ink ${selected ? "bg-player-ink/10" : "hover:bg-player-ink/5"} ${disabled ? "cursor-not-allowed opacity-50" : "cursor-pointer"}`}
    >
      <span className="min-w-0 flex-1">
        <span className={`block text-sm ${clamp}`}>{label}</span>
        {description ? (
          <span className={`type-caption mt-0.5 block text-player-ink/55 ${clamp}`}>{description}</span>
        ) : null}
      </span>
      {tags?.length ? (
        <span className="flex shrink-0 gap-1">
          {tags.map((tag) => (
            <span className="type-caption rounded bg-player-ink/10 px-1.5 tabular-nums text-player-ink/80" key={tag}>
              {tag}
            </span>
          ))}
        </span>
      ) : null}
    </Radio.Root>
  );
}

type Props = {
  id: string;
  title: string;
  triggerLabel: string;
  triggerIcon: ReactNode;
  triggerActiveIcon?: ReactNode;
  name: string;
  items: PlayerTrackItem[];
  selectedId: string | null;
  emptyText: string;
  error?: string | null;
  busy?: boolean;
  width?: string;
  children?: ReactNode;
  onSelect(id: string | null): void;
  onOpenChange?(open: boolean): void;
  onWillOpen?(): void;
};

export const PlayerTrackPopover = forwardRef<PlayerPopoverHandle, Props>(function PlayerTrackPopover(
  {
    id,
    title,
    triggerLabel,
    triggerIcon,
    triggerActiveIcon,
    name,
    items,
    selectedId,
    emptyText,
    error,
    busy,
    width = "min(18rem,calc(100vw - 1rem))",
    children,
    onSelect,
    onOpenChange,
    onWillOpen,
  },
  ref,
) {
  return (
    <>
      <PlayerPopoverTrigger id={id} label={triggerLabel} icon={triggerIcon} activeIcon={triggerActiveIcon} />
      <PlayerPopoverPanel
        ref={ref}
        id={id}
        title={title}
        width={width}
        busy={busy}
        error={error}
        onWillOpen={onWillOpen}
        onOpenChange={onOpenChange}
      >
        {children ?? (items.length > 0 ? (
          <RadioGroup name={name} value={selectedId ?? "off"} onValueChange={(value) => onSelect(value === "off" ? null : String(value))} render={<fieldset />} className="m-0 border-0 p-0">
            <legend className="sr-only">{title}</legend>
            {items.map((item) => (
              <PlayerOptionRow
                key={item.id ?? "off"}
                value={item.id ?? "off"}
                label={item.label}
                description={item.description}
                selected={item.id === selectedId}
              />
            ))}
          </RadioGroup>
        ) : (
          <p className="type-caption px-3 pb-2 text-player-ink/55">{emptyText}</p>
        ))}
      </PlayerPopoverPanel>
    </>
  );
});
