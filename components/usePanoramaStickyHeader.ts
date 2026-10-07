"use client";

import { useEffect, useRef, useState, type RefObject } from "react";

export function usePanoramaStickyHeader(
  headerRef: RefObject<HTMLElement | null>,
  {
    heroRef,
    overHeroInitially = false,
    revealOn = false,
  }: {
    heroRef?: RefObject<HTMLElement | null>;
    overHeroInitially?: boolean;
    revealOn?: boolean;
  } = {},
) {
  const [hidden, setHidden] = useState(false);
  const [overHero, setOverHero] = useState(overHeroInitially);
  const [height, setHeight] = useState(0);
  const previousScrollTop = useRef(0);
  const previousRevealOn = useRef(revealOn);
  const holdVisibleUntil = useRef(0);

  useEffect(() => {
    const viewport = document.body.classList.contains("panorama-desktop")
      ? document.querySelector<HTMLElement>(".app-viewport")
      : null;
    const scrollTarget = viewport ?? window;
    const getScrollTop = () => viewport?.scrollTop ?? window.scrollY;
    const syncHeight = () => setHeight(headerRef.current?.offsetHeight ?? 0);

    const newlyRevealed = revealOn && !previousRevealOn.current;
    previousRevealOn.current = revealOn;
    previousScrollTop.current = getScrollTop();
    if (newlyRevealed) {
      holdVisibleUntil.current = performance.now() + 400;
      setHidden(false);
    }
    syncHeight();
    const handleScroll = () => {
      const scrollTop = getScrollTop();
      const headerHeight = headerRef.current?.offsetHeight ?? 0;
      setHeight(headerHeight);
      const viewportTop = viewport?.getBoundingClientRect().top ?? 0;
      setOverHero(Boolean(heroRef?.current && heroRef.current.getBoundingClientRect().bottom > viewportTop + headerHeight));
      if (scrollTop <= 0 || scrollTop < previousScrollTop.current) {
        setHidden(false);
      } else if (scrollTop > headerHeight && performance.now() >= holdVisibleUntil.current) {
        setHidden(true);
      }
      previousScrollTop.current = scrollTop;
    };

    const header = headerRef.current;
    const observer = typeof ResizeObserver === "undefined" || !header
      ? null
      : new ResizeObserver(syncHeight);
    if (observer && header) observer.observe(header);
    scrollTarget.addEventListener("scroll", handleScroll, { passive: true });
    window.addEventListener("resize", syncHeight, { passive: true });
    return () => {
      observer?.disconnect();
      scrollTarget.removeEventListener("scroll", handleScroll);
      window.removeEventListener("resize", syncHeight);
    };
  }, [headerRef, heroRef, revealOn]);

  return { height, hidden, overHero };
}
