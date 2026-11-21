//! On-device toxicity model: a small BERT classifier (MiniLMv2, 6 layers) that scores how
//! toxic a message is. It runs on the CPU in pure Rust (candle); no text leaves the computer.
//!
//! Model: minuva/MiniLMv2-toxic-jigsaw-lite (Apache-2.0), converted to half precision.
//! See `resources/toxicity/README.md` for where it came from and how it was converted.

use candle_core::{DType, Device, Module, Tensor};
use candle_nn::{Linear, VarBuilder};
use candle_transformers::models::bert::{BertModel, Config};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

/// Longest input in tokens (chat messages are at most 500 characters).
const MAX_TOKENS: usize = 128;
const CLS: u32 = 101;
const SEP: u32 = 102;
const UNK: u32 = 100;

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Scores {
    /// 0–1: how likely the message is toxic (insults, harassment, slurs, threats…).
    pub toxic: f32,
    /// 0–1: how likely it's severely toxic.
    pub severe: f32,
}

// ---------------- tokenizer ----------------

/// BERT's uncased WordPiece tokenizer (lowercase, strip accents, split punctuation).
pub struct Tokenizer {
    vocab: HashMap<String, u32>,
}

fn is_punctuation(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_punctuation();
    }
    matches!(c as u32,
        0x00A1..=0x00BF | 0x2010..=0x2027 | 0x2030..=0x205E | 0x3001..=0x3003 | 0x3008..=0x3011
        | 0x3014..=0x301F | 0xFE50..=0xFE6B | 0xFF01..=0xFF0F | 0xFF1A..=0xFF20 | 0xFF3B..=0xFF40 | 0xFF5B..=0xFF65)
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x2A6DF | 0x2A700..=0x2B73F | 0x2B740..=0x2B81F
        | 0x2B820..=0x2CEAF | 0xF900..=0xFAFF | 0x2F800..=0x2FA1F)
}

fn is_combining_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

impl Tokenizer {
    pub fn from_vocab(text: &str) -> Self {
        Tokenizer { vocab: text.lines().enumerate().map(|(i, l)| (l.to_string(), i as u32)).collect() }
    }

    fn words(text: &str) -> Vec<String> {
        let cleaned: String = text.to_lowercase().nfd().filter(|c| !is_combining_mark(*c)).collect();
        let mut words = Vec::new();
        let mut cur = String::new();
        for c in cleaned.chars() {
            if c.is_whitespace() {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            } else if is_punctuation(c) || is_cjk(c) {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
                words.push(c.to_string());
            } else if !c.is_control() && c != '\u{FFFD}' {
                cur.push(c);
            }
        }
        if !cur.is_empty() {
            words.push(cur);
        }
        words
    }

    fn word_pieces(&self, word: &str, out: &mut Vec<u32>) {
        let chars: Vec<char> = word.chars().collect();
        if chars.len() > 100 {
            out.push(UNK);
            return;
        }
        let mut pieces = Vec::new();
        let mut start = 0;
        while start < chars.len() {
            let mut end = chars.len();
            let mut found = None;
            while start < end {
                let piece: String = chars[start..end].iter().collect();
                let key = if start > 0 { format!("##{piece}") } else { piece };
                if let Some(id) = self.vocab.get(&key) {
                    found = Some(*id);
                    break;
                }
                end -= 1;
            }
            match found {
                Some(id) => {
                    pieces.push(id);
                    start = end;
                }
                None => {
                    out.push(UNK);
                    return;
                }
            }
        }
        out.extend(pieces);
    }

    /// Token ids including [CLS] and [SEP], cut to `MAX_TOKENS`.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut ids = vec![CLS];
        for w in Self::words(text) {
            self.word_pieces(&w, &mut ids);
            if ids.len() >= MAX_TOKENS - 1 {
                break;
            }
        }
        ids.truncate(MAX_TOKENS - 1);
        ids.push(SEP);
        ids
    }
}

// ---------------- model ----------------

pub struct Classifier {
    bert: BertModel,
    pooler: Linear,
    head: Linear,
    tokenizer: Tokenizer,
    device: Device,
}

impl Classifier {
    /// Load `model.safetensors`, `config.json` and `vocab.txt` from `dir`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let read = |name: &str| std::fs::read(dir.join(name)).map_err(|e| format!("Couldn't read the toxicity model's {name}: {e}"));
        let config: Config = serde_json::from_slice(&read("config.json")?).map_err(|e| format!("Bad model config: {e}"))?;
        let vocab = String::from_utf8(read("vocab.txt")?).map_err(|e| e.to_string())?;
        let weights = read("model.safetensors")?;
        let device = Device::Cpu;
        let vb = VarBuilder::from_buffered_safetensors(weights, DType::F32, &device).map_err(|e| format!("Bad model file: {e}"))?;
        let hidden = config.hidden_size;
        let bert = BertModel::load(vb.clone(), &config).map_err(|e| format!("Couldn't load the model: {e}"))?;
        let pooler = candle_nn::linear(hidden, hidden, vb.pp("bert.pooler.dense")).map_err(|e| e.to_string())?;
        let head = candle_nn::linear(hidden, 2, vb.pp("classifier")).map_err(|e| e.to_string())?;
        Ok(Classifier { bert, pooler, head, tokenizer: Tokenizer::from_vocab(&vocab), device })
    }

    pub fn score(&self, text: &str) -> Result<Scores, String> {
        let run = || -> candle_core::Result<Vec<f32>> {
            let ids = self.tokenizer.encode(text);
            let n = ids.len();
            let input = Tensor::from_vec(ids, (1, n), &self.device)?;
            let types = input.zeros_like()?;
            let mask = input.ones_like()?;
            let hidden = self.bert.forward(&input, &types, Some(&mask))?;
            let first = hidden.narrow(1, 0, 1)?.squeeze(1)?;
            let pooled = self.pooler.forward(&first)?.tanh()?;
            let logits = self.head.forward(&pooled)?;
            candle_nn::ops::sigmoid(&logits)?.squeeze(0)?.to_vec1::<f32>()
        };
        let v = run().map_err(|e| e.to_string())?;
        Ok(Scores { toxic: v[0], severe: v[1] })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/toxicity")
    }

    #[test]
    fn tokenizes_like_bert_uncased() {
        let t = Tokenizer::from_vocab(&std::fs::read_to_string(model_dir().join("vocab.txt")).unwrap());
        assert_eq!(t.encode("This is pure trash"), vec![101, 2023, 2003, 5760, 11669, 102]);
        assert_eq!(t.encode("go left!! wrong way"), vec![101, 2175, 2187, 999, 999, 3308, 2126, 102]);
        // Accents are stripped and unknown words become [UNK].
        assert_eq!(t.encode("café"), t.encode("cafe"));
        assert!(t.encode(&"x".repeat(400)).contains(&UNK));
        assert!(t.encode(&"word ".repeat(300)).len() <= MAX_TOKENS);
    }

    #[test]
    fn scores_match_the_reference_model() {
        let c = Classifier::load(&model_dir()).unwrap();
        // The model card's reference output for this sentence is 0.887.
        let s = c.score("This is pure trash").unwrap();
        assert!((s.toxic - 0.887).abs() < 0.01, "{s:?}");
        assert!(c.score("you are an idiot and nobody likes you").unwrap().toxic > 0.9);
        assert!(c.score("gg wp that was a great stream").unwrap().toxic < 0.05);
        assert!(c.score("go left!! wrong way").unwrap().toxic < 0.05);
        assert!(c.score("").unwrap().toxic < 0.5); // empty input still works
    }
}
