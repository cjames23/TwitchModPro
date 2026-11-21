import { useEffect, useMemo, useRef, useState } from "react";
import type { Channel, Flag, FlagKind, FlagStatus, LadderStep, ModAction, Severity, Wave } from "../types";
import { api } from "../api";
import { channelColor, clockTime, highlight, timeAgo } from "../util";
import { accountAge, actionLabel, isNewAccount, offenseCount, otherChannelNotes, suggestedStep } from "../intel";
import { ModActions, type ActionHandlers, type ActionMode } from "./ModActions";
import { WaveCard, type WaveHandlers } from "./WaveCard";

export interface InboxHandlers {
  automod: (flag: Flag, allow: boolean) => Promise<void>;
  unbanRequest: (flag: Flag, approve: boolean, text: string) => Promise<void>;
}

export interface Intel {
  byLogin: Map<string, ModAction[]>;
  ages: Record<string, number>;
  ladder: LadderStep[];
  lookbackDays: number;
}

interface Props {
  flags: Flag[];
  waves: Wave[];
  channels: Channel[];
  channelFilter: string | null;
  now: number;
  actions: ActionHandlers;
  inbox: InboxHandlers;
  waveHandlers: WaveHandlers;
  intel: Intel;
  keyboardEnabled: boolean;
  onStatus: (id: number, status: FlagStatus) => void;
  onClearClosed: () => void;
  onOpenUser: (channel: string, login: string) => void;
}

type StatusFilter = "open" | "all";

const KIND_LABEL: Record<FlagKind, string> = {
  rule: "Rule flags",
  automod: "AutoMod",
  unbanRequest: "Unban requests",
  suspicious: "Suspicious users",
  evasion: "Possible ban evaders",
};

const SHORTCUTS: [string, string][] = [
  ["J / K or ↓ / ↑", "Next / previous"],
  ["Enter", "Suggested action"],
  ["W", "Warn"],
  ["T", "Timeout"],
  ["B", "Ban"],
  ["X", "Delete message"],
  ["R", "Mark reviewed"],
  ["D", "Dismiss"],
  ["A / N", "Approve / deny (AutoMod, unban requests)"],
  ["U", "Open user history"],
  ["C", "Show context"],
  ["?", "Show or hide this list"],
];

function isTyping(el: EventTarget | null) {
  const t = el as HTMLElement | null;
  return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
}

function InboxActions({ flag, inbox }: { flag: Flag; inbox: InboxHandlers }) {
  const [busy, setBusy] = useState(false);
  const [reply, setReply] = useState("");
  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
    } finally {
      setBusy(false);
    }
  };
  if (flag.kind === "automod") {
    return (
      <>
        <button className="primary" disabled={busy} onClick={() => run(() => inbox.automod(flag, true))} title="Approve (A)">
          Approve
        </button>
        <button className="danger" disabled={busy} onClick={() => run(() => inbox.automod(flag, false))} title="Deny (N)">
          Deny
        </button>
      </>
    );
  }
  return (
    <>
      <input className="reason" value={reply} onChange={(e) => setReply(e.target.value)} placeholder="Reply to the user (optional)" />
      <button className="primary" disabled={busy} onClick={() => run(() => inbox.unbanRequest(flag, true, reply))} title="Approve (A)">
        Unban
      </button>
      <button className="danger" disabled={busy} onClick={() => run(() => inbox.unbanRequest(flag, false, reply))} title="Deny (N)">
        Deny
      </button>
    </>
  );
}

export function FlagQueue(p: Props) {
  const { flags, waves, channels, channelFilter, now, actions, inbox, intel } = p;
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("open");
  const [severityFilter, setSeverityFilter] = useState<Severity | "any">("any");
  const [kindFilter, setKindFilter] = useState<FlagKind | "any">("any");
  const [expanded, setExpanded] = useState<number | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [modeFor, setModeFor] = useState<{ id: number; mode: ActionMode } | null>(null);
  const [showHelp, setShowHelp] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);

  const modChannels = channels.filter((c) => c.isMod);
  const modIn = new Set(modChannels.map((c) => c.name));
  const visible = useMemo(
    () =>
      flags.filter(
        (f) =>
          (statusFilter === "all" || f.status === "open") &&
          (severityFilter === "any" || f.severity === severityFilter) &&
          (kindFilter === "any" || f.kind === kindFilter) &&
          (!channelFilter || f.channel === channelFilter),
      ),
    [flags, statusFilter, severityFilter, kindFilter, channelFilter],
  );
  const shownWaves = waves.filter(
    (w) =>
      w.status !== "dismissed" &&
      // Keep recently actioned waves in view so Undo stays reachable.
      (statusFilter === "all" || w.status === "open" || now - w.lastSeen < 30 * 60_000) &&
      (!channelFilter || w.channels.includes(channelFilter)),
  );
  const closedCount = flags.filter((f) => f.status !== "open").length;
  const current = visible.find((f) => f.id === selected) ?? visible[0];

  function suggestionFor(f: Flag) {
    const prior = offenseCount(intel.byLogin, f.channel, f.userLogin, intel.lookbackDays, now);
    return { step: suggestedStep(intel.ladder, prior), prior };
  }

  // Keep the selected card in view.
  useEffect(() => {
    if (!current) return;
    listRef.current?.querySelector(`[data-flag="${current.id}"]`)?.scrollIntoView({ block: "nearest" });
  }, [current?.id]);

  useEffect(() => {
    if (!p.keyboardEnabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e.target) || e.metaKey || e.ctrlKey || e.altKey) return;
      const key = e.key;
      if (key === "?") {
        setShowHelp((h) => !h);
        return;
      }
      if (!visible.length) return;
      const idx = Math.max(0, visible.findIndex((f) => f.id === current?.id));
      const f = current;
      const target = f && { channel: f.channel, userLogin: f.userLogin, userId: f.userId, messageId: f.messageId || undefined };
      const wellbeing = f && f.matches.length > 0 && f.matches.every((m) => m.wellbeing);
      const canAct = f && modIn.has(f.channel) && f.status === "open" && !wellbeing;
      const isInbox = f && (f.kind === "automod" || f.kind === "unbanRequest");
      const handled = () => e.preventDefault();
      switch (key.toLowerCase()) {
        case "j":
        case "arrowdown":
          handled();
          setSelected(visible[Math.min(visible.length - 1, idx + 1)].id);
          break;
        case "k":
        case "arrowup":
          handled();
          setSelected(visible[Math.max(0, idx - 1)].id);
          break;
        case "r":
          if (f) p.onStatus(f.id, "reviewed");
          break;
        case "d":
          if (f) p.onStatus(f.id, "dismissed");
          break;
        case "u":
          if (f) p.onOpenUser(f.channel, f.userLogin);
          break;
        case "c":
          if (f) setExpanded((x) => (x === f.id ? null : f.id));
          break;
        case "a":
          if (f && canAct && isInbox) f.kind === "automod" ? inbox.automod(f, true) : inbox.unbanRequest(f, true, "");
          break;
        case "n":
          if (f && canAct && isInbox) f.kind === "automod" ? inbox.automod(f, false) : inbox.unbanRequest(f, false, "");
          break;
        case "x":
          if (canAct && !isInbox && target?.messageId) actions.deleteMessage(target);
          break;
        case "w":
          if (canAct && !isInbox) (handled(), setModeFor({ id: f.id, mode: "warn" }));
          break;
        case "t":
          if (canAct && !isInbox) (handled(), setModeFor({ id: f.id, mode: "timeout" }));
          break;
        case "b":
          if (canAct && !isInbox) (handled(), setModeFor({ id: f.id, mode: "ban" }));
          break;
        case "enter":
          if (canAct && !isInbox && target) {
            handled();
            const { step } = suggestionFor(f);
            if (step.action === "timeout") actions.timeout(target, step.durationSecs ?? 600, f.matches.map((m) => m.ruleName).join(", "));
            else setModeFor({ id: f.id, mode: step.action });
          }
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

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
          <select value={kindFilter} onChange={(e) => setKindFilter(e.target.value as FlagKind | "any")}>
            <option value="any">Everything</option>
            {(Object.keys(KIND_LABEL) as FlagKind[]).map((k) => (
              <option key={k} value={k}>
                {KIND_LABEL[k]}
              </option>
            ))}
          </select>
          <select value={severityFilter} onChange={(e) => setSeverityFilter(e.target.value as Severity | "any")}>
            <option value="any">Any severity</option>
            <option value="high">High</option>
            <option value="medium">Medium</option>
            <option value="low">Low</option>
          </select>
          {closedCount > 0 && (
            <button className="ghost" onClick={p.onClearClosed} title="Delete reviewed and dismissed flags">
              Clear {closedCount} closed
            </button>
          )}
          <button className="ghost" onClick={() => setShowHelp((h) => !h)} title="Keyboard shortcuts (?)">
            ⌨
          </button>
        </div>
      </header>

      {showHelp && (
        <div className="shortcut-help">
          {SHORTCUTS.map(([k, d]) => (
            <div key={k}>
              <kbd>{k}</kbd> {d}
            </div>
          ))}
        </div>
      )}

      <div className="flag-list" ref={listRef}>
        {kindFilter === "any" && shownWaves.map((w) => <WaveCard key={w.id} wave={w} now={now} handlers={p.waveHandlers} />)}

        {visible.length === 0 && shownWaves.length === 0 && (
          <div className="empty">
            <div className="empty-icon">✓</div>
            <p>{statusFilter === "open" ? "Nothing to review right now." : "No flags yet."}</p>
            <p className="muted-text">
              Rule matches, AutoMod holds, unban requests and suspicious users from all your channels show up here.
            </p>
          </div>
        )}
        {visible.map((f) => {
          const terms = f.matches.flatMap((m) => m.terms);
          const reasons = f.matches.map((m) => m.reason).filter(Boolean) as string[];
          const isOpen = expanded === f.id;
          const canAct = modIn.has(f.channel) && f.status === "open";
          const isInbox = f.kind === "automod" || f.kind === "unbanRequest";
          const wellbeing = f.matches.length > 0 && f.matches.every((m) => m.wellbeing);
          const created = f.meta?.accountCreatedAt ?? intel.ages[f.userLogin];
          const others = otherChannelNotes(intel.byLogin, f.channel, f.userLogin);
          const selectedNow = current?.id === f.id;
          const target = { channel: f.channel, userLogin: f.userLogin, userId: f.userId, messageId: f.messageId || undefined };
          return (
            <article
              key={f.id}
              data-flag={f.id}
              className={`flag-card sev-${f.severity} status-${f.status} kind-${f.kind} ${wellbeing ? "wellbeing" : ""} ${selectedNow ? "selected" : ""}`}
              onClick={() => setSelected(f.id)}
            >
              <div className="flag-top">
                <span className="chan-tag" style={{ borderColor: channelColor(f.channel), color: channelColor(f.channel) }}>
                  #{f.channel}
                </span>
                {f.kind !== "rule" && <span className={`kind-tag ${f.kind}`}>{KIND_LABEL[f.kind].replace(/s$/, "")}</span>}
                {wellbeing && <span className="kind-tag wellbeing">Wellbeing check</span>}
                <span className={`sev-badge ${f.severity}`}>{f.severity}</span>
                {f.kind === "rule" &&
                  f.matches.map((m) => (
                    <span key={m.ruleId} className="rule-chip">
                      {m.ruleName}
                    </span>
                  ))}
                <span className="spacer" />
                <time title={new Date(f.timestamp).toLocaleString()}>{timeAgo(f.timestamp, now)}</time>
              </div>
              <div className="flag-msg">
                <button className="user-link" onClick={() => p.onOpenUser(f.channel, f.userLogin)} title="Show this user's history (U)">
                  {f.displayName}
                </button>
                {f.displayName.toLowerCase() !== f.userLogin && <span className="login"> ({f.userLogin})</span>}
                {f.kind === "unbanRequest" ? " asks to be unbanned: " : ": "}
                <span className="msg-text">{f.text ? highlight(f.text, terms) : <em className="muted-text">(no message)</em>}</span>
              </div>
              {reasons.length > 0 && <div className="reasons">{reasons.join(" · ")}</div>}
              {f.kind === "evasion" && f.meta.evasionOf && (
                <div className="reasons">
                  <button
                    className="link-button"
                    onClick={() => p.onOpenUser(f.meta.evasionChannel ?? f.channel, f.meta.evasionOf!)}
                  >
                    Compare with {f.meta.evasionOf}'s history
                  </button>{" "}
                  · Names alone can match by chance; check how they talk before acting.
                </div>
              )}
              {wellbeing && (
                <div className="wellbeing-note">
                  This chatter may be struggling. Consider checking in with them or letting the streamer know, rather than
                  punishing them.
                </div>
              )}

              {(created || others.length > 0) && (
                <div className="intel-notes">
                  {created && (
                    <span className={`note ${isNewAccount(created, now) ? "warn" : ""}`} title={new Date(created).toLocaleDateString()}>
                      Account {accountAge(created, now)}
                    </span>
                  )}
                  {others.map((a) => (
                    <span key={a.id} className="note warn" title={a.byApp ? "Done from this app" : "Done by a mod (seen in chat)"}>
                      {actionLabel(a)} in #{a.channel} {timeAgo(a.timestamp, now)}
                    </span>
                  ))}
                </div>
              )}

              {isOpen && (
                <div className="context">
                  <div className="context-label">Conversation in #{f.channel}</div>
                  {f.context.map((c, i) => (
                    <div key={`b${i}`} className="context-line">
                      <span className="ctx-time">{clockTime(c.timestamp)}</span>
                      <strong>{c.displayName}</strong>: {c.text}
                    </div>
                  ))}
                  <div className="context-line flagged">
                    <span className="ctx-time">{clockTime(f.timestamp)}</span>
                    <strong>{f.displayName}</strong>: {highlight(f.text, terms)}
                  </div>
                  {f.contextAfter.map((c, i) => (
                    <div key={`a${i}`} className="context-line">
                      <span className="ctx-time">{clockTime(c.timestamp)}</span>
                      <strong>{c.displayName}</strong>: {c.text}
                    </div>
                  ))}
                  {f.contextAfter.length === 0 && f.kind === "rule" && (
                    <div className="muted-text">No messages after this one yet.</div>
                  )}
                </div>
              )}
              <div className="flag-actions">
                {f.status === "open" ? (
                  <>
                    {!isInbox && (
                      <>
                        <button className="primary" onClick={() => p.onStatus(f.id, "reviewed")} title="Mark reviewed (R)">
                          Reviewed
                        </button>
                        <button onClick={() => p.onStatus(f.id, "dismissed")} title="Dismiss (D)">
                          Dismiss
                        </button>
                      </>
                    )}
                  </>
                ) : (
                  <>
                    <span className="status-label">{f.resolution ?? f.status}</span>
                    {!isInbox && <button onClick={() => p.onStatus(f.id, "open")}>Reopen</button>}
                  </>
                )}
                {canAct && !wellbeing && (
                  <>
                    {!isInbox && <span className="divider" />}
                    {isInbox ? (
                      <InboxActions flag={f} inbox={inbox} />
                    ) : (
                      <ModActions
                        target={target}
                        actions={actions}
                        defaultReason={f.matches.map((m) => m.ruleName).join(", ")}
                        suggestion={suggestionFor(f)}
                        modChannelCount={modChannels.length}
                        mode={modeFor?.id === f.id ? modeFor.mode : "idle"}
                        onModeChange={(mode) => setModeFor(mode === "idle" ? null : { id: f.id, mode })}
                      />
                    )}
                  </>
                )}
                <span className="spacer" />
                {(f.kind === "rule" || f.kind === "evasion") && (
                  <button className="ghost" onClick={() => setExpanded(isOpen ? null : f.id)} title="Context (C)">
                    {isOpen ? "Hide context" : "Context"}
                  </button>
                )}
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
