// Mirrors src-tauri/src/alerts.rs so the UI can show whether a rule will alert in a channel.
import type { Channel, Rule, RuleAlertSetting, Settings } from "./types";
import { SEVERITY_RANK } from "./types";

export function ruleAlertIn(rule: Rule, channel: string): RuleAlertSetting {
  const c = rule.channelAlerts?.[channel];
  return c && c !== "default" ? c : rule.alert;
}

/** Whether a flag from this rule alone would alert in this channel, and why. */
export function effectiveAlert(rule: Rule, channel: Channel, settings: Settings): { alerts: boolean; why: string } {
  if (!rule.enabled) return { alerts: false, why: "rule is off" };
  if (rule.mode === "test") return { alerts: false, why: "test mode: no flags yet" };
  if (channel.alertLevel === "off") return { alerts: false, why: "channel alerts are off" };
  const o = ruleAlertIn(rule, channel.name);
  const fromChannel = rule.channelAlerts?.[channel.name] && rule.channelAlerts[channel.name] !== "default";
  if (o === "never") return { alerts: false, why: fromChannel ? "set to never here" : "rule is set to never" };
  if (o === "always") return { alerts: true, why: fromChannel ? "set to always here" : "rule is set to always" };
  const sev = SEVERITY_RANK[rule.severity];
  switch (channel.alertLevel) {
    case "all":
      return { alerts: true, why: "channel alerts on everything" };
    case "medium":
      return { alerts: sev >= 1, why: "channel: medium and high" };
    case "high":
      return { alerts: sev >= 2, why: "channel: high only" };
    default:
      if (!settings.notifyFlags) return { alerts: false, why: "flag alerts are off in your defaults" };
      return {
        alerts: sev >= SEVERITY_RANK[settings.notifyMinSeverity],
        why: `your default: ${settings.notifyMinSeverity} and up`,
      };
  }
}
