//! Text normalization that defeats common filter evasion:
//! zero-width characters, accents, full-width/stylized letters, Cyrillic/Greek
//! lookalikes and leetspeak. Keeps a map back to the original text so matches
//! can be highlighted where they actually appear.

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Normalized text plus, for every byte of it, the byte range in the original.
pub struct Normalized {
    pub text: String,
    spans: Vec<(usize, usize)>,
}

impl Normalized {
    /// Map a byte range in the normalized text back to the original text.
    pub fn original_range(&self, start: usize, end: usize) -> (usize, usize) {
        if start >= end || end > self.spans.len() {
            return (0, 0);
        }
        (self.spans[start].0, self.spans[end - 1].1)
    }
}

fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}' | '\u{034F}' | '\u{00AD}'
            | '\u{180E}' | '\u{E0000}'..='\u{E007F}'
    )
}

fn lookalike(c: char) -> char {
    match c {
        // Cyrillic
        'а' | 'А' => 'a',
        'в' | 'В' => 'b',
        'е' | 'Е' | 'ё' | 'Ё' => 'e',
        'к' | 'К' => 'k',
        'м' | 'М' => 'm',
        'н' | 'Н' => 'h',
        'о' | 'О' => 'o',
        'р' | 'Р' => 'p',
        'с' | 'С' => 'c',
        'т' | 'Т' => 't',
        'у' | 'У' => 'y',
        'х' | 'Х' => 'x',
        'і' | 'І' | 'ї' => 'i',
        'ј' | 'Ј' => 'j',
        'ѕ' | 'Ѕ' => 's',
        'ԁ' => 'd',
        'ɡ' => 'g',
        'ӏ' => 'l',
        // Greek
        'α' | 'Α' => 'a',
        'β' | 'Β' => 'b',
        'ε' | 'Ε' => 'e',
        'η' | 'Η' => 'h',
        'ι' | 'Ι' => 'i',
        'κ' | 'Κ' => 'k',
        'ν' | 'Ν' => 'v',
        'ο' | 'Ο' => 'o',
        'ρ' | 'Ρ' => 'p',
        'τ' | 'Τ' => 't',
        'υ' | 'Υ' => 'u',
        'χ' | 'Χ' => 'x',
        'ζ' | 'Ζ' => 'z',
        _ => c,
    }
}

fn leet(c: char) -> char {
    match c {
        '0' => 'o',
        '1' => 'i',
        '3' => 'e',
        '4' => 'a',
        '5' => 's',
        '7' => 't',
        '8' => 'b',
        '9' => 'g',
        _ => c,
    }
}

/// Normalize `input`. Always lowercases; `leetspeak` additionally maps digits/symbols to letters.
pub fn normalize(input: &str) -> Normalized {
    let mut text = String::with_capacity(input.len());
    let mut spans = Vec::with_capacity(input.len());
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    for (i, &(start, c)) in chars.iter().enumerate() {
        let end = start + c.len_utf8();
        if is_invisible(c) {
            continue;
        }
        // `@` and `$` stand in for letters only inside words ("fre@k", "$cam"),
        // so @mentions and prices are left alone.
        let prev_alnum = i > 0 && chars[i - 1].1.is_alphanumeric();
        let next_alpha = chars.get(i + 1).is_some_and(|(_, n)| n.is_alphabetic());
        let c = match c {
            '@' if prev_alnum && next_alpha => 'a',
            '$' if next_alpha => 's',
            other => other,
        };
        // NFKD turns stylized/full-width letters into plain ones and splits off accents.
        for d in c.nfkd() {
            if is_combining_mark(d) || is_invisible(d) {
                continue;
            }
            for l in lookalike(d).to_lowercase() {
                let out = leet(l);
                let before = text.len();
                text.push(out);
                spans.extend(std::iter::repeat_n((start, end), text.len() - before));
            }
        }
    }
    Normalized { text, spans }
}

/// Normalize a rule pattern the same way message text is normalized.
pub fn normalize_pattern(p: &str) -> String {
    normalize(p).text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defeats_common_evasion() {
        assert_eq!(normalize("FR33 F0LL0W3RS").text, "free followers");
        assert_eq!(normalize("ｆｒｅｅ").text, "free"); // full-width
        assert_eq!(normalize("frее").text, "free"); // Cyrillic е
        assert_eq!(normalize("f\u{200B}ree").text, "free"); // zero-width space
        assert_eq!(normalize("fréé").text, "free"); // accents
        assert_eq!(normalize("𝐟𝐫𝐞𝐞").text, "free"); // math bold
        assert_eq!(normalize("LOL \u{034F}").text, "lol ");
        assert_eq!(normalize("$cam fre@k").text, "scam freak");
        assert_eq!(normalize("@mod costs $5!").text, "@mod costs $s!");
    }

    #[test]
    fn maps_back_to_original_text() {
        let src = "get fr33 f0llowers!";
        let n = normalize(src);
        let start = n.text.find("free followers").unwrap();
        let (a, b) = n.original_range(start, start + "free followers".len());
        assert_eq!(&src[a..b], "fr33 f0llowers");

        let src = "ｆｒｅｅ stuff";
        let n = normalize(src);
        let (a, b) = n.original_range(0, 4);
        assert_eq!(&src[a..b], "ｆｒｅｅ");
    }
}
