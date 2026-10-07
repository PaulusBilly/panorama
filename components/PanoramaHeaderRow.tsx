"use client";

import type { ReactNode, Ref } from "react";
import Link from "next/link";
import { AccountMenu } from "./AccountMenu";

type Props = {
  leadingControl: ReactNode;
  replacementContent?: ReactNode;
  replacementVisible?: boolean;
  signedInEmail: string | null;
  onSignIn(): void;
  onLogout(): void;
  rowRef?: Ref<HTMLDivElement>;
  chromeHidden?: boolean;
  logoInverted?: boolean;
};

export const panoramaHeaderIconButtonClass = "focus-ring grid cursor-pointer place-items-center border-0 bg-transparent p-0 text-current";

export function PanoramaHeaderRow({
  leadingControl,
  replacementContent,
  replacementVisible = false,
  signedInEmail,
  onSignIn,
  onLogout,
  rowRef,
  chromeHidden = false,
  logoInverted = false,
}: Props) {
  const replacementMotion = "transition-[opacity,transform] duration-[320ms] ease-[cubic-bezier(0.22,1,0.36,1)]";

  return (
    <div ref={rowRef} className="content-container relative grid min-h-10 grid-cols-[1fr_auto_1fr] items-center gap-4 pt-5">
      <div
        className={`flex min-h-7 items-center justify-start ${replacementMotion} ${replacementVisible ? "pointer-events-none opacity-0" : "opacity-100"}`}
        aria-hidden={replacementVisible}
        inert={replacementVisible || undefined}
      >
        {leadingControl}
      </div>
      <Link
        className={`focus-ring block h-7 w-[138px] cursor-pointer justify-self-center transition-[opacity,filter] duration-[320ms] ease-[cubic-bezier(0.22,1,0.36,1)] ${chromeHidden || replacementVisible ? "pointer-events-none opacity-0" : "opacity-100"} ${logoInverted ? "invert" : ""}`}
        href="/"
        aria-label="Panorama home"
        aria-hidden={chromeHidden || replacementVisible}
        tabIndex={chromeHidden || replacementVisible ? -1 : undefined}
      >
        <img
          className="block size-full"
          src="/logo.svg"
          alt=""
          width="138"
          height="28"
        />
      </Link>
      <div
        className={`flex min-h-7 items-center justify-end text-xs transition-opacity duration-[320ms] ease-[cubic-bezier(0.22,1,0.36,1)] ${chromeHidden || replacementVisible ? "pointer-events-none opacity-0" : "opacity-100"}`}
        aria-hidden={chromeHidden || replacementVisible}
        inert={chromeHidden || replacementVisible || undefined}
      >
        <AccountMenu signedIn={signedInEmail !== null} onLogin={onSignIn} onLogout={onLogout} />
      </div>
      {replacementContent ? (
        <div
          className={`absolute inset-x-0 top-5 grid min-h-10 grid-cols-[auto_minmax(0,1fr)] items-center gap-4 ${replacementMotion} ${replacementVisible ? "translate-y-0 opacity-100" : "pointer-events-none -translate-y-full opacity-0"}`}
          aria-hidden={!replacementVisible}
          inert={!replacementVisible || undefined}
        >
          {replacementContent}
        </div>
      ) : null}
    </div>
  );
}
