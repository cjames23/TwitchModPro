import { useState } from "react";
import type { Flag, FlagStatus, Severity } from "../types";
import { api } from "../api";
import { channelColor, clockTime, highlight, timeAgo } from "../util";

interface Props {
  flags: Flag[];
  channelFilter: string | null;
  now: number;
  onStatus: (id: number, status: FlagStatus) => void;
  onClearClosed: () => void;
}

type StatusFilter = "open" | "all";

export function FlagQueue({ flags, channelFilter, now, onStatus, onClearClosed }: Props) {
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("open");
  const [severityFilter, setSeverityFilter] = useState<Severity | "any">("any");
  const [expanded, setExpanded] = useState<number | null>(null);

  const visible = flags.filter(
    (f) =>
      (statusFilter === "all" || f.status === "open") &&
      (severityFilter === "any" || f.severity === severityFilter) &&
      (!channelFilter || f.channel === channelFilter),
  );
  const closedCount = flags.filter((f) => f.status !== "open").length;

  return (
    <section className="panel">
      <header className="panel-header">
        <h2>
          Flag queue {channelFilter && <span className="sub">in #{channelFilter}</span>}
        </h2>
        <div className="filters">
          <div className="seg">
            {(["open", "all"] as const).map((s) => (
              <button key={s} className={statusFilter === s ? "on" : ""} onClick={() => setStatusFilter(s)}>
                {s === "open" ? "Open" : "All"}
              </button>
            ))}
          </div>
          <select value={severityFilter} onChange={(e) => setSeverityFilter(e.target.value as Severity | "any")}>
            <option value="any">Any severity</option>
            <option value="high">High</option>
            <option value="medium">Medium</option>
            <option value="low">Low</option>
          </select>
          {closedCount > 0 && (
            <button className="ghost" onClick={onClearClosed} title="Delete reviewed and dismissed flags">
              Clear {closedCount} closed
            </button>
          )}
        </div>
      </header>

      <div className="flag-list">
        {visible.length === 0 && (
          <div className="empty">
            <div className="empty-icon">✓</div>
            <p>{statusFilter === "open" ? "Nothing to review right now." : "No flags yet."}</p>
            <p className="muted-text">Messages that match your rules will show up here.</p>
          </div>
        )}
        {visible.map((f) => {
          const terms = f.matches.flatMap((m) => m.terms);
          const isOpen = expanded === f.id;
          return (
            <article key={f.id} className={`flag-card sev-${f.severity} status-${f.status}`}>
              <div className="flag-top">
                <span className="chan-tag" style={{ borderColor: channelColor(f.channel), color: channelColor(f.channel) }}>
                  #{f.channel}
                </span>
                <span className={`sev-badge ${f.severity}`}>{f.severity}</span>
                {f.matches.map((m) => (
                  <span key={m.ruleId} className="rule-chip">
                    {m.ruleName}
                  </span>
                ))}
                <span className="spacer" />
                <time title={new Date(f.timestamp).toLocaleString()}>{timeAgo(f.timestamp, now)}</time>
              </div>
              <div className="flag-msg">
                <strong>{f.displayName}</strong>
                {f.displayName.toLowerCase() !== f.userLogin && <span className="login"> ({f.userLogin})</span>}:{" "}
                <span className="msg-text">{highlight(f.text, terms)}</span>
              </div>
              {isOpen && (
                <div className="context">
                  <div className="context-label">Earlier in #{f.channel}</div>
                  {f.context.length === 0 && <div className="muted-text">No earlier messages captured.</div>}
                  {f.context.map((c, i) => (
                    <div key={i} className="context-line">
                      <span className="ctx-time">{clockTime(c.timestamp)}</span>
                      <strong>{c.displayName}</strong>: {c.text}
                    </div>
                  ))}
                  <div className="context-line flagged">
                    <span className="ctx-time">{clockTime(f.timestamp)}</span>
                    <strong>{f.displayName}</strong>: {highlight(f.text, terms)}
                  </div>
                </div>
              )}
              <div className="flag-actions">
                {f.status === "open" ? (
                  <>
                    <button className="primary" onClick={() => onStatus(f.id, "reviewed")}>
                      Reviewed
                    </button>
                    <button onClick={() => onStatus(f.id, "dismissed")}>Dismiss</button>
                  </>
                ) : (
                  <>
                    <span className="status-label">{f.status}</span>
                    <button onClick={() => onStatus(f.id, "open")}>Reopen</button>
                  </>
                )}
                <button className="ghost" onClick={() => setExpanded(isOpen ? null : f.id)}>
                  {isOpen ? "Hide context" : "Context"}
                </button>
                <button className="ghost" onClick={() => api.openChat(f.channel)}>
                  Open chat ↗
                </button>
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}
