import type { ReactNode } from "react";
import { createElement } from "react";

/** A stable color for each channel so tags are easy to tell apart. */
export function channelColor(name: string): string {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${h % 360} 70% 62%)`;
}

export function timeAgo(ts: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ts) / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  return new Date(ts).toLocaleDateString();
}

export function clockTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Wrap every occurrence of the matched terms in <mark>. */
export function highlight(text: string, terms: string[]): ReactNode[] {
  const clean = terms.filter(Boolean).sort((a, b) => b.length - a.length);
  if (!clean.length) return [text];
  const re = new RegExp(`(${clean.map(escapeRe).join("|")})`, "gi");
  return text.split(re).map((part, i) => (i % 2 === 1 ? createElement("mark", { key: i }, part) : part));
}

let audioCtx: AudioContext | null = null;

/** Short synthesized alert tones: a single blip for flags, a rising chirp for raids. */
export function playTone(kind: "flag" | "raid") {
  try {
    audioCtx ??= new AudioContext();
    const ctx = audioCtx;
    const notes = kind === "raid" ? [523, 659, 784] : [880];
    notes.forEach((freq, i) => {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      const t = ctx.currentTime + i * 0.12;
      osc.frequency.value = freq;
      osc.type = "sine";
      gain.gain.setValueAtTime(0.0001, t);
      gain.gain.exponentialRampToValueAtTime(0.25, t + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, t + 0.18);
      osc.connect(gain).connect(ctx.destination);
      osc.start(t);
      osc.stop(t + 0.2);
    });
  } catch {
    /* audio unavailable */
  }
}
