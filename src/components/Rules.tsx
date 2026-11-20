import { useEffect, useState } from "react";
import type { Channel, Rule, RuleKind, RuleMatch, RuleParams, Severity } from "../types";
import { api } from "../api";
import { highlight } from "../util";

const DEFAULT_PARAMS: RuleParams = {
  minLength: 12,
  capsPercent: 75,
  maxRepeatedChars: 12,
  maxEmotes: 12,
  repeatCount: 3,
  repeatWindowSecs: 60,
  maxAccountAgeDays: 7,
  fixationCount: 8,
  fixationWindowMins: 10,
};

const KINDS: { kind: RuleKind; label: string; help: string }[] = [
  { kind: "keyword", label: "Words & phrases", help: "Flag messages (or usernames) containing any of these words." },
  { kind: "regex", label: "Regular expression", help: "Advanced pattern matching." },
  { kind: "links", label: "Links", help: "Flag any link, except to the domains you allow." },
  { kind: "caps", label: "Excessive caps", help: "Flag messages that are mostly capital letters." },
  { kind: "spam", label: "Symbol & emote spam", help: "Long runs of one character, too many emotes, or ASCII art." },
  { kind: "repeat", label: "Repeated messages", help: "The same user sending the same message again and again." },
  { kind: "firstMessage", label: "First-time chatters", help: "A user's first message in the channel." },
  {
    kind: "fixation",
    label: "Fixation on the streamer",
    help: "One user talking to the streamer over and over in a short time. Flags once per window.",
  },
  {
    kind: "newAccount",
    label: "New accounts",
    help: "Accounts created recently. Checked with Twitch within about a second, and flagged once per user per channel.",
  },
];

const kindLabel = (k: RuleKind) => KINDS.find((x) => x.kind === k)?.label ?? k;

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
  target: "message",
  normalize: true,
  activeDuring: "always",
  params: { ...DEFAULT_PARAMS },
  alsoPatterns: [],
  addressedToStreamer: false,
  notToOtherChatters: false,
  exceptions: [],
  skipSubscribers: false,
  skipAccountsOlderThanDays: 0,
  mode: "test",
  wellbeing: false,
  pack: null,
});

interface Props {
  rules: Rule[];
  channels: Channel[];
  onRulesChanged: (rules: Rule[]) => void;
  /** Open this rule in the editor (e.g. from the Tuning view). */
  editRuleId?: number | null;
}

export function Rules({ rules, channels, onRulesChanged, editRuleId }: Props) {
  const [editing, setEditing] = useState<Rule | null>(null);

  useEffect(() => {
    if (editRuleId) setEditing(rules.find((r) => r.id === editRuleId) ?? null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editRuleId]);

  async function toggle(rule: Rule) {
    onRulesChanged(await api.saveRule({ ...rule, enabled: !rule.enabled }));
  }

  function summary(r: Rule) {
    const parts = [kindLabel(r.kind)];
    if (r.kind === "keyword" || r.kind === "regex") {
      parts.push(`${r.patterns.length} pattern${r.patterns.length === 1 ? "" : "s"}`);
      if (r.target === "username") parts.push("usernames");
    }
    parts.push(r.channels.length ? r.channels.map((c) => `#${c}`).join(", ") : "all channels");
    if (r.activeDuring === "raid") parts.push("raid mode only");
    if (r.addressedToStreamer) parts.push("clearly to the streamer");
    else if (r.notToOtherChatters) parts.push("not to other chatters");
    return parts.join(" · ");
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
          {rules.length === 0 && (
            <div className="empty">
              <p>No rules yet. Create one to start flagging messages.</p>
            </div>
          )}
          {rules.map((r) => (
            <div key={r.id} className={`rule-row ${editing?.id === r.id ? "active" : ""} ${r.enabled ? "" : "disabled"}`}>
              <label className="switch" title={r.enabled ? "Enabled" : "Disabled"}>
                <input type="checkbox" checked={r.enabled} onChange={() => toggle(r)} />
                <span />
              </label>
              <button className="rule-main" onClick={() => setEditing(r)}>
                <div className="rule-title">
                  {r.name} <span className={`sev-badge ${r.severity}`}>{r.severity}</span>
                  {r.activeDuring === "raid" && <span className="raid-tag">⚡ raid</span>}
                  {r.mode === "test" && <span className="test-tag">test</span>}
                  {r.wellbeing && <span className="wellbeing-tag">wellbeing</span>}
                </div>
                <div className="rule-meta">
                  {r.pack && <span className="pack-tag">{r.pack}</span>}
                  {summary(r)}
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

function NumberField(props: {
  label: string;
  value: number;
  min: number;
  max: number;
  suffix?: string;
  onChange: (n: number) => void;
}) {
  return (
    <label className="field">
      <span>{props.label}</span>
      <div className="num-field">
        <input
          type="number"
          min={props.min}
          max={props.max}
          value={props.value}
          onChange={(e) => props.onChange(Math.min(props.max, Math.max(props.min, Number(e.target.value) || props.min)))}
        />
        {props.suffix && <span className="hint">{props.suffix}</span>}
      </div>
    </label>
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
  const [rule, setRule] = useState<Rule>({ ...initial, params: { ...DEFAULT_PARAMS, ...initial.params } });
  const [patternText, setPatternText] = useState(initial.patterns.join("\n"));
  const [alsoText, setAlsoText] = useState(initial.alsoPatterns.join("\n"));
  const [exceptionText, setExceptionText] = useState(initial.exceptions.join("\n"));
  const [error, setError] = useState<string | null>(null);
  const [sample, setSample] = useState("");
  const [sampleUser, setSampleUser] = useState("");
  const [sampleFirst, setSampleFirst] = useState(false);
  const [sampleResult, setSampleResult] = useState<{ matches: RuleMatch[]; error?: string } | null>(null);

  const lines = (t: string) => t.split("\n").map((p) => p.trim()).filter(Boolean);
  const draft: Rule = { ...rule, patterns: lines(patternText), alsoPatterns: lines(alsoText), exceptions: lines(exceptionText) };
  const set = <K extends keyof Rule>(k: K, v: Rule[K]) => setRule((r) => ({ ...r, [k]: v }));
  const setParam = <K extends keyof RuleParams>(k: K, v: RuleParams[K]) =>
    setRule((r) => ({ ...r, params: { ...r.params, [k]: v } }));

  const usesPatterns = rule.kind === "keyword" || rule.kind === "regex";
  const byUsername = usesPatterns && rule.target === "username";

  // Live tester: re-run whenever the sample or the rule changes.
  const draftKey = JSON.stringify(draft);
  useEffect(() => {
    if (!sample.trim() && !(byUsername && sampleUser.trim()) && !sampleFirst) {
      setSampleResult(null);
      return;
    }
    const t = setTimeout(() => {
      api
        .testRule({ ...draft, name: draft.name || "test" }, sample, sampleUser, sampleFirst)
        .then((matches) => setSampleResult({ matches }))
        .catch((e) => setSampleResult({ matches: [], error: String(e) }));
    }, 200);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sample, sampleUser, sampleFirst, draftKey]);

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
  const kindInfo = KINDS.find((k) => k.kind === rule.kind)!;
  const hit = sampleResult?.matches[0];

  return (
    <div className="rule-editor">
      <h3>{initial.id ? "Edit rule" : "New rule"}</h3>

      <label className="field">
        <span>Name</span>
        <input value={rule.name} onChange={(e) => set("name", e.target.value)} placeholder="e.g. Spoilers" autoFocus />
      </label>

      <div className="field">
        <span>Mode</span>
        <div className="seg fit">
          <button className={rule.mode === "test" ? "on" : ""} onClick={() => set("mode", "test")}>
            Test (silent)
          </button>
          <button className={rule.mode === "active" ? "on" : ""} onClick={() => set("mode", "active")}>
            Active (creates flags)
          </button>
        </div>
        <span className="hint">
          {rule.mode === "test"
            ? "Records what it would flag, shown in Tuning. Nothing appears in your queue."
            : "Matches go to the flag queue and can alert you."}
        </span>
      </div>

      <div className="field-row">
        <label className="field">
          <span>Type</span>
          <select value={rule.kind} onChange={(e) => set("kind", e.target.value as RuleKind)}>
            {KINDS.map((k) => (
              <option key={k.kind} value={k.kind}>
                {k.label}
              </option>
            ))}
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
      <p className="hint kind-help">{kindInfo.help}</p>

      {usesPatterns && (
        <>
          <div className="field">
            <span>Check</span>
            <div className="seg fit">
              <button className={rule.target === "message" ? "on" : ""} onClick={() => set("target", "message")}>
                Message text
              </button>
              <button className={rule.target === "username" ? "on" : ""} onClick={() => set("target", "username")}>
                Username
              </button>
            </div>
          </div>
          <label className="field">
            <span>{rule.kind === "regex" ? "Patterns (one per line)" : "Words or phrases (one per line)"}</span>
            <textarea
              rows={5}
              value={patternText}
              onChange={(e) => setPatternText(e.target.value)}
              placeholder={
                rule.kind === "regex"
                  ? "bit\\.ly/\\w+\n(?:discord\\.gg|discord\\.com/invite)/\\w+"
                  : byUsername
                    ? "hoss00312\nviewerbot"
                    : "free followers\nspoiler"
              }
              spellCheck={false}
            />
          </label>
          {!byUsername && (
            <label className="field">
              <span>Must also contain one of (optional, one per line)</span>
              <textarea
                rows={3}
                value={alsoText}
                onChange={(e) => setAlsoText(e.target.value)}
                placeholder={"feet\nbody"}
                spellCheck={false}
              />
            </label>
          )}
        </>
      )}

      {rule.kind === "fixation" && (
        <div className="field-row">
          <NumberField label="Flag on message number" value={rule.params.fixationCount} min={2} max={100} onChange={(n) => setParam("fixationCount", n)} />
          <NumberField label="To the streamer within" value={rule.params.fixationWindowMins} min={1} max={240} suffix="minutes" onChange={(n) => setParam("fixationWindowMins", n)} />
        </div>
      )}

      {rule.kind === "links" && (
        <label className="field">
          <span>Allowed domains (one per line, subdomains included)</span>
          <textarea
            rows={4}
            value={patternText}
            onChange={(e) => setPatternText(e.target.value)}
            placeholder={"twitch.tv\nyoutube.com\nclips.twitch.tv"}
            spellCheck={false}
          />
        </label>
      )}

      {rule.kind === "caps" && (
        <div className="field-row">
          <NumberField label="Flag when capitals are at least" value={rule.params.capsPercent} min={1} max={100} suffix="%" onChange={(n) => setParam("capsPercent", n)} />
          <NumberField label="Ignore messages shorter than" value={rule.params.minLength} min={1} max={500} suffix="letters" onChange={(n) => setParam("minLength", n)} />
        </div>
      )}

      {rule.kind === "spam" && (
        <div className="field-row">
          <NumberField label="Same character repeated" value={rule.params.maxRepeatedChars} min={3} max={500} suffix="+ times" onChange={(n) => setParam("maxRepeatedChars", n)} />
          <NumberField label="More emotes than" value={rule.params.maxEmotes} min={1} max={500} onChange={(n) => setParam("maxEmotes", n)} />
        </div>
      )}

      {rule.kind === "newAccount" && (
        <div className="field-row">
          <NumberField
            label="Flag accounts younger than"
            value={rule.params.maxAccountAgeDays}
            min={1}
            max={3650}
            suffix="days"
            onChange={(n) => setParam("maxAccountAgeDays", n)}
          />
        </div>
      )}

      {rule.kind === "repeat" && (
        <div className="field-row">
          <NumberField label="Flag on message number" value={rule.params.repeatCount} min={2} max={50} onChange={(n) => setParam("repeatCount", n)} />
          <NumberField label="Within" value={rule.params.repeatWindowSecs} min={5} max={3600} suffix="seconds" onChange={(n) => setParam("repeatWindowSecs", n)} />
        </div>
      )}

      <div className="checks">
        {usesPatterns && (
          <>
            <label>
              <input type="checkbox" checked={rule.normalize} onChange={(e) => set("normalize", e.target.checked)} />
              Catch disguised spellings <span className="hint">(fr33, ｆｒｅｅ, lookalike letters, hidden characters)</span>
            </label>
            <label>
              <input type="checkbox" checked={rule.wholeWord} onChange={(e) => set("wholeWord", e.target.checked)} />
              Whole words only <span className="hint">(“ass” won't match “class”)</span>
            </label>
            {!rule.normalize && (
              <label>
                <input type="checkbox" checked={rule.caseSensitive} onChange={(e) => set("caseSensitive", e.target.checked)} />
                Case sensitive
              </label>
            )}
          </>
        )}
        <label>
          <input type="checkbox" checked={rule.exemptPrivileged} onChange={(e) => set("exemptPrivileged", e.target.checked)} />
          Ignore mods, VIPs and the broadcaster
        </label>
        {rule.kind !== "fixation" && !byUsername && (
          <div className="audience">
            <span className="hint">Who the message is aimed at:</span>
            <label>
              <input
                type="radio"
                name="audience"
                checked={!rule.addressedToStreamer && !rule.notToOtherChatters}
                onChange={() => setRule((r) => ({ ...r, addressedToStreamer: false, notToOtherChatters: false }))}
              />
              Anyone
            </label>
            <label>
              <input
                type="radio"
                name="audience"
                checked={rule.notToOtherChatters && !rule.addressedToStreamer}
                onChange={() => setRule((r) => ({ ...r, addressedToStreamer: false, notToOtherChatters: true }))}
              />
              Not another chatter <span className="hint">(skips replies and messages starting with @someone else)</span>
            </label>
            <label>
              <input
                type="radio"
                name="audience"
                checked={rule.addressedToStreamer}
                onChange={() => setRule((r) => ({ ...r, addressedToStreamer: true, notToOtherChatters: false }))}
              />
              Clearly the streamer <span className="hint">(must say their name, @ them, or say “you”; misses “go left”)</span>
            </label>
          </div>
        )}
        <label>
          <input type="checkbox" checked={rule.skipSubscribers} onChange={(e) => set("skipSubscribers", e.target.checked)} />
          Ignore subscribers
        </label>
        <label>
          <input type="checkbox" checked={rule.wellbeing} onChange={(e) => set("wellbeing", e.target.checked)} />
          Wellbeing check <span className="hint">(shown as a check-in, with no suggested punishment)</span>
        </label>
        <label>
          <input
            type="checkbox"
            checked={rule.activeDuring === "raid"}
            onChange={(e) => set("activeDuring", e.target.checked ? "raid" : "always")}
          />
          Only during raid mode <span className="hint">(for a few minutes after a raid)</span>
        </label>
      </div>

      <div className="field-row">
        <label className="field">
          <span>Exceptions: never flag messages containing (one per line)</span>
          <textarea
            rows={3}
            value={exceptionText}
            onChange={(e) => setExceptionText(e.target.value)}
            placeholder={"die hard\ndark souls"}
            spellCheck={false}
          />
        </label>
        <NumberField
          label="Ignore accounts older than (0 = off)"
          value={rule.skipAccountsOlderThanDays}
          min={0}
          max={3650}
          suffix="days"
          onChange={(n) => set("skipAccountsOlderThanDays", n)}
        />
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
        <span className="tester-label">Try it</span>
        {byUsername ? (
          <input value={sampleUser} onChange={(e) => setSampleUser(e.target.value)} placeholder="Type a username to test" />
        ) : (
          <input value={sample} onChange={(e) => setSample(e.target.value)} placeholder="Type or paste a chat message to test" />
        )}
        {rule.kind === "firstMessage" && (
          <label className="check">
            <input type="checkbox" checked={sampleFirst} onChange={(e) => setSampleFirst(e.target.checked)} />
            Pretend it's the user's first message
          </label>
        )}
        {rule.kind === "repeat" && <span className="hint">The tester sends your sample {rule.params.repeatCount} times.</span>}
        {(rule.kind === "newAccount" || rule.kind === "fixation") && (
          <span className="hint">This rule depends on account age or message history, so it can't be tried here.</span>
        )}
        {sampleResult &&
          (sampleResult.error ? (
            <div className="form-error">{sampleResult.error}</div>
          ) : hit ? (
            <div className="test-hit">
              ⚑ Would flag{hit.reason ? `: ${hit.reason}` : ""}
              {rule.addressedToStreamer && <span className="hint"> (the tester counts your sample as talking to the streamer only if it says “you” or names them)</span>}
              {hit.terms.length > 0 && (
                <div className="msg-text">{highlight(sample, hit.terms)}</div>
              )}
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
