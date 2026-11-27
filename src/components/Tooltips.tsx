import { useEffect, useLayoutEffect, useRef, useState } from "react";

const DELAY_MS = 380;
/** Wider than this (e.g. a list row), the tooltip follows the pointer instead of centering. */
const WIDE_PX = 280;

interface Tip {
  text: string;
  /** Anchor: horizontal position and the edge the tip sits against. */
  x: number;
  y: number;
  above: boolean;
}

/**
 * Replaces the browser's native `title` tooltips with themed ones. Any element
 * with a `title` works unchanged: while the pointer is over it, its title is
 * moved to `data-tip` so the native tooltip (a separate, unthemed window) can
 * never show, and it is put back when the pointer leaves.
 */
export function TooltipLayer() {
  const [tip, setTip] = useState<Tip | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  useEffect(() => {
    let timer: number | undefined;
    let current: HTMLElement | null = null;
    let observer: MutationObserver | null = null;
    let mouseX = 0;

    /** Move the element's title aside (also when React sets it again). */
    const strip = (el: HTMLElement) => {
      const t = el.getAttribute("title");
      if (t === null) return;
      el.dataset.tip = t;
      el.removeAttribute("title");
      setTip((cur) => (cur ? { ...cur, text: t } : cur));
    };
    const release = () => {
      window.clearTimeout(timer);
      observer?.disconnect();
      observer = null;
      if (current && current.dataset.tip !== undefined) {
        if (!current.hasAttribute("title")) current.setAttribute("title", current.dataset.tip);
        delete current.dataset.tip;
      }
      current = null;
      setTip(null);
    };
    /** Hide the tooltip but keep the title moved aside while the pointer stays put. */
    const dismiss = () => {
      window.clearTimeout(timer);
      setTip(null);
    };

    const over = (e: MouseEvent) => {
      mouseX = e.clientX;
      const el = (e.target as Element | null)?.closest<HTMLElement>("[title], [data-tip]");
      if (el === current) return;
      release();
      if (!el) return;
      current = el;
      strip(el);
      const text = el.dataset.tip;
      if (!text) return;
      observer = new MutationObserver(() => strip(el));
      observer.observe(el, { attributes: true, attributeFilter: ["title"] });
      timer = window.setTimeout(() => {
        if (!el.isConnected || current !== el) return;
        const r = el.getBoundingClientRect();
        const above = r.bottom + 48 > window.innerHeight;
        const x = r.width > WIDE_PX ? mouseX : r.left + r.width / 2;
        setTip({ text: el.dataset.tip ?? text, x, y: above ? r.top - 6 : r.bottom + 6, above });
      }, DELAY_MS);
    };
    const move = (e: MouseEvent) => {
      mouseX = e.clientX;
    };
    const leaveWindow = (e: MouseEvent) => {
      if (!e.relatedTarget) release();
    };

    document.addEventListener("mouseover", over);
    document.addEventListener("mousemove", move, { passive: true });
    document.addEventListener("mouseout", leaveWindow);
    document.addEventListener("mousedown", dismiss, true);
    document.addEventListener("keydown", dismiss, true);
    document.addEventListener("scroll", dismiss, true);
    window.addEventListener("blur", dismiss);
    return () => {
      document.removeEventListener("mouseover", over);
      document.removeEventListener("mousemove", move);
      document.removeEventListener("mouseout", leaveWindow);
      document.removeEventListener("mousedown", dismiss, true);
      document.removeEventListener("keydown", dismiss, true);
      document.removeEventListener("scroll", dismiss, true);
      window.removeEventListener("blur", dismiss);
      release();
    };
  }, []);

  // Keep the tooltip inside the window.
  useLayoutEffect(() => {
    if (!tip || !ref.current) {
      setPos(null);
      return;
    }
    const { width, height } = ref.current.getBoundingClientRect();
    const left = Math.max(8, Math.min(tip.x - width / 2, window.innerWidth - width - 8));
    const top = tip.above ? tip.y - height : tip.y;
    setPos({ left, top });
  }, [tip]);

  if (!tip) return null;
  return (
    <div
      ref={ref}
      className="tooltip"
      role="tooltip"
      style={pos ? { left: pos.left, top: pos.top } : { left: -9999, top: -9999 }}
    >
      {tip.text}
    </div>
  );
}

/** Turn off the browser's own right-click menu, except where copy/paste matters. */
export function useNoNativeContextMenu() {
  useEffect(() => {
    const onMenu = (e: MouseEvent) => {
      const t = e.target as Element | null;
      if (t?.closest("input, textarea, [contenteditable='true'], .selectable")) return;
      e.preventDefault();
    };
    document.addEventListener("contextmenu", onMenu);
    return () => document.removeEventListener("contextmenu", onMenu);
  }, []);
}
