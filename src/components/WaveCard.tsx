import { useState } from "react";
import type { Wave } from "../types";
import { channelColor, timeAgo } from "../util";

export interface WaveHandlers {
  act: (wave: Wave, action: "ban" | "timeout", seconds: number | null, reason: string, exclude: string[]) => Promise<void>;
  undo: (wave: Wave) => Promise<void>;
  dismiss: (wave: Wave) => Promise<void>;
}

function headline(w: Wave) {
  const n = w.members.length;
  switch (w.kind) {
    case "sameMessage":
      return `${n} accounts posting the same message`;
    case "similarNames":
      return `${n} accounts with names like “${w.key}…”`;
    case "firstTimeBurst":
      return `${n} first-time chatters at once`;
  }
}

export function WaveCard({ wave, now, handlers }: { wave: Wave; now: number; handlers: WaveHandlers }) {
  const [open, setOpen] = useState(false);
  const [exclude, setExclude] = useState<Set<string>>(new Set());
  const [reason, setReason] = useState("Spam wave");
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState<null | "ban" | "timeout">(null);

  const actionable = (o: string | null) => !o || o.startsWith("failed") || o === "unbanned";
  const targets = wave.members.filter((m) => !exclude.has(m.login) && actionable(m.outcome));
  const acted = wave.members.filter((m) => m.outcome && !m.outcome.startsWith("failed") && m.outcome !== "unbanned");
  const sample = wave.members[0]?.text ?? "";

  async function run(fn: () => Promise<void>) {
    setBusy(true);
    try {
      await fn();
      setConfirm(null);
    } finally {
      setBusy(false);
    }
  }

  return (
    <article className={`wave-card sev-${wave.severity} status-${wave.status}`}>
      <div className="flag-top">
        <span className="wave-tag">Spam wave</span>
        <span className={`sev-badge ${wave.severity}`}>{wave.severity}</span>
        {wave.channels.map((c) => (
          <span key={c} className="chan-tag" style={{ borderColor: channelColor(c), color: channelColor(c) }}>
            #{c}
          </span>
        ))}
        <span className="spacer" />
        <time>{timeAgo(wave.firstSeen, now)}</time>
      </div>
      <div className="wave-headline">{headline(wave)}</div>
      {wave.kind === "sameMessage" && <div className="wave-sample">“{sample}”</div>}
      {wave.kind === "firstTimeBurst" && (
        <div className="reasons">This can be normal right after a raid. Check the list before acting.</div>
      )}

      {open && (
        <div className="wave-members">
          {wave.members.map((m) => (
            <label key={`${m.channel}:${m.login}`} className={`wave-member ${m.outcome ? "done" : ""}`}>
              <input
                type="checkbox"
                checked={!exclude.has(m.login)}
                disabled={!actionable(m.outcome)}
                onChange={(e) =>
                  setExclude((s) => {
                    const n = new Set(s);
                    if (e.target.checked) n.delete(m.login);
                    else n.add(m.login);
                    return n;
                  })
                }
              />
              <strong>{m.displayName}</strong>
              {m.firstMessage && <span className="badge f">1st</span>}
              <span className="muted-text">#{m.channel}</span>
              <span className="msg-text">{m.text}</span>
              {m.outcome && <span className={`outcome ${m.outcome.startsWith("failed") ? "bad" : ""}`}>{m.outcome}</span>}
            </label>
          ))}
        </div>
      )}

      {confirm ? (
        <div className="mod-actions expanded">
          <span className="label">
            {confirm === "ban" ? "Ban" : "Time out for 10m"} {targets.length} account{targets.length === 1 ? "" : "s"}?
          </span>
          <input className="reason" value={reason} onChange={(e) => setReason(e.target.value)} placeholder="Reason" autoFocus />
          <button
            className={confirm === "ban" ? "danger solid" : "primary"}
            disabled={busy || !targets.length}
            onClick={() => run(() => handlers.act(wave, confirm, confirm === "ban" ? null : 600, reason, [...exclude]))}
          >
            {busy ? "Working…" : confirm === "ban" ? `Ban ${targets.length}` : `Time out ${targets.length}`}
          </button>
          <button className="ghost" onClick={() => setConfirm(null)} disabled={busy}>
            Cancel
          </button>
        </div>
      ) : (
        <div className="flag-actions">
          {wave.status !== "dismissed" && targets.length > 0 && (
            <>
              <button className="danger solid" onClick={() => setConfirm("ban")}>
                Ban all {targets.length}
              </button>
              <button onClick={() => setConfirm("timeout")}>Time out all 10m</button>
            </>
          )}
          {acted.length > 0 && (
            <button disabled={busy} onClick={() => run(() => handlers.undo(wave))} title="Unban everyone this app banned or timed out from this wave">
              Undo ({acted.length})
            </button>
          )}
          {wave.status === "open" && (
            <button className="ghost" onClick={() => handlers.dismiss(wave)} title="Hide this and ignore the same pattern for 30 minutes">
              Not spam
            </button>
          )}
          <span className="spacer" />
          <button className="ghost" onClick={() => setOpen((o) => !o)}>
            {open ? "Hide accounts" : `Show ${wave.members.length} accounts`}
          </button>
        </div>
      )}
    </article>
  );
}
