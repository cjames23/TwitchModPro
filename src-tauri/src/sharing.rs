//! Rule-pack sharing: export rules to a JSON file (or text you can paste in Discord) and import them.
//!
//! Exports leave out anything tied to your own channels (channel limits and per-channel alert
//! settings). Imports are checked rule by rule and come in as a group, in test mode by default.

use crate::rules::{self, Rule, RuleMode};
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "twitch-mod-pro/rules";
pub const VERSION: u32 = 1;
/// Largest file or pasted text accepted.
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RULES: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleFile {
    pub format: String,
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub exported_at: i64,
    #[serde(default)]
    pub app_version: String,
    pub rules: Vec<Rule>,
}

/// A rule with everything specific to the exporter's channels removed.
fn portable(rule: &Rule) -> Rule {
    let mut r = rule.clone();
    r.id = 0;
    r.channels.clear();
    r.channel_alerts.clear();
    r
}

pub fn export(rules: &[Rule], name: &str, description: &str, now: i64) -> RuleFile {
    RuleFile {
        format: FORMAT.into(),
        version: VERSION,
        name: name.trim().to_string(),
        description: description.trim().to_string(),
        exported_at: now,
        app_version: env!("CARGO_PKG_VERSION").into(),
        rules: rules.iter().map(portable).collect(),
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImportStatus {
    /// Nothing like it yet.
    New,
    /// You already have exactly this rule.
    Identical,
    /// You have a rule with this name but different settings.
    Conflict,
    /// Couldn't be loaded (see `error`).
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    pub index: usize,
    pub rule: Rule,
    pub status: ImportStatus,
    /// For conflicts: the id of your rule with the same name.
    pub existing_id: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub name: String,
    pub description: String,
    pub exported_at: i64,
    pub items: Vec<ImportItem>,
}

/// Compare two rules ignoring the id, the group tag, on/off, mode and anything channel-specific.
fn same_rule(a: &Rule, b: &Rule) -> bool {
    let norm = |r: &Rule| {
        let mut r = portable(r);
        r.pack = None;
        r.enabled = true;
        r.mode = RuleMode::Active;
        r
    };
    norm(a) == norm(b)
}

/// Read a rule file (or pasted text) and check each rule against yours.
pub fn preview(text: &str, existing: &[Rule]) -> Result<ImportPreview, String> {
    if text.len() > MAX_BYTES {
        return Err("That file is too big to be a rule pack (over 2 MB).".into());
    }
    let value: serde_json::Value =
        serde_json::from_str(text.trim()).map_err(|_| "That isn't a Twitch Mod Pro rule pack (it isn't valid JSON).".to_string())?;
    if value["format"].as_str() != Some(FORMAT) {
        return Err("That isn't a Twitch Mod Pro rule pack.".into());
    }
    let version = value["version"].as_u64().unwrap_or(0);
    if version == 0 || version > VERSION as u64 {
        return Err(format!("This rule pack was made by a newer version of Twitch Mod Pro (format {version}). Update the app to import it."));
    }
    let raw_rules = value["rules"].as_array().ok_or("The rule pack has no rules.")?;
    if raw_rules.len() > MAX_RULES {
        return Err(format!("The rule pack has {} rules; the most that can be imported at once is {MAX_RULES}.", raw_rules.len()));
    }
    let items = raw_rules
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let parsed = serde_json::from_value::<Rule>(raw.clone()).map_err(|e| format!("Unreadable rule: {e}"));
            let rule = match parsed {
                Ok(r) => portable(&r),
                Err(error) => {
                    let mut placeholder = crate::packs::base(raw["name"].as_str().unwrap_or("(unnamed rule)"), rules::RuleKind::Keyword, rules::Severity::Low);
                    placeholder.enabled = false;
                    return ImportItem { index, rule: placeholder, status: ImportStatus::Invalid, existing_id: None, error: Some(error) };
                }
            };
            if let Err(error) = rules::validate(&rule) {
                return ImportItem { index, rule, status: ImportStatus::Invalid, existing_id: None, error: Some(error) };
            }
            let named = existing.iter().find(|e| e.name.trim().eq_ignore_ascii_case(rule.name.trim()));
            let (status, existing_id) = match named {
                Some(e) if same_rule(e, &rule) => (ImportStatus::Identical, Some(e.id)),
                Some(e) => (ImportStatus::Conflict, Some(e.id)),
                None if existing.iter().any(|e| same_rule(e, &rule)) => (ImportStatus::Identical, None),
                None => (ImportStatus::New, None),
            };
            ImportItem { index, rule, status, existing_id, error: None }
        })
        .collect();
    Ok(ImportPreview {
        name: value["name"].as_str().unwrap_or_default().trim().to_string(),
        description: value["description"].as_str().unwrap_or_default().to_string(),
        exported_at: value["exportedAt"].as_i64().unwrap_or(0),
        items,
    })
}

/// What to do with one rule from the preview.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImportChoice {
    Skip,
    /// Add it (as a copy with a new name if one with this name exists).
    Add,
    /// Overwrite your rule with the same name, keeping your channel settings for it.
    Replace,
}

/// Turn the preview plus the user's choices into rules to save. `group` becomes each rule's pack tag.
pub fn apply(preview: &ImportPreview, choices: &[(usize, ImportChoice)], group: &str, test_mode: bool, existing: &[Rule]) -> Vec<Rule> {
    let group = group.trim();
    let mut taken: Vec<String> = existing.iter().map(|r| r.name.to_lowercase()).collect();
    let mut out = Vec::new();
    for (index, choice) in choices {
        let Some(item) = preview.items.iter().find(|i| i.index == *index) else { continue };
        if item.status == ImportStatus::Invalid || *choice == ImportChoice::Skip {
            continue;
        }
        let mut rule = item.rule.clone();
        rule.pack = (!group.is_empty()).then(|| group.to_string());
        if test_mode {
            rule.mode = RuleMode::Test;
        }
        match (choice, item.existing_id.and_then(|id| existing.iter().find(|e| e.id == id))) {
            (ImportChoice::Replace, Some(old)) if item.status == ImportStatus::Conflict => {
                rule.id = old.id;
                rule.channels = old.channels.clone();
                rule.channel_alerts = old.channel_alerts.clone();
            }
            _ => {
                if taken.contains(&rule.name.to_lowercase()) {
                    let base = rule.name.clone();
                    let mut n = 2;
                    while taken.contains(&format!("{base} ({n})").to_lowercase()) {
                        n += 1;
                    }
                    rule.name = format!("{base} ({n})");
                }
                rule.id = 0;
            }
        }
        taken.push(rule.name.to_lowercase());
        out.push(rule);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{RuleKind, Severity};

    fn kw(name: &str, words: &[&str]) -> Rule {
        let mut r = crate::packs::base(name, RuleKind::Keyword, Severity::Medium);
        r.patterns = words.iter().map(|w| w.to_string()).collect();
        r
    }

    #[test]
    fn export_strips_channel_settings_and_round_trips() {
        let mut r = kw("Slurs", &["badword"]);
        r.id = 7;
        r.channels = vec!["mychannel".into()];
        r.channel_alerts.insert("mychannel".into(), crate::alerts::RuleAlert::Always);
        let file = export(&[r], "My pack", "", 1);
        let text = serde_json::to_string(&file).unwrap();
        assert!(!text.contains("mychannel"));
        let p = preview(&text, &[]).unwrap();
        assert_eq!(p.name, "My pack");
        assert_eq!(p.items[0].status, ImportStatus::New);
        assert_eq!(p.items[0].rule.id, 0);
    }

    #[test]
    fn detects_identical_conflicting_and_invalid_rules() {
        let mut mine = kw("Slurs", &["badword"]);
        mine.id = 3;
        mine.channels = vec!["mine".into()];
        let mut bad_regex = crate::packs::base("Broken", RuleKind::Regex, Severity::Low);
        bad_regex.patterns = vec!["(unclosed".into()];
        let file = export(&[kw("Slurs", &["badword"]), kw("slurs", &["other"]), bad_regex, kw("Fresh", &["x"])], "P", "", 0);
        let mut v = serde_json::to_value(&file).unwrap();
        v["rules"].as_array_mut().unwrap().push(serde_json::json!({ "name": "Garbage", "kind": "nope" }));
        let p = preview(&v.to_string(), &[mine.clone()]).unwrap();
        let statuses: Vec<ImportStatus> = p.items.iter().map(|i| i.status).collect();
        use ImportStatus::*;
        assert_eq!(statuses, vec![Identical, Conflict, Invalid, New, Invalid]);
        assert_eq!(p.items[1].existing_id, Some(3));

        // Replace keeps your channel limits; Add makes a renamed copy; imports land in test mode.
        let saved = apply(&p, &[(1, ImportChoice::Replace), (3, ImportChoice::Add), (2, ImportChoice::Add)], "Shared", true, &[mine.clone()]);
        assert_eq!(saved.len(), 2);
        assert_eq!((saved[0].id, saved[0].channels.clone()), (3, vec!["mine".to_string()]));
        assert_eq!(saved[1].pack.as_deref(), Some("Shared"));
        assert_eq!(saved[1].mode, RuleMode::Test);
        let copy = apply(&p, &[(1, ImportChoice::Add)], "", false, &[mine]);
        assert_eq!(copy[0].name, "slurs (2)");
        assert_eq!(copy[0].id, 0);
    }

    #[test]
    fn rejects_other_files() {
        assert!(preview("hello", &[]).is_err());
        assert!(preview(r#"{"format":"something-else","rules":[]}"#, &[]).is_err());
        assert!(preview(r#"{"format":"twitch-mod-pro/rules","version":99,"rules":[]}"#, &[]).unwrap_err().contains("newer"));
    }
}
