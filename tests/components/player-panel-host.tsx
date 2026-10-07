import type { ReactNode } from "react";
import { PlayerPanelProvider, PlayerPanelSurface, usePlayerPanels } from "../../components/PlayerTrackPopover";

/** Hosts player panels outside PlayerDialog: the shared context plus the surface they render into. */
export function PlayerPanelHost({ children }: { children: ReactNode }) {
  const panels = usePlayerPanels();
  return (
    <PlayerPanelProvider panels={panels}>
      {children}
      <PlayerPanelSurface />
    </PlayerPanelProvider>
  );
}
