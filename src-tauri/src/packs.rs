//! Default rule packs. They're added in test mode, so they only record what they
//! would have flagged until a mod switches them on from the Tuning view.

use crate::rules::{ActiveDuring, Rule, RuleKind, RuleMode, RuleParams, Severity, Target};

/// A rule with sensible defaults; callers fill in what differs.
pub fn base(name: &str, kind: RuleKind, severity: Severity) -> Rule {
    Rule {
        id: 0,
        name: name.into(),
        enabled: true,
        kind,
        patterns: vec![],
        whole_word: true,
        case_sensitive: false,
        channels: vec![],
        severity,
        exempt_privileged: true,
        target: Target::Message,
        normalize: true,
        active_during: ActiveDuring::Always,
        params: RuleParams::default(),
        also_patterns: vec![],
        addressed_to_streamer: false,
        not_to_other_chatters: false,
        exceptions: vec![],
        skip_subscribers: false,
        skip_accounts_older_than_days: 0,
        mode: RuleMode::Active,
        wellbeing: false,
        pack: None,
        alert: Default::default(),
    }
}

pub(crate) const BACKSEATING_V1: &[&str] =
    &["go left", "go right", "you missed", "wrong way", "you should have", "why didn't you", "why didnt you", "just use the"];
const BACKSEATING_V2: &[&str] = &[
    "go left",
    "go right",
    "go back",
    "go up",
    "go down",
    "wrong way",
    "other way",
    "you missed",
    "you forgot",
    "behind you",
    "you should have",
    "why didn't you",
    "why didnt you",
    "just use the",
];
pub(crate) const BODY_COMMENTS_V1: &[&str] = &["nice body", "thicc", "bend over", "cleavage", "thighs", "boobs", "booty", "nice rack"];

/// Fix pack rules saved by older versions, unless the mod has edited their word lists.
/// Returns the updated rules (to be saved).
pub fn upgrade_pack_rules(existing: &[Rule]) -> Vec<Rule> {
    let mut out = Vec::new();
    for r in existing {
        let is = |pack: &str, name: &str| r.pack.as_deref() == Some(pack) && r.name == name;
        let untouched = |v1: &[&str]| r.patterns == words(v1) && r.addressed_to_streamer;
        if is("Chat quality", "Backseating") && untouched(BACKSEATING_V1) {
            let mut u = r.clone();
            u.addressed_to_streamer = false;
            u.not_to_other_chatters = true;
            u.patterns = words(BACKSEATING_V2);
            out.push(u);
        } else if is("Sexual content", "Body comments") && untouched(BODY_COMMENTS_V1) {
            let mut u = r.clone();
            u.addressed_to_streamer = false;
            u.not_to_other_chatters = true;
            out.push(u);
        }
    }
    out
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn packed(pack: &str, mut r: Rule) -> Rule {
    r.pack = Some(pack.into());
    r.mode = RuleMode::Test;
    r
}

pub fn default_packs() -> Vec<Rule> {
    use RuleKind::*;
    use Severity::*;
    let mut out = Vec::new();

    // ---- Parasocial ----
    let p = "Parasocial";
    let mut r = base("Stalking / location", Keyword, High);
    r.patterns = words(&[
        "i know where you live",
        "i know where u live",
        "your address",
        "ur address",
        "i'm outside your",
        "im outside your",
        "outside your house",
        "outside your apartment",
        "saw your car",
        "saw you at",
        "drove past your",
        "drove by your",
        "found your house",
        "found where you live",
        "your home address",
        "i'm coming to your",
        "im coming to your",
    ]);
    out.push(packed(p, r));

    let mut r = base("Romantic / possessive", Keyword, Medium);
    r.addressed_to_streamer = true;
    r.patterns = words(&[
        "marry me",
        "be my girlfriend",
        "be my boyfriend",
        "be my wife",
        "be my husband",
        "be my gf",
        "be my bf",
        "you're mine",
        "youre mine",
        "ur mine",
        "my wife",
        "my waifu",
        "my queen",
        "love of my life",
        "the only one for me",
        "i'd do anything for you",
        "id do anything for you",
        "date me",
        "go on a date with me",
        "kiss me",
        "i'm in love with you",
        "im in love with you",
    ]);
    out.push(packed(p, r));

    let mut r = base("Personal probing", Keyword, Medium);
    r.addressed_to_streamer = true;
    r.patterns = words(&[
        "are you single",
        "r u single",
        "do you have a boyfriend",
        "do you have a girlfriend",
        "got a boyfriend",
        "got a girlfriend",
        "where do you live",
        "where do u live",
        "what city do you live",
        "what city are you in",
        "what state do you live",
        "how old are you really",
        "what's your real name",
        "whats your real name",
        "what school do you",
        "where do you work",
        "your phone number",
        "your snapchat",
        "add me on snap",
    ]);
    out.push(packed(p, r));

    let mut r = base("Jealousy / entitlement", Keyword, Medium);
    r.patterns = words(&[
        "who is he",
        "who was that guy",
        "who's that guy",
        "whos that guy",
        "you ignored me",
        "why are you ignoring me",
        "stop ignoring me",
        "you never reply",
        "why didn't you answer",
        "why didnt you answer",
        "answer my dm",
        "answer my dms",
        "read my dm",
        "read my dms",
        "after everything i've done",
        "after everything ive done",
        "after all i've donated",
        "after all ive donated",
        "you owe me",
        "notice me",
    ]);
    out.push(packed(p, r));

    let mut r = base("Fixation on the streamer", Fixation, Low);
    r.params.fixation_count = 8;
    r.params.fixation_window_mins = 10;
    out.push(packed(p, r));

    // ---- Sexual content ----
    let p = "Sexual content";
    let mut r = base("Explicit requests", Keyword, High);
    r.patterns = words(&[
        "send nudes",
        "nudes",
        "feet pics",
        "feet pic",
        "onlyfans",
        "only fans",
        "sit on my face",
        "take it off",
        "take off your",
        "strip for",
        "pay you for pics",
        "buy your pics",
        "sell me pics",
        "sugar daddy",
    ]);
    out.push(packed(p, r));

    let mut r = base("Body comments", Keyword, Medium);
    // Short comments like "thicc" rarely say "you", so only skip ones aimed at other chatters.
    r.not_to_other_chatters = true;
    r.patterns = words(&["nice body", "thicc", "bend over", "cleavage", "thighs", "boobs", "booty", "nice rack"]);
    out.push(packed(p, r));

    let mut r = base("Camera / pose requests", Keyword, Medium);
    r.patterns = words(&["show us your", "show me your", "turn around", "zoom in on", "stand up", "lean forward", "what are you wearing"]);
    r.also_patterns = words(&["feet", "body", "butt", "ass", "chest", "boobs", "legs", "thighs", "cleavage", "booty", "outfit"]);
    out.push(packed(p, r));

    let mut r = base("Innuendo", Keyword, Low);
    r.enabled = false;
    r.addressed_to_streamer = true;
    r.patterns = words(&["daddy", "mommy", "step on me", "spit on me", "choke me", "step bro", "stepbro", "rawr"]);
    out.push(packed(p, r));

    // ---- Harassment ----
    let mut r = base("Telling someone to die", Keyword, High);
    r.patterns = words(&["kys", "kill yourself", "go die", "unalive yourself", "neck yourself", "hope you die", "drink bleach"]);
    out.push(packed("Harassment", r));

    // ---- Wellbeing ----
    let mut r = base("Self-harm statements", Keyword, Medium);
    r.wellbeing = true;
    r.exempt_privileged = false;
    r.patterns = words(&[
        "i want to die",
        "i wanna die",
        "kill myself",
        "killing myself",
        "kms",
        "end it all",
        "end my life",
        "i'm suicidal",
        "im suicidal",
        "self harm",
        "cut myself",
        "no reason to live",
    ]);
    out.push(packed("Wellbeing", r));

    // ---- Chat quality ----
    let mut r = base("Backseating", Keyword, Low);
    r.enabled = false;
    // Instructions like "go left" never say "you", so only skip ones aimed at other chatters.
    r.not_to_other_chatters = true;
    r.patterns = words(BACKSEATING_V2);
    out.push(packed("Chat quality", r));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{validate, RuleEngine};

    #[test]
    fn every_pack_rule_is_valid_and_starts_in_test_mode() {
        for r in default_packs() {
            validate(&r).unwrap_or_else(|e| panic!("{}: {e}", r.name));
            assert_eq!(r.mode, RuleMode::Test, "{}", r.name);
            assert!(r.pack.is_some());
        }
    }

    #[test]
    fn packs_catch_typical_messages_and_skip_ordinary_ones() {
        let rules: Vec<Rule> = default_packs()
            .into_iter()
            .enumerate()
            .map(|(i, mut r)| {
                r.id = i as i64 + 1;
                r.enabled = true;
                r
            })
            .collect();
        let engine = RuleEngine::new(&rules);
        let names = |text: &str| -> Vec<String> {
            let m = crate::irc::ChatMessage {
                id: "1".into(),
                channel: "pixelqueen".into(),
                room_id: None,
                user_id: None,
                user_login: "viewer".into(),
                display_name: "viewer".into(),
                color: None,
                text: text.into(),
                is_action: false,
                is_mod: false,
                is_broadcaster: false,
                is_vip: false,
                is_subscriber: false,
                is_first_msg: false,
                emote_count: 0,
                reply_to: None,
                timestamp: 1,
            };
            engine.evaluate(&m, false).into_iter().map(|x| x.rule_name).collect()
        };
        assert_eq!(names("pixelqueen marry me pls"), vec!["Romantic / possessive"]);
        assert_eq!(names("I know where u live"), vec!["Stalking / location"]);
        assert_eq!(names("are you single?"), vec!["Personal probing"]);
        assert_eq!(names("send nudes"), vec!["Explicit requests"]);
        assert_eq!(names("show us your feet"), vec!["Camera / pose requests"]);
        assert_eq!(names("kys"), vec!["Telling someone to die"]);
        assert_eq!(names("honestly i wanna die"), vec!["Self-harm statements"]);
        assert_eq!(names("pixelqueen you're so thicc"), vec!["Body comments"]);
        assert_eq!(names("thicc"), vec!["Body comments"]);
        // Ordinary chat that shares words with the packs.
        for ok in ["show me your setup", "gg that was clean", "@bob marry me lol", "my wife loves this game", "stand up for yourself"] {
            assert!(names(ok).is_empty(), "{ok:?} matched {:?}", names(ok));
        }
    }

    /// The exact messages from a live test in a real chat. Before the fix only "you missed" matched.
    #[test]
    fn backseating_catches_live_chat_examples() {
        let rules: Vec<Rule> = default_packs()
            .into_iter()
            .filter(|r| r.name == "Backseating")
            .map(|mut r| {
                r.id = 1;
                r.enabled = true;
                r
            })
            .collect();
        let engine = RuleEngine::new(&rules);
        let check = |login: &str, text: &str, is_mod: bool, is_sub: bool| {
            let m = crate::irc::ChatMessage {
                id: "1".into(),
                channel: "shadesofnate".into(),
                room_id: None,
                user_id: None,
                user_login: login.into(),
                display_name: login.into(),
                color: None,
                text: text.into(),
                is_action: false,
                is_mod,
                is_broadcaster: false,
                is_vip: false,
                is_subscriber: is_sub,
                is_first_msg: false,
                emote_count: 0,
                reply_to: None,
                timestamp: 1,
            };
            !engine.evaluate(&m, false).is_empty()
        };
        let live = [
            ("smokeyfireman", "wrong way", false),
            ("smokeyfireman", "go left", false),
            ("paloma_roxanne", "Go left", true),
            ("florian_silvanus", "go left", false),
            ("florian_silvanus", "go right", false),
            ("smokeyfireman", "you missed", false),
            ("paloma_roxanne", "You missed", true),
            ("florian_silvanus", "you missed", false),
            ("florian_silvanus", "wrong way", false),
        ];
        for (user, text, sub) in live {
            assert!(check(user, text, false, sub), "{user}: {text:?} wasn't caught");
        }
        assert!(!check("spectresofzizek", "Thank you!", true, true));
        assert!(!check("someone", "@smokeyfireman go left", false, false));
    }

    #[test]
    fn upgrades_untouched_v1_pack_rules_only() {
        let mut old = base("Backseating", RuleKind::Keyword, Severity::Low);
        old.pack = Some("Chat quality".into());
        old.addressed_to_streamer = true;
        old.patterns = words(BACKSEATING_V1);
        let mut edited = old.clone();
        edited.name = "Body comments".into();
        edited.pack = Some("Sexual content".into());
        edited.patterns = words(&["thicc", "my own word"]);
        let up = upgrade_pack_rules(&[old, edited]);
        assert_eq!(up.len(), 1);
        assert!(up[0].not_to_other_chatters && !up[0].addressed_to_streamer);
        assert!(up[0].patterns.contains(&"go back".to_string()));
    }
}
