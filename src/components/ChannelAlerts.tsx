import { useEffect, useMemo, useState } from "react";
import type { AlertLevel, Channel, Rule, RuleAlertSetting, Settings } from "../types";
import { api } from "../api";
import { channelColor } from "../util";
import { effectiveAlert } from "../alerts";

interface Props {
  channels: Channel[];
  rules: Rule[];
  settings: Settings;
  /** Channel to show first (e.g. opened from a channel's ⚙ menu). */
  focus: string | null;
  onRules: (rules: Rule[]) => void;
  onChannelAlerts: (name: string, level: AlertLevel, sound: boolean) => void;
  toast: (text: string, kind?: "info" | "ok" | "error") => void;
}

const OPTIONS: { value: RuleAlertSetting; label: string; title: string }[] = [
  { value: "default", label: "Channel level", title: "Follow this channel's alert level" },
  { value: "always", label: "Always", title: "Always alert for this rule in this channel" },
  { value: "never", label: "Never", title: "Never alert for this rule in this channel (it still goes to the queue)" },
];

const LEVELS: { value: AlertLevel; label: string }[] = [
  { value: "default", label: "My default" },
  { value: "all", label: "Everything" },
  { value: "medium", label: "Medium and high" },
  { value: "high", label: "High only" },
  { value: "off", label: "Off (queue only)" },
];

/** Per-streamer alert settings: the channel's level plus a setting for every rule. */
export function ChannelAlerts({ channels, rules, settings, focus, onRules, onChannelAlerts, toast }: Props) {
  const modChannels = channels.filter((c) => c.isMod);
  const [selected, setSelected] = useState<string | null>(focus ?? modChannels[0]?.name ?? null);

  useEffect(() => {
    if (focus) setSelected(focus);
  }, [focus]);

  const channel = modChannels.find((c) => c.name === selected) ?? modChannels[0];

  // Your own rules first, then each default pack.
  const groups = useMemo(() => {
    const m = new Map<string, Rule[]>();
    for (const r of rules) {
      const key = r.pack ?? "";
      m.set(key, [...(m.get(key) ?? []), r]);
    }
    return [...m.entries()].sort(([a], [b]) => (a === "" ? -1 : b === "" ? 1 : a.localeCompare(b)));
  }, [rules]);

  if (!channel) {
    return <p className="muted-text">Add a channel you moderate to set its alerts.</p>;
  }

  async function setRule(rule: Rule, alert: RuleAlertSetting) {
    try {
      onRules(await api.setRuleChannelAlert(rule.id, channel.name, alert));
    } catch (e) {
      toast(String(e), "error");
    }
  }

  async function setGroup(pack: string, alert: RuleAlertSetting) {
    try {
      onRules(await api.setPackChannelAlert(pack === "" ? null : pack, channel.name, alert));
      const what = pack === "" ? "your rules" : `the ${pack} pack`;
      toast(`#${channel.name}: ${what} set to ${alert === "default" ? "channel level" : alert}`, "ok");
    } catch (e) {
      toast(String(e), "error");
    }
  }

  return (
    <div className="channel-alerts">
      <div className="ca-channels">
        {modChannels.map((c) => (
          <button
            key={c.name}
            className={`chip ${c.name === channel.name ? "on" : ""}`}
            style={{ borderColor: c.name === channel.name ? channelColor(c.name) : undefined }}
            onClick={() => setSelected(c.name)}
          >
            <span className="swatch" style={{ background: channelColor(c.name) }} /> #{c.name}
          </button>
        ))}
      </div>

      <div className="ca-level">
        <label className="field inline">
          <span>Alert level in #{channel.name}</span>
          <select value={channel.alertLevel} onChange={(e) => onChannelAlerts(channel.name, e.target.value as AlertLevel, channel.sound)}>
            {LEVELS.map((l) => (
              <option key={l.value} value={l.value}>
                {l.label}
              </option>
            ))}
          </select>
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={channel.sound}
            disabled={channel.alertLevel === "off"}
            onChange={(e) => onChannelAlerts(channel.name, channel.alertLevel, e.target.checked)}
          />
          Sounds
        </label>
      </div>
      <p className="muted-text">
        Each rule below can follow that level, always alert, or never alert, separately for every streamer. The 🔔 column
        shows what will actually happen in #{channel.name}. Flags always land in the queue either way.
      </p>

      {groups.map(([pack, list]) => (
        <div key={pack || "mine"} className="ca-group">
          <div className="ca-group-head">
            <strong>{pack || "Your rules"}</strong>
            <span className="spacer" />
            <span className="hint">Set all in #{channel.name}:</span>
            {OPTIONS.map((o) => (
              <button key={o.value} className="ghost small" onClick={() => setGroup(pack, o.value)} title={o.title}>
                {o.label}
              </button>
            ))}
          </div>
          {list.map((rule) => {
            const current = rule.channelAlerts?.[channel.name] ?? "default";
            const eff = effectiveAlert(rule, channel, settings);
            return (
              <div key={rule.id} className={`ca-row ${rule.enabled && rule.mode !== "test" ? "" : "inactive"}`}>
                <span className="ca-name">
                  {rule.name} <span className={`sev-badge ${rule.severity}`}>{rule.severity}</span>
                  {rule.mode === "test" && <span className="test-tag">test</span>}
                  {rule.alert !== "default" && (
                    <span className="hint" title="Set in the rule editor; applies to channels without their own setting">
                      {" "}
                      (everywhere: {rule.alert})
                    </span>
                  )}
                </span>
                <div className="seg small-seg">
                  {OPTIONS.map((o) => (
                    <button key={o.value} className={current === o.value ? "on" : ""} title={o.title} onClick={() => setRule(rule, o.value)}>
                      {o.label}
                    </button>
                  ))}
                </div>
                <span className={`ca-eff ${eff.alerts ? "yes" : "no"}`} title={eff.why}>
                  {eff.alerts ? "🔔 alerts" : "— quiet"}
                </span>
              </div>
            );
          })}
        </div>
      ))}
    </div>
  );
}
