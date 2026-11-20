//! Token counting for the chunks output: by default an estimate that needs
//! no tokenizer, or a real tokenizer when one is given (a Hugging Face
//! `tokenizer.json`, OpenAI's byte-pair encodings, or any
//! counting function).
//!
//! The estimate splits the text as subword tokenizers do before their
//! vocabulary applies: runs of letters (cut at case changes, so that
//! `MedDRA` is `Med` + `DRA`), single digits, single symbols, and the space
//! before a number (byte-level tokenizers keep it apart from the digits,
//! which counts in tables of numbers). Each piece is then worth a number of
//! tokens by its length. The constants were fitted
//! on 20,000 pieces of text from 100 arXiv papers (chunks and paragraphs)
//! against two different tokenizers, Qwen3's byte-level BPE and BERT's
//! WordPiece: the estimate is 15 to 20% above the real count on average,
//! and under it by more than 10% for fewer than 1 text in 100 (names,
//! e-mail addresses, all-capital titles), so that a chunk sized by the
//! estimate seldom exceeds a model's limit.

use std::sync::Arc;

use anyhow::Result;

/// Letters of a lower-case (or capitalised) word that make one token: a
/// word of up to 6 letters is one token, 7 to 12 are two, and so on.
const LOWER_LETTERS_PER_TOKEN: usize = 6;

/// Letters of an all-capital word (acronym) that make one token: tokenizers
/// know few of them and cut them short.
const UPPER_LETTERS_PER_TOKEN: usize = 2;

/// Tokens of one character outside ASCII (accented letters, Greek, math
/// symbols): byte-level tokenizers often need two or more.
const NON_ASCII_TOKENS_PER_CHAR: usize = 2;

/// Names of the byte-pair encodings that `--tokenizer` takes by name.
pub const NAMED_TOKENIZERS: [&str; 2] = ["cl100k_base", "o200k_base"];

/// How the chunks output counts tokens. [`TokenCounter::default`] is the
/// estimate; [`TokenCounter::load`] reads a real tokenizer, and
/// [`TokenCounter::custom`] wraps any counting function.
#[derive(Clone, Default)]
pub struct TokenCounter {
    /// The counter itself.
    kind: CounterKind,
}

/// The kinds of [`TokenCounter`].
#[derive(Clone, Default)]
enum CounterKind {
    /// [`estimate`].
    #[default]
    Estimate,
    /// A function given by the caller.
    Custom(Arc<dyn Fn(&str) -> usize + Send + Sync>),
    /// A Hugging Face tokenizer, without truncation or padding.
    #[cfg(feature = "tokenizers")]
    HuggingFace(Arc<tokenizers::Tokenizer>),
    /// One of OpenAI's byte-pair encodings.
    #[cfg(feature = "openai-tokenizers")]
    OpenAi(&'static tiktoken_rs::CoreBPE),
}

impl std::fmt::Debug for TokenCounter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match &self.kind {
            CounterKind::Estimate => "estimate",
            CounterKind::Custom(_) => "custom",
            #[cfg(feature = "tokenizers")]
            CounterKind::HuggingFace(_) => "tokenizer.json",
            #[cfg(feature = "openai-tokenizers")]
            CounterKind::OpenAi(_) => "byte-pair encoding",
        };
        write!(f, "TokenCounter({name})")
    }
}

impl TokenCounter {
    /// The estimate (see the module documentation): no tokenizer needed.
    pub fn estimate() -> Self {
        Self::default()
    }

    /// Counts tokens with `count`, for example a tokenizer of another
    /// library.
    pub fn custom(count: impl Fn(&str) -> usize + Send + Sync + 'static) -> Self {
        Self {
            kind: CounterKind::Custom(Arc::new(count)),
        }
    }

    /// A real tokenizer: one of [`NAMED_TOKENIZERS`] by name (OpenAI's
    /// `cl100k_base` and `o200k_base`), or the path
    /// of a Hugging Face `tokenizer.json` (the file next to an embedding
    /// model's weights). Truncation and padding set in the file are turned
    /// off, and special tokens (`[CLS]`, `<s>`) are not counted.
    ///
    /// # Errors
    /// When the file cannot be read as a tokenizer, or kakemono was built
    /// without the feature that reads this kind of tokenizer
    /// (`tokenizers`, `openai-tokenizers`).
    pub fn load(spec: &str) -> Result<Self> {
        if NAMED_TOKENIZERS.contains(&spec) {
            return Self::named(spec);
        }
        Self::from_file(spec)
    }

    /// The number of tokens of `text`.
    pub fn count(&self, text: &str) -> usize {
        match &self.kind {
            CounterKind::Estimate => estimate(text),
            CounterKind::Custom(count) => count(text),
            #[cfg(feature = "tokenizers")]
            CounterKind::HuggingFace(tokenizer) => tokenizer
                .encode_fast(text, false)
                .map_or_else(|_| estimate(text), |enc| enc.len()),
            #[cfg(feature = "openai-tokenizers")]
            CounterKind::OpenAi(bpe) => bpe.encode_ordinary(text).len(),
        }
    }

    /// One of [`NAMED_TOKENIZERS`].
    #[cfg(feature = "openai-tokenizers")]
    fn named(name: &str) -> Result<Self> {
        let bpe = if name == "cl100k_base" {
            tiktoken_rs::cl100k_base_singleton()
        } else {
            tiktoken_rs::o200k_base_singleton()
        };
        Ok(Self {
            kind: CounterKind::OpenAi(bpe),
        })
    }

    /// One of [`NAMED_TOKENIZERS`]: not built in.
    #[cfg(not(feature = "openai-tokenizers"))]
    fn named(name: &str) -> Result<Self> {
        anyhow::bail!("this kakemono was built without the openai-tokenizers feature: {name} is not available")
    }

    /// A Hugging Face `tokenizer.json`.
    #[cfg(feature = "tokenizers")]
    fn from_file(path: &str) -> Result<Self> {
        let mut tokenizer = tokenizers::Tokenizer::from_file(path)
            .map_err(|err| anyhow::anyhow!("cannot read the tokenizer {path}: {err}"))?;
        // A tokenizer.json often truncates at the model's limit: the count
        // must see the whole text.
        tokenizer
            .with_truncation(None)
            .map_err(|err| anyhow::anyhow!("cannot turn off truncation of {path}: {err}"))?;
        tokenizer.with_padding(None);
        Ok(Self {
            kind: CounterKind::HuggingFace(Arc::new(tokenizer)),
        })
    }

    /// A Hugging Face `tokenizer.json`: not built in.
    #[cfg(not(feature = "tokenizers"))]
    fn from_file(path: &str) -> Result<Self> {
        anyhow::bail!("this kakemono was built without the tokenizers feature: cannot read {path}")
    }
}

/// The estimated number of tokens of `text` (see the module
/// documentation). White space counts for nothing, except before a digit.
pub fn estimate(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = 0;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_ascii_alphabetic() {
            let (len, upper) = letter_piece(&chars[i..]);
            let per_token = if upper && len > 1 {
                UPPER_LETTERS_PER_TOKEN
            } else {
                LOWER_LETTERS_PER_TOKEN
            };
            tokens += 1 + (len - 1) / per_token;
            i += len;
            continue;
        }
        if ch.is_whitespace() {
            if chars.get(i + 1).is_some_and(char::is_ascii_digit) {
                tokens += 1;
            }
        } else {
            tokens += if ch.is_ascii() && !ch.is_ascii_control() {
                1
            } else {
                NON_ASCII_TOKENS_PER_CHAR
            };
        }
        i += 1;
    }
    tokens
}

/// The ASCII word that starts `chars` (which starts with a letter), cut at
/// case changes: an optional capital and lower-case letters (`Med`), or a
/// run of capitals not followed by a lower-case letter (`DRA`, and `HTML`
/// in `HTMLParser`). Returns its length and whether it is all capitals.
fn letter_piece(chars: &[char]) -> (usize, bool) {
    let upper = chars.iter().take_while(|ch| ch.is_ascii_uppercase()).count();
    let lower_after = |from: usize| chars[from..].iter().take_while(|ch| ch.is_ascii_lowercase()).count();
    match upper {
        0 => (lower_after(0), false),
        1 => (1 + lower_after(1), lower_after(1) == 0),
        _ if lower_after(upper) > 0 => (upper - 1, true), // the last capital starts the next word
        _ => (upper, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_words_are_one_token_each() {
        assert_eq!(estimate("the cat sat on a mat"), 6);
    }

    #[test]
    fn long_words_and_acronyms_count_more() {
        // "transformers" has 12 letters: two tokens; "BERT" four capitals: two.
        assert_eq!(estimate("transformers"), 2);
        assert_eq!(estimate("BERT"), 2);
    }

    #[test]
    fn camel_case_splits_at_case_changes() {
        // Med + DRA: 1 + 2; HTML + Parser: 2 + 1.
        assert_eq!(estimate("MedDRA"), 3);
        assert_eq!(estimate("HTMLParser"), 3);
    }

    #[test]
    fn digits_and_symbols_count_one_each() {
        assert_eq!(estimate("3.14"), 4);
        assert_eq!(estimate("α"), NON_ASCII_TOKENS_PER_CHAR);
    }

    #[test]
    fn white_space_counts_nothing_but_before_numbers() {
        assert_eq!(estimate("a b"), estimate("a") + estimate("b"));
        assert_eq!(estimate(" \n\t"), 0);
        // "x", the space before "12", "1", "2".
        assert_eq!(estimate("x 12"), 4);
    }

    /// A word-level tokenizer.json: one token per word, truncated at 2.
    #[cfg(feature = "tokenizers")]
    const WORD_TOKENIZER: &str = r#"{"version":"1.0","truncation":{"direction":"Right","max_length":2,"strategy":"LongestFirst","stride":0},
"padding":null,"added_tokens":[],"normalizer":null,"pre_tokenizer":{"type":"Whitespace"},"post_processor":null,"decoder":null,
"model":{"type":"WordLevel","vocab":{"[UNK]":0,"a":1},"unk_token":"[UNK]"}}"#;

    #[cfg(feature = "tokenizers")]
    #[test]
    fn a_tokenizer_json_counts_without_truncation() {
        let path = std::env::temp_dir().join(format!("kakemono-tokenizer-{}.json", std::process::id()));
        std::fs::write(&path, WORD_TOKENIZER).unwrap();
        let counter = TokenCounter::load(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        // Five words, although the file truncates at two.
        assert_eq!(counter.count("a b c d e"), 5);
    }

    #[test]
    fn a_missing_tokenizer_is_an_error() {
        assert!(TokenCounter::load("/no/such/tokenizer.json").is_err());
    }

    #[cfg(feature = "openai-tokenizers")]
    #[test]
    fn openai_tokenizers_are_known_by_name() {
        let counter = TokenCounter::load("cl100k_base").unwrap();
        assert_eq!(counter.count("hello world"), 2);
    }

    #[test]
    fn a_custom_counter_is_used() {
        let counter = TokenCounter::custom(|text| text.len());
        assert_eq!(counter.count("abcd"), 4);
    }
}
