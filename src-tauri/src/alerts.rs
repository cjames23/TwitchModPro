//! When a flag should alert you (desktop notification and sound), combining the
//! channel's alert level with any per-rule override.

use crate::rules::Severity;
use serde::{Deserialize, Serialize};

/// Per-channel alert level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChannelAlertLevel {
    /// Use the global setting from the Alerts tab.
    #[default]
    Default,
    All,
    Medium,
    High,
    Off,
}

impl ChannelAlertLevel {
    pub fn parse(s: &str) -> Self {
        match s {
            "all" => Self::All,
            "medium" => Self::Medium,
            "high" => Self::High,
            "off" => Self::Off,
            _ => Self::Default,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::All => "all",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Off => "off",
        }
    }
}

/// Per-rule alert override.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RuleAlert {
    /// Follow the channel's alert level.
    #[default]
    Default,
    /// Alert whenever this rule fires (unless the channel's alerts are off).
    Always,
    /// Never alert; the flag still goes to the queue.
    Never,
}

/// Global flag-alert settings from the Alerts tab.
#[derive(Debug, Clone, Copy)]
pub struct GlobalAlerts {
    pub enabled: bool,
    pub min_severity: Severity,
}

/// Decide whether a flag alerts. `rules` are the overrides of every rule that matched.
pub fn flag_alerts(level: ChannelAlertLevel, global: GlobalAlerts, severity: Severity, rules: &[RuleAlert]) -> bool {
    if level == ChannelAlertLevel::Off {
        return false;
    }
    if !rules.is_empty() && rules.iter().all(|r| *r == RuleAlert::Never) {
        return false;
    }
    if rules.contains(&RuleAlert::Always) {
        return true;
    }
    match level {
        ChannelAlertLevel::Default => global.enabled && severity >= global.min_severity,
        ChannelAlertLevel::All => true,
        ChannelAlertLevel::Medium => severity >= Severity::Medium,
        ChannelAlertLevel::High => severity >= Severity::High,
        ChannelAlertLevel::Off => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ChannelAlertLevel as L;
    use RuleAlert as R;
    use Severity::*;

    const GLOBAL: GlobalAlerts = GlobalAlerts { enabled: true, min_severity: Medium };

    #[test]
    fn channel_levels() {
        assert!(!flag_alerts(L::Default, GLOBAL, Low, &[]));
        assert!(flag_alerts(L::Default, GLOBAL, Medium, &[]));
        assert!(!flag_alerts(L::Default, GlobalAlerts { enabled: false, ..GLOBAL }, High, &[]));
        assert!(flag_alerts(L::All, GLOBAL, Low, &[]));
        assert!(!flag_alerts(L::High, GLOBAL, Medium, &[]));
        assert!(flag_alerts(L::High, GLOBAL, High, &[]));
        assert!(!flag_alerts(L::Off, GLOBAL, High, &[R::Always]));
    }

    #[test]
    fn rule_overrides() {
        // A low-severity rule set to "always" alerts even where the channel wants high only...
        assert!(flag_alerts(L::High, GLOBAL, Low, &[R::Always]));
        // ...and a "never" rule stays quiet even in an "everything" channel.
        assert!(!flag_alerts(L::All, GLOBAL, High, &[R::Never]));
        // When one matched rule is "never" and another follows the channel, the channel decides.
        assert!(flag_alerts(L::All, GLOBAL, Low, &[R::Never, R::Default]));
    }
}
