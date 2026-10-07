"use client";

import { Dialog } from "@base-ui/react/dialog";
import { Button } from "@base-ui/react/button";
import { Input } from "@base-ui/react/input";

import { FormEvent, useEffect, useRef, useState } from "react";
import { m } from "motion/react";
import type { RuntimeSnapshot } from "@/runtime/types";

type Props = {
  open: boolean;
  account: RuntimeSnapshot["account"];
  onClose(): void;
  onSubmit(email: string, password: string): Promise<void>;
};

const editorialEase = [0.22, 1, 0.36, 1] as const;

export function LoginDialog({ open, account, onClose, onSubmit }: Props) {
  const emailRef = useRef<HTMLInputElement>(null);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");

  useEffect(() => {
    if (account.status === "signedIn") onClose();
  }, [account.status, onClose]);

  const close = () => onClose();

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    await onSubmit(email, password);
  };

  return (
    <Dialog.Root open={open} onOpenChange={(next, details) => {
      if (!next && details.reason === "outside-press") details.cancel();
      else if (!next) onClose();
    }}>
      <Dialog.Portal>
      <Dialog.Popup
        className="fixed inset-0 z-50 m-0 h-dvh max-h-none w-screen max-w-none overflow-hidden border-0 bg-transparent p-0 text-ink"
        initialFocus={emailRef}
        finalFocus={() => document.querySelector<HTMLButtonElement>('[aria-label="Account menu"]')}
        aria-labelledby="login-title"
      >
      <m.div
        className="absolute inset-0 bg-scrim"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.16 }}
        aria-hidden="true"
      />
      <div className="relative grid h-full place-items-center p-5">
        <m.form
          className="grid w-full max-w-[32.5rem] gap-6 bg-canvas p-[34px] shadow-[0_24px_80px_var(--theme-shadow)] max-[560px]:p-[26px_22px]"
          onSubmit={(event) => void submit(event)}
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.22, ease: editorialEase }}
        >
          <div className="flex items-start justify-between gap-6 border-b border-rule pb-[26px]">
            <Dialog.Title render={<h2 />} className="type-title text-balance" id="login-title">Sign in to sync addons</Dialog.Title>
            <Button
              className="focus-ring min-h-10 shrink-0 cursor-pointer border-0 border-b border-current bg-transparent px-1 text-xs text-ink-muted"
              type="button"
              onClick={close}
              aria-label="Close sign in"
            >
              Close
            </Button>
          </div>
          <label className="grid gap-2 text-sm text-ink-muted">
            Email
            <Input
              ref={emailRef}
              className="w-full rounded-none border-0 border-b border-ink bg-transparent py-[11px] text-base text-ink outline-none focus:border-b-2"
              type="email"
              name="email"
              autoComplete="username"
              required
              value={email}
              onChange={(event) => setEmail(event.currentTarget.value)}
            />
          </label>
          <label className="grid gap-2 text-sm text-ink-muted">
            Password
            <Input
              className="w-full rounded-none border-0 border-b border-ink bg-transparent py-[11px] text-base text-ink outline-none focus:border-b-2"
              type="password"
              name="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(event) => setPassword(event.currentTarget.value)}
            />
          </label>
          {account.status === "error" && account.error ? (
            <p className="type-caption m-0 text-danger" role="alert">{account.error}</p>
          ) : null}
          <Button
            className="focus-ring min-h-11 w-full cursor-pointer border border-ink bg-ink px-[18px] py-[13px] font-medium text-canvas disabled:cursor-wait disabled:opacity-60"
            type="submit"
            disabled={account.status === "authenticating"}
          >
            {account.status === "authenticating" ? "Signing in" : "Sign in"}
          </Button>
        </m.form>
      </div>
      </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
