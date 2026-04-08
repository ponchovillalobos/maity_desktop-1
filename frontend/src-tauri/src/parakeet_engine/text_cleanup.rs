// src/parakeet_engine/text_cleanup.rs
//
// UX-013: Hallucination filter + text cleanup for Parakeet transcripts.
//
// Parakeet (like most CTC/RNN-T models) produces three classes of noise that
// are worth stripping before showing text to the user or sending it to the
// summarizer:
//
// 1. Special / meta tokens that leak out of the vocab: `[blank]`, `<unk>`,
//    `<s>`, `</s>`, `[music]`, `[applause]`, `[laughter]`, `▁` (SentencePiece
//    word marker), etc.
// 2. Consecutive repeated words: "the the the the" → "the". This is the
//    classic CTC hallucination when the model gets stuck on a filler or on
//    silence that slipped past the VAD.
// 3. Repeated phrases: "thank you thank you thank you" → "thank you". Seen
//    when the encoder state loops on a prosodic feature.
//
// All cleanup is deterministic, allocation-light, and Unicode-aware (we split
// on whitespace, not char boundaries, so acentos and emojis survive).

/// Tokens that should be stripped from Parakeet output. Exact match, case
/// insensitive for the ones in `[...]` / `<...>` form.
const SPECIAL_TOKENS: &[&str] = &[
    "[blank]",
    "[pad]",
    "[unk]",
    "[music]",
    "[applause]",
    "[laughter]",
    "[noise]",
    "[silence]",
    "<blank>",
    "<pad>",
    "<unk>",
    "<s>",
    "</s>",
    "<|endoftext|>",
    "<|startoftranscript|>",
];

/// Maximum run length we allow for a single repeated word. Above this, we
/// collapse to a single occurrence. 2 is conservative — legitimate text like
/// "muy muy bueno" survives, but "the the the the" collapses to "the".
const MAX_CONSECUTIVE_WORD_RUN: usize = 2;

/// Maximum run length for repeated multi-word phrases (2-5 word windows).
/// We collapse to a single copy above this.
const MAX_CONSECUTIVE_PHRASE_RUN: usize = 2;

/// Maximum phrase window size we scan for repetition.
const MAX_PHRASE_WINDOW: usize = 5;

/// Minimum non-whitespace characters for a transcript to be considered
/// non-empty after cleanup. Shorter outputs are returned as empty string.
const MIN_TRANSCRIPT_CHARS: usize = 1;

/// Public entry point: apply the full cleanup chain to a raw Parakeet
/// transcript. Returns an empty string if the result is pure noise.
pub fn clean_transcription(raw: &str) -> String {
    let stripped = strip_special_tokens(raw);
    let deduped_words = dedupe_consecutive_words(&stripped);
    let deduped_phrases = dedupe_consecutive_phrases(&deduped_words);
    let normalized = normalize_whitespace(&deduped_phrases);
    if normalized.chars().filter(|c| !c.is_whitespace()).count() < MIN_TRANSCRIPT_CHARS {
        return String::new();
    }
    normalized
}

/// Remove `[blank]`, `<unk>`, SentencePiece `▁` word markers, and similar
/// meta tokens. Case-insensitive on the bracketed forms.
pub fn strip_special_tokens(text: &str) -> String {
    let mut out = text.replace('▁', " "); // SentencePiece word boundary
    for tok in SPECIAL_TOKENS {
        // Case-insensitive replacement without pulling in a regex engine.
        let tok_lower = tok.to_lowercase();
        let mut cleaned = String::with_capacity(out.len());
        let lower = out.to_lowercase();
        let mut cursor = 0;
        while let Some(pos) = lower[cursor..].find(&tok_lower) {
            let abs = cursor + pos;
            cleaned.push_str(&out[cursor..abs]);
            cursor = abs + tok.len();
        }
        cleaned.push_str(&out[cursor..]);
        out = cleaned;
    }
    out
}

/// Collapse runs of the same word longer than `MAX_CONSECUTIVE_WORD_RUN`
/// to a single occurrence. Case-insensitive comparison, preserves the
/// first-occurrence casing.
pub fn dedupe_consecutive_words(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return String::new();
    }

    let mut kept: Vec<&str> = Vec::with_capacity(words.len());
    let mut run_len = 0usize;
    let mut prev_lower = String::new();

    for w in words {
        let lower = w.to_lowercase();
        if lower == prev_lower {
            run_len += 1;
            if run_len <= MAX_CONSECUTIVE_WORD_RUN {
                kept.push(w);
            }
            // else: skip — part of a long repetition run
        } else {
            kept.push(w);
            run_len = 1;
            prev_lower = lower;
        }
    }

    kept.join(" ")
}

/// Collapse runs of repeated multi-word phrases. We scan windows of size
/// 2..=MAX_PHRASE_WINDOW and, when we find a phrase repeated more than
/// `MAX_CONSECUTIVE_PHRASE_RUN` times, we keep only one copy.
pub fn dedupe_consecutive_phrases(text: &str) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.to_string())
        .collect();
    if words.len() < 4 {
        return text.to_string();
    }

    // Greedy pass: for each position, try the largest window that yields a
    // repetition, collapse it, then continue.
    let mut result: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0usize;
    while i < words.len() {
        let mut collapsed = false;
        // Try windows from large to small so we catch the longest repeating unit.
        let max_window = MAX_PHRASE_WINDOW.min((words.len() - i) / 2);
        for w in (2..=max_window).rev() {
            let phrase = &words[i..i + w];
            let phrase_lower: Vec<String> = phrase.iter().map(|s| s.to_lowercase()).collect();
            // Count how many consecutive copies follow.
            let mut copies = 1usize;
            let mut j = i + w;
            while j + w <= words.len() {
                let next: Vec<String> = words[j..j + w].iter().map(|s| s.to_lowercase()).collect();
                if next == phrase_lower {
                    copies += 1;
                    j += w;
                } else {
                    break;
                }
            }
            if copies > MAX_CONSECUTIVE_PHRASE_RUN {
                // Keep only one copy of the phrase, skip the rest.
                for p in phrase {
                    result.push(p.clone());
                }
                i = j;
                collapsed = true;
                break;
            }
        }
        if !collapsed {
            result.push(words[i].clone());
            i += 1;
        }
    }

    result.join(" ")
}

/// Collapse runs of whitespace (including newlines/tabs) into a single
/// space, and trim leading/trailing whitespace.
pub fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_blank_token() {
        assert_eq!(strip_special_tokens("hello [blank] world"), "hello  world");
    }

    #[test]
    fn strips_unk_and_sentencepiece() {
        let cleaned = strip_special_tokens("<unk>▁hola▁mundo");
        assert!(cleaned.contains("hola"));
        assert!(cleaned.contains("mundo"));
        assert!(!cleaned.contains('▁'));
        assert!(!cleaned.contains("<unk>"));
    }

    #[test]
    fn strips_case_insensitive() {
        assert_eq!(strip_special_tokens("[BLANK]hola"), "hola");
        assert_eq!(strip_special_tokens("[Music]"), "");
    }

    #[test]
    fn dedupe_consecutive_collapses_long_runs() {
        // 4 repetitions → collapses to MAX_CONSECUTIVE_WORD_RUN (2).
        let out = dedupe_consecutive_words("the the the the world");
        assert_eq!(out, "the the world");
    }

    #[test]
    fn dedupe_consecutive_preserves_legitimate_repetition() {
        // "muy muy bueno" (2 repetitions) survives.
        let out = dedupe_consecutive_words("muy muy bueno");
        assert_eq!(out, "muy muy bueno");
    }

    #[test]
    fn dedupe_consecutive_is_case_insensitive() {
        let out = dedupe_consecutive_words("The the THE the hello");
        assert_eq!(out, "The the hello");
    }

    #[test]
    fn dedupe_phrases_collapses_thank_you_loop() {
        let out = dedupe_consecutive_phrases("thank you thank you thank you thank you");
        // 4 copies > MAX_CONSECUTIVE_PHRASE_RUN → one copy retained.
        assert_eq!(out, "thank you");
    }

    #[test]
    fn dedupe_phrases_preserves_normal_text() {
        let out = dedupe_consecutive_phrases("hola como estas hoy");
        assert_eq!(out, "hola como estas hoy");
    }

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize_whitespace("hola   \n\t mundo"), "hola mundo");
    }

    #[test]
    fn full_chain_cleans_hallucination() {
        let raw = "[blank] the the the the thank you thank you thank you thank you <unk>";
        let cleaned = clean_transcription(raw);
        // After strip, dedupe words, dedupe phrases, normalize:
        assert!(cleaned.contains("thank you"));
        assert!(!cleaned.contains("[blank]"));
        assert!(!cleaned.contains("<unk>"));
        // No 4-in-a-row of "the"
        let the_count = cleaned.split_whitespace().filter(|w| w.eq_ignore_ascii_case("the")).count();
        assert!(the_count <= 2, "too many 'the' left: {:?}", cleaned);
    }

    #[test]
    fn full_chain_empty_on_pure_noise() {
        assert_eq!(clean_transcription("[blank][blank]<unk>"), "");
        assert_eq!(clean_transcription("   "), "");
    }

    #[test]
    fn full_chain_preserves_spanish_accents() {
        let out = clean_transcription("holá cómo estás");
        assert_eq!(out, "holá cómo estás");
    }
}
