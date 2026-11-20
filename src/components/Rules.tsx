import { useEffect, useState } from "react";
import type { Channel, Rule, Severity } from "../types";
import { api } from "../api";
import { highlight } from "../util";

const blankRule = (): Rule => ({
  id: 0,
  name: "",
  enabled: true,
  kind: "keyword",
  patterns: [],
  wholeWord: true,
  caseSensitive: false,
  channels: [],
  severity: "medium",
  exemptPrivileged: true,
});

interface Props {
  rules: Rule[];
  channels: Channel[];
  onRulesChanged: (rules: Rule[]) => void;
}

export function Rules({ rules, channels, onRulesChanged }: Props) {
  const [editing, setEditing] = useState<Rule | null>(null);

  async function toggle(rule: Rule) {
    onRulesChanged(await api.saveRule({ ...rule, enabled: !rule.enabled }));
  }

  return (
    <section className="panel">
      <header className="panel-header">
        <h2>Rules</h2>
        <button className="primary" onClick={() => setEditing(blankRule())}>
          + New rule
        </button>
      </header>

      <div className="rules-layout">
        <div className="rule-list">
          {rules.length === 0 && <div className="empty"><p>No rules yet. Create one to start flagging messages.</p></div>}
          {rules.map((r) => (
            <div key={r.id} className={`rule-row ${editing?.id === r.id ? "active" : ""} ${r.enabled ? "" : "disabled"}`}>
              <label className="switch" title={r.enabled ? "Enabled" : "Disabled"}>
                <input type="checkbox" checked={r.enabled} onChange={() => toggle(r)} />
                <span />
              </label>
              <button className="rule-main" onClick={() => setEditing(r)}>
                <div className="rule-title">
                  {r.name} <span className={`sev-badge ${r.severity}`}>{r.severity}</span>
                </div>
                <div className="rule-meta">
                  {r.kind === "regex" ? "Regex" : "Keywords"} · {r.patterns.length} pattern{r.patterns.length === 1 ? "" : "s"} ·{" "}
                  {r.channels.length ? r.channels.map((c) => `#${c}`).join(", ") : "all channels"}
                </div>
              </button>
            </div>
          ))}
        </div>

        {editing ? (
          <RuleEditor
            key={editing.id || "new"}
            initial={editing}
            channels={channels}
            onCancel={() => setEditing(null)}
            onSaved={(rs) => {
              onRulesChanged(rs);
              setEditing(null);
            }}
          />
        ) : (
          <div className="rule-editor placeholder">
            <p className="muted-text">Select a rule to edit it, or create a new one.</p>
          </div>
        )}
      </div>
    </section>
  );
}

function RuleEditor({
  initial,
  channels,
  onCancel,
  onSaved,
}: {
  initial: Rule;
  channels: Channel[];
  onCancel: () => void;
  onSaved: (rules: Rule[]) => void;
}) {
  const [rule, setRule] = useState<Rule>(initial);
  const [patternText, setPatternText] = useState(initial.patterns.join("\n"));
  const [error, setError] = useState<string | null>(null);
  const [sample, setSample] = useState("");
  const [sampleResult, setSampleResult] = useState<{ terms: string[]; error?: string } | null>(null);

  const draft: Rule = { ...rule, patterns: patternText.split("\n").map((p) => p.trim()).filter(Boolean) };
  const set = <K extends keyof Rule>(k: K, v: Rule[K]) => setRule((r) => ({ ...r, [k]: v }));

  // Live tester: re-run whenever the sample or the rule changes.
  const draftKey = JSON.stringify(draft);
  useEffect(() => {
    if (!sample.trim()) {
      setSampleResult(null);
      return;
    }
    const t = setTimeout(() => {
      api
        .testRule({ ...draft, name: draft.name || "test" }, sample)
        .then((terms) => setSampleResult({ terms }))
        .catch((e) => setSampleResult({ terms: [], error: String(e) }));
    }, 200);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sample, draftKey]);

  async function save() {
    setError(null);
    try {
      onSaved(await api.saveRule(draft));
    } catch (e) {
      setError(String(e));
    }
  }

  async function remove() {
    onSaved(await api.deleteRule(rule.id));
  }

  const channelNames = Array.from(new Set([...channels.map((c) => c.name), ...rule.channels]));

  return (
    <div className="rule-editor">
      <h3>{initial.id ? "Edit rule" : "New rule"}</h3>

      <label className="field">
        <span>Name</span>
        <input value={rule.name} onChange={(e) => set("name", e.target.value)} placeholder="e.g. Spoilers" autoFocus />
      </label>

      <div className="field-row">
        <label className="field">
          <span>Match type</span>
          <select value={rule.kind} onChange={(e) => set("kind", e.target.value as Rule["kind"])}>
            <option value="keyword">Words &amp; phrases</option>
            <option value="regex">Regular expression</option>
          </select>
        </label>
        <label className="field">
          <span>Severity</span>
          <select value={rule.severity} onChange={(e) => set("severity", e.target.value as Severity)}>
            <option value="low">Low</option>
            <option value="medium">Medium</option>
            <option value="high">High</option>
          </select>
        </label>
      </div>

      <label className="field">
        <span>{rule.kind === "regex" ? "Patterns (one per line)" : "Words or phrases (one per line)"}</span>
        <textarea
          rows={6}
          value={patternText}
          onChange={(e) => setPatternText(e.target.value)}
          placeholder={rule.kind === "regex" ? "bit\\.ly/\\w+\n(?:discord\\.gg|discord\\.com/invite)/\\w+" : "free followers\nspoiler"}
          spellCheck={false}
        />
      </label>

      <div className="checks">
        <label>
          <input type="checkbox" checked={rule.wholeWord} onChange={(e) => set("wholeWord", e.target.checked)} />
          Whole words only <span className="hint">(“ass” won't match “class”)</span>
        </label>
        <label>
          <input type="checkbox" checked={rule.caseSensitive} onChange={(e) => set("caseSensitive", e.target.checked)} />
          Case sensitive
        </label>
        <label>
          <input
            type="checkbox"
            checked={rule.exemptPrivileged}
            onChange={(e) => set("exemptPrivileged", e.target.checked)}
          />
          Ignore mods, VIPs and the broadcaster
        </label>
      </div>

      <div className="field">
        <span>Channels</span>
        <div className="chip-select">
          <button className={rule.channels.length === 0 ? "on" : ""} onClick={() => set("channels", [])}>
            All channels
          </button>
          {channelNames.map((c) => {
            const on = rule.channels.includes(c);
            return (
              <button
                key={c}
                className={on ? "on" : ""}
                onClick={() => set("channels", on ? rule.channels.filter((x) => x !== c) : [...rule.channels, c])}
              >
                #{c}
              </button>
            );
          })}
        </div>
      </div>

      <div className="tester">
        <label className="field">
          <span>Try it: type a sample message</span>
          <input value={sample} onChange={(e) => setSample(e.target.value)} placeholder="Paste a chat message to test this rule" />
        </label>
        {sampleResult &&
          (sampleResult.error ? (
            <div className="form-error">{sampleResult.error}</div>
          ) : sampleResult.terms.length ? (
            <div className="test-hit">
              ⚑ Would flag: <span className="msg-text">{highlight(sample, sampleResult.terms)}</span>
            </div>
          ) : (
            <div className="test-miss">No match</div>
          ))}
      </div>

      {error && <div className="form-error">{error}</div>}

      <div className="editor-actions">
        {initial.id !== 0 && (
          <button className="danger" onClick={remove}>
            Delete
          </button>
        )}
        <span className="spacer" />
        <button onClick={onCancel}>Cancel</button>
        <button className="primary" onClick={save}>
          Save rule
        </button>
      </div>
    </div>
  );
}
