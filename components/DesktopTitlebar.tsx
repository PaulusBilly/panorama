"use client";

import { Button } from "@base-ui/react/button";

import { IconMinus, IconSquare, IconX } from "@tabler/icons-react";
import type { DesktopWindowControl } from "@/desktop/shared/desktop-api";

function controlWindow(action: DesktopWindowControl) {
  void window.panoramaDesktop?.controlWindow?.(action).catch(() => undefined);
}

export function DesktopTitlebar() {
  return (
    <div className="desktop-titlebar" aria-label="Panorama window title bar">
      <span>Panorama</span>
      <div className="desktop-window-controls">
        <Button type="button" aria-label="Minimize window" title="Minimize" onClick={() => controlWindow("minimize")}>
          <IconMinus aria-hidden="true" size={15} stroke={1.5} />
        </Button>
        <Button type="button" aria-label="Maximize window" title="Maximize" onClick={() => controlWindow("toggle-maximize")}>
          <IconSquare aria-hidden="true" size={12} stroke={1.5} />
        </Button>
        <Button className="desktop-window-close" type="button" aria-label="Close window" title="Close" onClick={() => controlWindow("close")}>
          <IconX aria-hidden="true" size={15} stroke={1.5} />
        </Button>
      </div>
    </div>
  );
}
