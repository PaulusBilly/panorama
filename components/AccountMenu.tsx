"use client";

import { Popover } from "@base-ui/react/popover";
import { Button } from "@base-ui/react/button";

import { useCallback, useEffect, useState } from "react";
import { IconMenu2 } from "@tabler/icons-react";

type Props = {
  signedIn: boolean;
  onLogin(): void;
  onLogout(): void;
};

export function AccountMenu({ signedIn, onLogin, onLogout }: Props) {
  const [open, setOpen] = useState(false);
  const [immediate, setImmediate] = useState(false);
  const hidePanel = useCallback(() => setOpen(false), []);
  useEffect(() => {
    if (!open) return;
    const dismiss = () => { setImmediate(true); setOpen(false); };
    window.addEventListener("wheel", dismiss, { capture: true, passive: true });
    window.addEventListener("touchmove", dismiss, { capture: true, passive: true });
    window.addEventListener("scroll", dismiss, { capture: true, passive: true });
    window.addEventListener("resize", dismiss, { passive: true });
    return () => {
      window.removeEventListener("wheel", dismiss, true);
      window.removeEventListener("touchmove", dismiss, true);
      window.removeEventListener("scroll", dismiss, true);
      window.removeEventListener("resize", dismiss);
    };
  }, [open]);

  return (
    <Popover.Root open={open} onOpenChange={(next) => { if (next) setImmediate(false); setOpen(next); }}>
    <div className="relative">
      <Popover.Trigger
        className="focus-ring grid min-h-10 min-w-10 cursor-pointer place-items-center border-0 bg-transparent p-0 text-current transition-transform duration-150 ease-out active:scale-[0.96]"
        type="button"
        aria-label="Account menu"
        title="Account menu"
        aria-expanded={open}
        aria-controls="account-menu"
      >
        <IconMenu2 aria-hidden="true" size={22} stroke={1.6} />
      </Popover.Trigger>
      <Popover.Portal>
      <Popover.Positioner positionMethod="fixed" side="bottom" align="end" sideOffset={8} collisionPadding={16}>
      <Popover.Popup
        id="account-menu"
        aria-label="Account actions"
        initialFocus={false}
        hidden={immediate && !open}
        data-dismiss-immediate={immediate || undefined}
        className="account-menu-popover relative inset-auto m-0 min-w-[12rem] bg-canvas p-3 text-ink shadow-[0_0_0_1px_rgba(0,0,0,0.06),0_2px_6px_rgba(0,0,0,0.08),0_10px_28px_rgba(0,0,0,0.10)]"
      >
        <Button
          className="focus-ring min-h-10 w-full cursor-pointer border-0 border-b border-current bg-transparent px-1 text-left text-sm text-ink"
          type="button"
          onClick={() => {
            hidePanel();
            if (signedIn) onLogout();
            else onLogin();
          }}
        >
          {signedIn ? "Log Out" : "Log In"}
        </Button>
      </Popover.Popup>
      </Popover.Positioner>
      </Popover.Portal>
    </div>
    </Popover.Root>
  );
}
