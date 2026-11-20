import { Fragment, useEffect, useMemo, useState } from "react";
import type { PatternStats, Rule, RuleStats, Severity } from "../types";
import { api } from "../api";
import { highlight, timeAgo } from "../util";

interface Props {
  rules: Rule[];
  onRulesChanged: (rules: Rule[]) => void;
  onEditRule: (rule: Rule) => void;
  toast: (text: string, kind?: "info" | "ok" | "error") => void;
}

const SEV_UP: Record<Severity, Severity | null> = { low: "medium", medium: "high", high: null };
const SEV_DOWN: Record<Severity, Severity | null> = { low: null, medium: "low", high: "medium" };

/** Share of decided flags (acted on or dismissed) that were acted on. */
export function precision(s: { actioned: number; dismissed: number }) {
  const decided = s.actioned + s.dismissed;
  return decided ? s.actioned / decided : null;
}

const pct = (x: number | null) => (x === null ? "–" : `${Math.round(x * 100)}%`);

interface Suggestion {
  key: string;
  rule: Rule;
  tone: "warn" | "good" | "info";
  text: string;
  actions: { label: string; primary?: boolean; run: () => Promise<void> | void }[];
}

export function Tuning({ rules, onRulesChanged, onEditRule, toast }: Props) {
  const [days, setDays] = useState(30);
  const [stats, setStats] = useState<RuleStats[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const [openRule, setOpenRule] = useState<number | null>(null);
  const [exceptionDraft, setExceptionDraft] = useState<Record<number, string>>({});

  async function load() {
    try {
      setStats(await api.ruleStats(days));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    load();
    const t = setInterval(load, 30_000);
    return () => clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [days]);

  const byId = useMemo(() => new Map((stats ?? []).map((s) => [s.ruleId, s])), [stats]);
  const statFor = (id: number): RuleStats =>
    byId.get(id) ?? { ruleId: id, flags: 0, actioned: 0, reviewed: 0, dismissed: 0, open: 0, lastFired: null, patterns: [], testHits: 0, testSamples: [] };

  async function save(rule: Rule, message: string) {
    try {
      onRulesChanged(await api.saveRule(rule));
      toast(message, "ok");
      load();
    } catch (e) {
      toast(String(e), "error");
    }
  }

  const removeWord = (rule: Rule, word: string) =>
    save({ ...rule, patterns: rule.patterns.filter((p) => p !== word) }, `Removed “${word}” from ${rule.name}`);
  const addException = (rule: Rule, phrase: string) =>
    save({ ...rule, exceptions: [...rule.exceptions, phrase] }, `Added exception “${phrase}” to ${rule.name}`);
  const setMode = (rule: Rule, mode: Rule["mode"]) =>
    save({ ...rule, mode }, mode === "active" ? `${rule.name} is on: it now creates flags` : `${rule.name} is back in test mode`);

  const suggestions: Suggestion[] = useMemo(() => {
    const out: Suggestion[] = [];
    for (const rule of rules.filter((r) => r.enabled)) {
      const s = statFor(rule.id);
      if (rule.mode === "test") {
        if (s.testHits > 0) {
          out.push({
            key: `test-${rule.id}`,
            rule,
            tone: "info",
            text: `Would have flagged ${s.testHits} message${s.testHits === 1 ? "" : "s"} in ${days} days. Check the examples below, then turn it on or keep testing.`,
            actions: [{ label: "Turn on", primary: true, run: () => setMode(rule, "active") }],
          });
        }
        continue;
      }
      const p = precision(s);
      const decided = s.actioned + s.dismissed;
      if (decided >= 8 && p !== null && p <= 0.3) {
        const down = SEV_DOWN[rule.severity];
        out.push({
          key: `noisy-${rule.id}`,
          rule,
          tone: "warn",
          text: `Mostly false alarms: ${s.dismissed} of ${decided} decided flags were dismissed.`,
          actions: [
            ...(down ? [{ label: `Lower to ${down}`, run: () => save({ ...rule, severity: down }, `${rule.name} lowered to ${down}`) }] : []),
            { label: "Back to test mode", run: () => setMode(rule, "test") },
            { label: "Edit rule", run: () => onEditRule(rule) },
          ],
        });
      }
      for (const w of s.patterns) {
        if (w.flags >= 4 && w.dismissed / w.flags >= 0.75 && rule.patterns.includes(w.pattern) && rule.patterns.length > 1) {
          out.push({
            key: `word-${rule.id}-${w.pattern}`,
            rule,
            tone: "warn",
            text: `“${w.pattern}” was dismissed ${w.dismissed} of ${w.flags} times.`,
            actions: [{ label: "Remove word", primary: true, run: () => removeWord(rule, w.pattern) }],
          });
        }
      }
      const up = SEV_UP[rule.severity];
      if (s.flags >= 10 && s.actioned / s.flags >= 0.9 && up) {
        out.push({
          key: `strong-${rule.id}`,
          rule,
          tone: "good",
          text: `${s.actioned} of ${s.flags} flags led to an action. It's reliable, so a higher severity would alert you sooner.`,
          actions: [{ label: `Raise to ${up}`, primary: true, run: () => save({ ...rule, severity: up }, `${rule.name} raised to ${up}`) }],
        });
      }
    }
    return out.filter((x) => !hidden.has(x.key));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rules, stats, hidden, days]);

  const testRules = rules.filter((r) => r.mode === "test");
  const activeRules = rules.filter((r) => r.mode !== "test");
  const quiet = activeRules.filter((r) => r.enabled && statFor(r.id).flags === 0);

  function wordRow(rule: Rule, w: PatternStats) {
    const p = precision(w);
    return (
      <tr key={w.pattern}>
        <td>
          <code>{w.pattern}</code>
        </td>
        <td className="num">{w.flags}</td>
        <td className="num">{w.actioned}</td>
        <td className="num">{w.dismissed}</td>
        <td className="num">{pct(p)}</td>
        <td className="row-actions">
          {rule.patterns.includes(w.pattern) && rule.patterns.length > 1 && (
            <button className="ghost small" onClick={() => removeWord(rule, w.pattern)}>
              Remove word
            </button>
          )}
        </td>
      </tr>
    );
  }

  function exceptionInput(rule: Rule) {
    const draft = exceptionDraft[rule.id] ?? "";
    return (
      <form
        className="row-gap exception-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (draft.trim()) {
            addException(rule, draft.trim());
            setExceptionDraft((d) => ({ ...d, [rule.id]: "" }));
          }
        }}
      >
        <input
          value={draft}
          onChange={(e) => setExceptionDraft((d) => ({ ...d, [rule.id]: e.target.value }))}
          placeholder="Add an exception phrase (e.g. “die hard”)"
        />
        <button disabled={!draft.trim()}>Add exception</button>
        {rule.exceptions.length > 0 && <span className="hint">Current: {rule.exceptions.join(", ")}</span>}
      </form>
    );
  }

  return (
    <section className="panel">
      <header className="panel-header">
        <h2>Rule tuning</h2>
        <div className="filters">
          <div className="seg">
            {[7, 30].map((d) => (
              <button key={d} className={days === d ? "on" : ""} onClick={() => setDays(d)}>
                {d} days
              </button>
            ))}
          </div>
          <button
            className="ghost"
            onClick={async () => {
              try {
                const before = rules.length;
                const next = await api.restoreDefaultPacks();
                onRulesChanged(next);
                toast(next.length > before ? `Restored ${next.length - before} default rules (in test mode)` : "All default rules are already there", "ok");
              } catch (e) {
                toast(String(e), "error");
              }
            }}
          >
            Restore default rules
          </button>
        </div>
      </header>

      <div className="tuning">
        {error && <div className="form-error">{error}</div>}
        {!stats && !error && <p className="muted-text">Loading…</p>}

        {stats && (
          <>
            <section>
              <h3>Suggestions</h3>
              {suggestions.length === 0 && (
                <p className="muted-text">Nothing to change right now. Suggestions appear once rules have flagged enough messages.</p>
              )}
              {suggestions.map((sg) => (
                <div key={sg.key} className={`suggestion ${sg.tone}`}>
                  <div>
                    <strong>{sg.rule.name}</strong>
                    {sg.rule.pack && <span className="pack-tag">{sg.rule.pack}</span>}
                    <div>{sg.text}</div>
                  </div>
                  <div className="row-gap">
                    {sg.actions.map((a) => (
                      <button key={a.label} className={a.primary ? "primary" : ""} onClick={() => a.run()}>
                        {a.label}
                      </button>
                    ))}
                    <button className="ghost" onClick={() => setHidden((h) => new Set(h).add(sg.key))}>
                      Ignore
                    </button>
                  </div>
                </div>
              ))}
            </section>

            <section>
              <h3>In test mode ({testRules.length})</h3>
              <p className="muted-text">
                These rules don't create flags yet. They record what they would have flagged, so you can check them against
                your chat first.
              </p>
              {testRules.map((rule) => {
                const s = statFor(rule.id);
                const open = openRule === rule.id;
                return (
                  <div key={rule.id} className={`tune-card ${rule.enabled ? "" : "disabled"}`}>
                    <div className="tune-head">
                      <button className="rule-main" onClick={() => setOpenRule(open ? null : rule.id)}>
                        <span className="rule-title">
                          {rule.name} <span className={`sev-badge ${rule.severity}`}>{rule.severity}</span>
                          {rule.pack && <span className="pack-tag">{rule.pack}</span>}
                          {!rule.enabled && <span className="muted-text">(off)</span>}
                        </span>
                      </button>
                      <span className={`tune-count ${s.testHits ? "hot" : ""}`}>
                        {s.testHits ? `would have flagged ${s.testHits}` : "no matches yet"}
                      </span>
                      <button className="primary small" onClick={() => setMode({ ...rule, enabled: true }, "active")}>
                        Turn on
                      </button>
                      <button className="ghost small" onClick={() => onEditRule(rule)}>
                        Edit
                      </button>
                    </div>
                    {open && (
                      <div className="tune-body">
                        {s.testSamples.length === 0 && <p className="muted-text">Nothing matched in the last {days} days.</p>}
                        {s.testSamples.map((h, i) => (
                          <div key={i} className="history-line">
                            <span className="ctx-time">{timeAgo(h.timestamp)}</span>
                            <span className="muted-text">#{h.channel}</span>
                            <strong>{h.displayName}</strong>
                            <span className="msg-text">{highlight(h.text, h.terms)}</span>
                            {h.reason && <span className="hint">{h.reason}</span>}
                          </div>
                        ))}
                        {exceptionInput(rule)}
                      </div>
                    )}
                  </div>
                );
              })}
            </section>

            <section>
              <h3>Active rules</h3>
              <table className="tune-table">
                <thead>
                  <tr>
                    <th>Rule</th>
                    <th className="num">Flags</th>
                    <th className="num">Acted on</th>
                    <th className="num">Dismissed</th>
                    <th className="num" title="Acted on ÷ (acted on + dismissed)">
                      Accuracy
                    </th>
                    <th>Last fired</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {activeRules.map((rule) => {
                    const s = statFor(rule.id);
                    const p = precision(s);
                    const open = openRule === rule.id;
                    return (
                      <Fragment key={rule.id}>
                        <tr className={`${rule.enabled ? "" : "disabled"} ${open ? "open" : ""}`}>
                          <td>
                            <button className="rule-main" onClick={() => setOpenRule(open ? null : rule.id)}>
                              {open ? "▾" : "▸"} {rule.name}
                              {rule.pack && <span className="pack-tag">{rule.pack}</span>}
                            </button>
                          </td>
                          <td className="num">{s.flags}</td>
                          <td className="num">{s.actioned}</td>
                          <td className="num">{s.dismissed}</td>
                          <td className={`num ${p !== null && p < 0.3 && s.actioned + s.dismissed >= 8 ? "bad" : ""}`}>{pct(p)}</td>
                          <td>{s.lastFired ? timeAgo(s.lastFired) : <span className="muted-text">never</span>}</td>
                          <td className="row-actions">
                            <button className="ghost small" onClick={() => setMode(rule, "test")}>
                              Test mode
                            </button>
                            <button className="ghost small" onClick={() => onEditRule(rule)}>
                              Edit
                            </button>
                          </td>
                        </tr>
                        {open && (
                          <tr className="detail">
                            <td colSpan={7}>
                              {s.patterns.length > 0 ? (
                                <table className="tune-table inner">
                                  <thead>
                                    <tr>
                                      <th>Word / phrase</th>
                                      <th className="num">Flags</th>
                                      <th className="num">Acted on</th>
                                      <th className="num">Dismissed</th>
                                      <th className="num">Accuracy</th>
                                      <th />
                                    </tr>
                                  </thead>
                                  <tbody>{s.patterns.map((w) => wordRow(rule, w))}</tbody>
                                </table>
                              ) : (
                                <p className="muted-text">No per-word data for this rule yet.</p>
                              )}
                              {exceptionInput(rule)}
                            </td>
                          </tr>
                        )}
                      </Fragment>
                    );
                  })}
                </tbody>
              </table>
              {quiet.length > 0 && (
                <p className="muted-text">
                  Haven't fired in {days} days: {quiet.map((r) => r.name).join(", ")}. That's fine for rare problems; turn
                  them off if they don't apply to your channels.
                </p>
              )}
            </section>
          </>
        )}
      </div>
    </section>
  );
}
