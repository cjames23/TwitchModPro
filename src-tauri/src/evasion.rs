//! Ban-evasion matching: a new chatter whose name looks like someone banned recently
//! (`troll_guy` banned, then `troll_guy2` or `tr0ll_guy` shows up).

/// Names shorter than this (after cleanup) are too generic to compare.
const MIN_CORE: usize = 5;
/// Fuzzy matches (one letter off, or starting the same) need longer names.
const MIN_FUZZY: usize = 6;

/// A banned user, prepared for comparison.
#[derive(Debug, Clone)]
pub struct Banned {
    pub login: String,
    pub channel: String,
    pub banned_at: i64,
    core: String,
}

impl Banned {
    pub fn new(login: &str, channel: &str, banned_at: i64) -> Self {
        Banned { login: login.to_string(), channel: channel.to_string(), banned_at, core: core_name(login) }
    }
}

/// The part of a login that people keep when making a new account:
/// trailing numbers and underscores dropped, lookalike characters folded, underscores removed.
pub fn core_name(login: &str) -> String {
    let base = login.to_lowercase();
    let base = base.trim_end_matches(|c: char| c.is_ascii_digit() || c == '_');
    let folded: String = base
        .chars()
        .filter_map(|c| match c {
            '_' => None,
            '0' => Some('o'),
            '1' | 'i' => Some('l'),
            '3' => Some('e'),
            '4' => Some('a'),
            '5' => Some('s'),
            '7' => Some('t'),
            '8' => Some('b'),
            c => Some(c),
        })
        .collect();
    folded.replace("rn", "m").replace("vv", "w")
}

/// Whether two cores differ by exactly one inserted, removed or changed character.
fn one_edit_apart(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (short, long) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
    match long.len() - short.len() {
        0 => short.iter().zip(long.iter()).filter(|(x, y)| x != y).count() == 1,
        1 => {
            let mut i = 0;
            while i < short.len() && short[i] == long[i] {
                i += 1;
            }
            short[i..] == long[i + 1..]
        }
        _ => false,
    }
}

/// How a login resembles a banned one, if it does.
pub fn resemblance(login: &str, banned: &Banned) -> Option<&'static str> {
    if login.eq_ignore_ascii_case(&banned.login) {
        return None;
    }
    let core = core_name(login);
    let b = &banned.core;
    if core.len() < MIN_CORE || b.len() < MIN_CORE {
        return None;
    }
    if &core == b {
        return Some("the same name with different numbers or lookalike letters");
    }
    let shorter = core.len().min(b.len());
    if shorter >= MIN_FUZZY {
        if one_edit_apart(&core, b) {
            return Some("almost the same name (one letter different)");
        }
        // "trollguy" → "trollguyreturns": the new name starts with the whole banned name.
        if core.starts_with(b.as_str()) && core.len() - b.len() <= 8 {
            return Some("the banned name with something added");
        }
    }
    None
}

/// The closest banned match for a login. Matches in `channel` win over other channels, then the most recent ban.
pub fn best_match<'a>(login: &str, channel: &str, banned: &'a [Banned]) -> Option<(&'a Banned, &'static str)> {
    banned
        .iter()
        .filter_map(|b| resemblance(login, b).map(|why| (b, why)))
        .max_by_key(|(b, _)| (b.channel == channel, b.banned_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(login: &str) -> Banned {
        Banned::new(login, "chan", 0)
    }

    #[test]
    fn same_core_with_numbers_or_lookalikes() {
        assert_eq!(core_name("Troll_Guy_42"), "trollguy");
        assert_eq!(core_name("x1dl0t"), "xldlot");
        assert!(resemblance("troll_guy2", &b("troll_guy")).is_some());
        assert!(resemblance("tr0ll_guy", &b("troll_guy")).is_some());
        assert!(resemblance("hater9000", &b("hater_1")).is_some());
        assert!(resemblance("rnean_person", &b("mean_person")).is_some());
    }

    #[test]
    fn fuzzy_matches_need_long_names() {
        assert!(resemblance("trollguyy", &b("trollguy")).is_some()); // one letter added
        assert!(resemblance("trollgay", &b("trollguy")).is_some()); // one letter changed
        assert!(resemblance("trollguyisback", &b("trollguy")).is_some());
        assert!(resemblance("bobby", &b("bobbx")).is_none()); // too short for fuzzy matching
        assert!(resemblance("gamer", &b("gamer2")).is_some()); // exact core still counts
    }

    #[test]
    fn unrelated_or_identical_names_dont_match() {
        assert!(resemblance("troll_guy", &b("troll_guy")).is_none()); // that's the banned user themself
        assert!(resemblance("nicefriend", &b("trollguy")).is_none());
        assert!(resemblance("abc1", &b("abc2")).is_none()); // too generic
    }

    #[test]
    fn same_channel_match_wins() {
        let list = vec![Banned::new("troll_guy", "other", 50), Banned::new("troll_guy1", "chan", 10)];
        let (m, _) = best_match("troll_guy7", "chan", &list).unwrap();
        assert_eq!(m.channel, "chan");
    }
}
