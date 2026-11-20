import { useEffect, useState } from "react";
import type { ActionTarget, LadderStep } from "../types";
import { ordinal, stepLabel } from "../intel";

export const TIMEOUTS: { label: string; secs: number }[] = [
  { label: "1m", secs: 60 },
  { label: "10m", secs: 600 },
  { label: "1h", secs: 3600 },
  { label: "24h", secs: 86400 },
  { label: "7d", secs: 604800 },
];

export interface ActionHandlers {
  deleteMessage: (t: ActionTarget) => Promise<void>;
  warn: (t: ActionTarget, reason: string) => Promise<void>;
  timeout: (t: ActionTarget, secs: number, reason: string) => Promise<void>;
  ban: (t: ActionTarget, reason: string) => Promise<void>;
  banEverywhere: (t: ActionTarget, reason: string) => Promise<void>;
  unban: (t: ActionTarget) => Promise<void>;
}

export type ActionMode = "idle" | "warn" | "timeout" | "ban";

interface Props {
  target: ActionTarget;
  actions: ActionHandlers;
  /** Show Unban (used in the user panel). */
  showUnban?: boolean;
  defaultReason?: string;
  /** The ladder's next step for this user, and how many prior offenses led to it. */
  suggestion?: { step: LadderStep; prior: number };
  /** How many monitored channels you moderate (enables "ban everywhere" when > 1). */
  modChannelCount?: number;
  /** Controlled mode, so keyboard shortcuts can open the warn/timeout/ban prompts. */
  mode?: ActionMode;
  onModeChange?: (m: ActionMode) => void;
}

/** Warn / delete / timeout / ban controls. Warn and ban ask for confirmation. */
export function ModActions({
  target,
  actions,
  showUnban,
  defaultReason = "",
  suggestion,
  modChannelCount = 1,
  mode: controlled,
  onModeChange,
}: Props) {
  const [own, setOwn] = useState<ActionMode>("idle");
  const mode = controlled ?? own;
  const setMode = (m: ActionMode) => (onModeChange ? onModeChange(m) : setOwn(m));
  const [reason, setReason] = useState(defaultReason);
  const [everywhere, setEverywhere] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (mode === "idle") setEverywhere(false);
  }, [mode]);

  async function go(fn: () => Promise<void>) {
    setBusy(true);
    try {
      await fn();
      setMode("idle");
    } finally {
      setBusy(false);
    }
  }

  function runSuggestion() {
    if (!suggestion) return;
    const { step } = suggestion;
    if (step.action === "ban") setMode("ban");
    else if (step.action === "warn") setMode("warn");
    else go(() => actions.timeout(target, step.durationSecs ?? 600, reason));
  }

  const onEnter = (fn: () => void) => (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      fn();
    } else if (e.key === "Escape") {
      e.preventDefault();
      setMode("idle");
    }
  };

  if (mode === "warn") {
    const send = () => go(() => actions.warn(target, reason));
    return (
      <div className="mod-actions expanded">
        <span className="label">Warn {target.userLogin}:</span>
        <input className="reason" value={reason} onChange={(e) => setReason(e.target.value)} onKeyDown={onEnter(send)} placeholder="Reason they'll see" autoFocus />
        <button className="primary" disabled={busy} onClick={send}>
          Send warning
        </button>
        <button className="ghost" onClick={() => setMode("idle")}>
          Cancel
        </button>
      </div>
    );
  }

  if (mode === "timeout") {
    return (
      <div className="mod-actions expanded">
        <span className="label">Time out {target.userLogin}:</span>
        {TIMEOUTS.map((t) => (
          <button key={t.secs} disabled={busy} onClick={() => go(() => actions.timeout(target, t.secs, reason))}>
            {t.label}
          </button>
        ))}
        <input
          className="reason"
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          onKeyDown={onEnter(() => go(() => actions.timeout(target, 600, reason)))}
          placeholder="Reason (optional)"
          autoFocus
        />
        <button className="ghost" onClick={() => setMode("idle")}>
          Cancel
        </button>
      </div>
    );
  }

  if (mode === "ban") {
    const send = () => go(() => (everywhere ? actions.banEverywhere(target, reason) : actions.ban(target, reason)));
    return (
      <div className="mod-actions expanded">
        <span className="label">
          Ban {target.userLogin} {everywhere ? `from all ${modChannelCount} channels` : `from #${target.channel}`}?
        </span>
        <input className="reason" value={reason} onChange={(e) => setReason(e.target.value)} onKeyDown={onEnter(send)} placeholder="Reason (optional)" autoFocus />
        {modChannelCount > 1 && (
          <label className="check small-check">
            <input type="checkbox" checked={everywhere} onChange={(e) => setEverywhere(e.target.checked)} />
            All my channels
          </label>
        )}
        <button className="danger solid" disabled={busy} onClick={send}>
          Ban
        </button>
        <button className="ghost" onClick={() => setMode("idle")}>
          Cancel
        </button>
      </div>
    );
  }

  return (
    <div className="mod-actions">
      {suggestion && (
        <button
          className={`suggested ${suggestion.step.action}`}
          disabled={busy}
          onClick={runSuggestion}
          title={`Suggested for a ${ordinal(suggestion.prior + 1)} offense here (Enter)`}
        >
          {stepLabel(suggestion.step)}
          <span className="offense">{ordinal(suggestion.prior + 1)}</span>
        </button>
      )}
      {target.messageId && (
        <button disabled={busy} onClick={() => go(() => actions.deleteMessage(target))} title="Delete this message (X)">
          Delete
        </button>
      )}
      {suggestion?.step.action !== "warn" && (
        <button disabled={busy} onClick={() => setMode("warn")} title="Warn (W)">
          Warn
        </button>
      )}
      <button disabled={busy} onClick={() => setMode("timeout")} title="Timeout (T)">
        Timeout ▾
      </button>
      {suggestion?.step.action !== "ban" && (
        <button className="danger" disabled={busy} onClick={() => setMode("ban")} title="Ban (B)">
          Ban
        </button>
      )}
      {showUnban && (
        <button disabled={busy} onClick={() => go(() => actions.unban(target))}>
          Unban
        </button>
      )}
    </div>
  );
}
