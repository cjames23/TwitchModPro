// Helpers for cross-channel notes, account age and the escalation ladder.
import type { LadderStep, ModAction } from "./types";

const SANCTIONS = new Set(["warn", "timeout", "ban"]);

export function fmtSecs(s: number) {
  if (s % 86400 === 0) return `${s / 86400}d`;
  if (s % 3600 === 0) return `${s / 3600}h`;
  if (s % 60 === 0) return `${s / 60}m`;
  return `${s}s`;
}

export function stepLabel(step: LadderStep) {
  if (step.action === "warn") return "Warn";
  if (step.action === "ban") return "Ban";
  return `Timeout ${fmtSecs(step.durationSecs ?? 600)}`;
}

export function actionLabel(a: ModAction) {
  switch (a.action) {
    case "warn":
      return "Warned";
    case "timeout":
      return `Timed out ${a.durationSecs ? fmtSecs(a.durationSecs) : ""}`.trim();
    case "ban":
      return "Banned";
    case "unban":
      return "Unbanned";
    case "delete":
      return "Deleted a message";
    case "automod_allow":
      return "Approved AutoMod message";
    case "automod_deny":
      return "Denied AutoMod message";
    case "unban_request_approve":
      return "Approved unban request";
    case "unban_request_deny":
      return "Denied unban request";
    default:
      return a.action;
  }
}

export function ordinal(n: number) {
  const s = ["th", "st", "nd", "rd"];
  const v = n % 100;
  return n + (s[(v - 20) % 10] || s[v] || s[0]);
}

/** Sanctions grouped by login, newest first. */
export function indexSanctions(list: ModAction[]): Map<string, ModAction[]> {
  const m = new Map<string, ModAction[]>();
  for (const a of list) {
    if (!a.ok || !(SANCTIONS.has(a.action) || a.action === "unban")) continue;
    const arr = m.get(a.targetLogin) ?? [];
    arr.push(a);
    m.set(a.targetLogin, arr);
  }
  for (const arr of m.values()) arr.sort((a, b) => b.timestamp - a.timestamp);
  return m;
}

/** Prior warns/timeouts/bans of this user in this channel within the lookback window. */
export function offenseCount(byLogin: Map<string, ModAction[]>, channel: string, login: string, lookbackDays: number, now: number) {
  const since = now - lookbackDays * 86_400_000;
  return (byLogin.get(login) ?? []).filter((a) => a.channel === channel && SANCTIONS.has(a.action) && a.timestamp >= since)
    .length;
}

export function suggestedStep(ladder: LadderStep[], priorOffenses: number): LadderStep {
  if (!ladder.length) return { action: "timeout", durationSecs: 600 };
  return ladder[Math.min(priorOffenses, ladder.length - 1)];
}

/** The latest sanction in each *other* channel. */
export function otherChannelNotes(byLogin: Map<string, ModAction[]>, channel: string, login: string): ModAction[] {
  const seen = new Set<string>();
  const out: ModAction[] = [];
  for (const a of byLogin.get(login) ?? []) {
    if (a.channel === channel || seen.has(a.channel) || !SANCTIONS.has(a.action)) continue;
    seen.add(a.channel);
    out.push(a);
  }
  return out;
}

export function accountAge(createdAt: number, now: number) {
  const days = Math.floor((now - createdAt) / 86_400_000);
  if (days < 1) return "created today";
  if (days < 60) return `${days} day${days === 1 ? "" : "s"} old`;
  if (days < 730) return `${Math.floor(days / 30)} months old`;
  return `${Math.floor(days / 365)} years old`;
}

export const isNewAccount = (createdAt: number, now: number) => now - createdAt < 30 * 86_400_000;
