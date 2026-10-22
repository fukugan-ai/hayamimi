//! Decoding with a tier: the recognizers kept in memory, the whole-buffer
//! decode, and the split retry for an utterance whose first words the
//! recognizer dropped.
//!
//! This module does not choose a language or a tier: the caller names the
//! tier, and a tier that is not installed gives way to the next one in its
//! chain (see [`Tier::chain`]). The public entry points are
//! [`Recognizers::decode`] (whole decode and retry), and its two halves
//! [`Recognizers::decode_whole`] and [`Recognizers::retry_if_truncated`] for a
//! caller that must decide between them.
//!
//! A long buffer whose text is too sparse for its speech time is suspected of
//! having lost its beginning. It is then cut at its inner silences and decoded
//! piece by piece, and the result replaces the whole-buffer text only when it
//! clearly recovers it. Cutting every buffer would lose more than it recovers,
//! so the whole-buffer text stays whenever the answer is unclear. The live
//! route never reaches the retry: the detector's own segments hold no inner
//! silence of the length used here.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::audio::SAMPLE_RATE;
use crate::cap::{Cut, DecodeCap};
use crate::error::AsrError;
use crate::fallback::{Reason, reason_of};
use crate::recognizer::{LangTag, Recognizer};
use crate::resident::Residents;
use crate::retry::{MIN_BUFFER_SECONDS, Pieces, Splitter};
use crate::route::Tier;
use crate::tier::is_installed;

/// Alphanumeric characters per second of speech below which a decode in a
/// language written without spaces is suspected of having dropped its
/// beginning: the log-midpoint between the slowest healthy clip measured and
/// the known defect.
pub const DENSITY_FLOOR_SPACELESS: f64 = 2.4;
/// The same for the other languages: Latin script packs fewer phonemes into a
/// character, so the floor is the measured one times about 2.5.
pub const DENSITY_FLOOR_SPACED: f64 = 6.0;
/// How many trailing characters of the whole-buffer text must reappear in the retry text ...
pub const TAIL_CHARS: usize = 12;
/// ... and what share of them.
pub const TAIL_MATCH: f64 = 0.6;
/// Without the cutting detector, a buffer is cut into chunks of this length ...
pub const CHUNK_SECONDS: f64 = 8.0;
/// ... that overlap by this much, so a word on a seam is whole in one of two chunks.
pub const CHUNK_OVERLAP_SECONDS: f64 = 0.25;
/// A chunk shorter than this is dropped.
const MIN_CHUNK_SECONDS: f64 = 0.2;
/// The languages written without spaces between words.
const SPACELESS: [&str; 4] = ["ja", "zh", "yue", "ko"];

/// Whether pieces of text in `lang` are joined with nothing between them.
pub fn is_spaceless(lang: &str) -> bool {
    SPACELESS.contains(&lang)
}

fn density_floor(lang: &str) -> f64 {
    if is_spaceless(lang) {
        DENSITY_FLOOR_SPACELESS
    } else {
        DENSITY_FLOOR_SPACED
    }
}

/// Alphanumeric characters per second. Punctuation is not counted, so the
/// figure does not move when ITN or punctuation rewrites the text.
pub fn text_density(text: &str, seconds: f64) -> f64 {
    if seconds <= 0.0 {
        return f64::INFINITY;
    }
    text.chars().filter(|c| c.is_alphanumeric()).count() as f64 / seconds
}

/// Does this whole-buffer decode look like it lost its beginning?
///
/// `speech_seconds` is the speech time the density is measured against;
/// `buffer_seconds` is the length of the buffer, and a buffer of four seconds
/// or less is never suspected. Empty text is not suspicious: decoding silence
/// in pieces would only invent words.
pub fn looks_truncated(text: &str, speech_seconds: f64, buffer_seconds: f64, lang: &str) -> bool {
    if buffer_seconds <= MIN_BUFFER_SECONDS || text.trim().is_empty() {
        return false;
    }
    text_density(text, speech_seconds) < density_floor(lang)
}

/// The length of the longest run of characters that `a` and `b` share.
pub fn longest_common_substring(a: &[char], b: &[char]) -> usize {
    let mut best = 0;
    let mut previous = vec![0_usize; b.len() + 1];
    for x in a {
        let mut row = vec![0_usize; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            if x == y {
                row[j + 1] = previous[j] + 1;
                best = best.max(row[j + 1]);
            }
        }
        previous = row;
    }
    best
}

/// Is the retry a recovery rather than a different reading of the audio?
///
/// Both must hold: the retry is longer and its density is back in the healthy
/// range, and most of the whole decode's tail reappears in it. The whole
/// decode is the surviving end of the utterance, so a genuine recovery keeps
/// it and adds what was lost.
pub fn retry_is_better(whole: &str, retry: &str, speech_seconds: f64, lang: &str) -> bool {
    let (whole, retry) = (whole.trim(), retry.trim());
    if retry.is_empty() || retry.chars().count() <= whole.chars().count() {
        return false;
    }
    if text_density(retry, speech_seconds) < density_floor(lang) {
        return false;
    }
    let whole: Vec<char> = whole.chars().collect();
    let tail = &whole[whole.len().saturating_sub(TAIL_CHARS)..];
    if tail.is_empty() {
        return false;
    }
    let retry: Vec<char> = retry.chars().collect();
    longest_common_substring(tail, &retry) as f64 / tail.len() as f64 >= TAIL_MATCH
}

/// The pieces joined: with nothing between them for a language written
/// without spaces, with one space otherwise. Empty pieces drop out.
pub fn join_pieces(parts: &[String], lang: &str) -> String {
    let separator = if is_spaceless(lang) { "" } else { " " };
    parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<&str>>()
        .join(separator)
}

/// Fixed-length chunks with a small overlap, for a buffer that cannot be cut
/// at its silences because the cutting detector is not there. A word on a
/// seam can appear twice; that is the cost of having no detector. `None` when
/// the buffer fits one chunk.
pub fn fixed_chunks(samples: &[f32]) -> Option<Vec<Vec<f32>>> {
    let rate = f64::from(SAMPLE_RATE);
    let size = (CHUNK_SECONDS * rate) as usize;
    let step = ((CHUNK_SECONDS - CHUNK_OVERLAP_SECONDS) * rate) as usize;
    if step == 0 || samples.len() <= size {
        return None;
    }
    let shortest = (MIN_CHUNK_SECONDS * rate) as usize;
    let chunks: Vec<Vec<f32>> = (0..samples.len())
        .step_by(step)
        .map(|from| samples[from..(from + size).min(samples.len())].to_vec())
        .filter(|chunk| chunk.len() > shortest)
        .collect();
    if chunks.is_empty() {
        None
    } else {
        Some(chunks)
    }
}

/// Whether `c` is written one character per unit by the `cjkchar` modeling
/// unit: Han, kana and Hangul.
fn is_cjk_unit(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
        || ('\u{3040}'..='\u{30ff}').contains(&c)
        || ('\u{ac00}'..='\u{d7af}').contains(&c)
}

/// Splits a hot word phrase the way the `cjkchar` modeling unit encodes it:
/// each CJK character is a unit of its own, and a run of other characters
/// that are not white space is one unit.
pub fn cjkchar_units(phrase: &str) -> Vec<String> {
    let mut units = Vec::new();
    let mut word = String::new();
    for c in phrase.chars() {
        if c.is_whitespace() || is_cjk_unit(c) {
            if !word.is_empty() {
                units.push(std::mem::take(&mut word));
            }
            if is_cjk_unit(c) {
                units.push(c.to_string());
            }
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        units.push(word);
    }
    units
}

/// The tokens of a `tokens.txt`: each line is `<token> <id>`, and the id is
/// split off from the right so that a token holding a space stays whole.
fn token_vocabulary(text: &str) -> BTreeSet<&str> {
    text.lines()
        .filter_map(|line| line.trim_end().rsplit_once(char::is_whitespace))
        .map(|(token, _id)| token.trim_end())
        .filter(|token| !token.is_empty())
        .collect()
}

/// How many hot words the file holds, and how many of them the tokens cannot
/// encode. The recognizer only warns when it cannot encode a hot word and
/// goes on, so a caller counts them to say so. A hot word file that is not
/// there holds none. Blank lines and lines starting with `#` are not hot words.
pub fn check_hotwords_encodable(
    hotwords: &Path,
    tokens: &Path,
) -> Result<(usize, usize), AsrError> {
    if !hotwords.is_file() {
        return Ok((0, 0));
    }
    let io =
        |path: &Path, error: std::io::Error| AsrError::Io(format!("{}: {error}", path.display()));
    let words = std::fs::read_to_string(hotwords).map_err(|error| io(hotwords, error))?;
    let vocabulary_text = std::fs::read_to_string(tokens).map_err(|error| io(tokens, error))?;
    let vocabulary = token_vocabulary(&vocabulary_text);
    let phrases: Vec<&str> = words
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let unencodable = phrases
        .iter()
        .filter(|phrase| {
            let units = cjkchar_units(phrase);
            units.is_empty() || units.iter().any(|unit| !vocabulary.contains(unit.as_str()))
        })
        .count();
    Ok((phrases.len(), unencodable))
}

/// Something the recognizers did that the caller should say: a model loaded
/// or dropped, or a road taken because a model was not there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResidentEvent {
    /// A tier was loaded into memory; `ms` is how long that took.
    Loaded { tier: Tier, ms: f64 },
    /// A tier was dropped from memory to make room (least recently used).
    Evicted { tier: Tier },
    /// The tier asked for is not usable, and `used` answers in its place.
    Substituted {
        requested: Tier,
        used: Tier,
        reason: Reason,
    },
    /// The detector that cuts a long buffer is not usable: long buffers are
    /// cut into fixed chunks.
    CutIntoChunks { reason: Reason },
    /// SenseVoice wrote no language tag that could be read. Said once; every
    /// such decode still carries [`LangTag::Unknown`].
    LangUnknown,
    /// A buffer longer than the decode cap was read in pieces. Said for every
    /// such decode.
    DecodeChunked { tier: Tier, cut: Cut },
}

/// What a decode made.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub text: String,
    /// The language the model put on the whole-buffer decode (SenseVoice only).
    pub lang: LangTag,
    /// The tier that made the text, which is not the one asked for when that one was not installed.
    pub tier: Tier,
    /// The recognizer's time, the split retry included; loading a model is not.
    pub decode_ms: f64,
}

/// The recognizers of the routed engine, loaded when first used.
pub struct Recognizers {
    models: PathBuf,
    threads: i32,
    /// The longest audio one decode of the Japanese tier reads.
    cap: DecodeCap,
    residents: Residents<Recognizer>,
    /// Tiers that cannot be used on this machine, and why. Asked once.
    unavailable: BTreeMap<Tier, Reason>,
    /// Tiers whose substitution has been reported, so that it is said once.
    reported: BTreeSet<Tier>,
    splitter: Splitter,
    /// The cutting detector failed or is missing: long buffers use fixed chunks.
    chunked: bool,
    /// An unreadable language tag has been reported.
    lang_unknown_reported: bool,
    events: Vec<ResidentEvent>,
    /// Milliseconds spent building recognizers since [`Recognizers::take_load_ms`].
    load_ms: f64,
}

fn millis(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

impl Recognizers {
    /// An engine with nothing loaded. `max_resident` caps the tiers kept in
    /// memory besides the Japanese one; `None` keeps them all. `cap` limits
    /// the audio one decode of the Japanese tier reads.
    pub fn new(
        models: &Path,
        threads: i32,
        max_resident: Option<NonZeroUsize>,
        cap: DecodeCap,
    ) -> Recognizers {
        Recognizers {
            models: models.to_path_buf(),
            threads,
            cap,
            residents: Residents::new(max_resident),
            unavailable: BTreeMap::new(),
            reported: BTreeSet::new(),
            splitter: Splitter::new(models),
            chunked: false,
            lang_unknown_reported: false,
            events: Vec::new(),
            load_ms: 0.0,
        }
    }

    /// Makes `tier` resident. `Ok(None)` when it is; `Ok(Some(reason))` when
    /// it cannot be used here. Only a failure that is not about the model is an error.
    pub fn load(&mut self, tier: Tier) -> Result<Option<Reason>, AsrError> {
        if let Some(reason) = self.unavailable.get(&tier) {
            return Ok(Some(*reason));
        }
        if self.residents.get(tier).is_some() {
            return Ok(None);
        }
        if !is_installed(tier, &self.models) {
            self.unavailable.insert(tier, Reason::Missing);
            return Ok(Some(Reason::Missing));
        }
        // The recognizer used least recently goes first, so that the cap holds
        // while the new one is being built.
        if let Some(victim) = self.residents.make_room_for(tier) {
            self.events.push(ResidentEvent::Evicted { tier: victim });
        }
        let started = Instant::now();
        let opened = Recognizer::open_tier(&self.models, tier, self.threads, self.cap);
        let ms = millis(started);
        self.load_ms += ms;
        match opened {
            Ok(recognizer) => {
                self.events.push(ResidentEvent::Loaded { tier, ms });
                self.residents.insert(tier, recognizer);
                Ok(None)
            }
            Err(error) => {
                let Some(reason) = reason_of(&error) else {
                    return Err(error);
                };
                self.unavailable.insert(tier, reason);
                Ok(Some(reason))
            }
        }
    }

    /// The tier to decode with for a request of `requested`: the first of its
    /// chain that is usable. A substitution is reported once per requested tier.
    fn acquire(&mut self, requested: Tier) -> Result<Tier, AsrError> {
        let mut why_not: Option<Reason> = None;
        for candidate in requested.chain() {
            match self.load(candidate)? {
                None => {
                    if let Some(reason) = why_not
                        && self.reported.insert(requested)
                    {
                        self.events.push(ResidentEvent::Substituted {
                            requested,
                            used: candidate,
                            reason,
                        });
                    }
                    return Ok(candidate);
                }
                Some(reason) => why_not = why_not.or(Some(reason)),
            }
        }
        Err(AsrError::NoRecognizer)
    }

    /// Decodes the whole buffer with `tier`, or with the tier that answers for it.
    pub fn decode_whole(&mut self, tier: Tier, samples: &[f32]) -> Result<Decoded, AsrError> {
        let used = self.acquire(tier)?;
        self.decode_loaded(used, samples)
    }

    /// Decodes the whole buffer with `tier` itself and no other: `None` when
    /// that tier is not usable here, so that the caller can choose what gives way.
    pub fn decode_exact(
        &mut self,
        tier: Tier,
        samples: &[f32],
    ) -> Result<Option<Decoded>, AsrError> {
        if self.load(tier)?.is_some() {
            return Ok(None);
        }
        self.decode_loaded(tier, samples).map(Some)
    }

    /// The whole-buffer decode with a tier that is resident.
    fn decode_loaded(&mut self, used: Tier, samples: &[f32]) -> Result<Decoded, AsrError> {
        let started = Instant::now();
        let Some(recognizer) = self.residents.get(used) else {
            return Err(AsrError::NoRecognizer);
        };
        let (text, lang, cut) = recognizer.decode_noted(samples)?;
        self.note_cut(used, cut);
        if lang == LangTag::Unknown && !self.lang_unknown_reported {
            self.lang_unknown_reported = true;
            self.events.push(ResidentEvent::LangUnknown);
        }
        Ok(Decoded {
            text,
            lang,
            tier: used,
            decode_ms: millis(started),
        })
    }

    /// Says that a decode read its buffer in pieces, when it did.
    fn note_cut(&mut self, tier: Tier, cut: Option<Cut>) {
        if let Some(cut) = cut {
            self.events.push(ResidentEvent::DecodeChunked { tier, cut });
        }
    }

    /// The pieces to decode again and the speech seconds. Without the cutting
    /// detector the buffer is cut into fixed chunks, and the speech seconds
    /// are the buffer's (the pessimistic reading: it cannot manufacture suspicion).
    fn pieces(&mut self, samples: &[f32]) -> Result<(Pieces, f64), AsrError> {
        let seconds = samples.len() as f64 / f64::from(SAMPLE_RATE);
        if seconds <= MIN_BUFFER_SECONDS {
            return Ok((None, seconds));
        }
        if !self.chunked {
            match self.splitter.speech_pieces(samples) {
                Ok(found) => return Ok(found),
                Err(error) => {
                    // Only a failure about the detector's model is absorbed.
                    let Some(reason) = reason_of(&error) else {
                        return Err(error);
                    };
                    self.chunked = true;
                    self.events.push(ResidentEvent::CutIntoChunks { reason });
                }
            }
        }
        Ok((fixed_chunks(samples), seconds))
    }

    /// Decodes `samples` again one utterance at a time with the recognizer of
    /// `tier` alone and returns the retry text only when it is a recovery;
    /// otherwise `text` unchanged. The language was decided once for the
    /// buffer and a retry must not move it, so a piece goes to no other model.
    fn split_retry(
        &mut self,
        tier: Tier,
        lang: &str,
        text: &str,
        samples: &[f32],
    ) -> Result<String, AsrError> {
        let (pieces, speech_seconds) = self.pieces(samples)?;
        self.retry_pieces(tier, lang, text, pieces, speech_seconds)
    }

    /// The second half of the split retry, once the buffer is cut: the pieces
    /// are decoded again only when `text` is sparse for the speech time
    /// `speech_seconds`, and the result stands only when it is a recovery.
    fn retry_pieces(
        &mut self,
        tier: Tier,
        lang: &str,
        text: &str,
        pieces: Pieces,
        speech_seconds: f64,
    ) -> Result<String, AsrError> {
        let Some(pieces) = pieces else {
            return Ok(text.to_owned());
        };
        // The second half of the two-stage gate: the caller's check used the
        // buffer's length, this one the speech the detector actually found.
        if !looks_truncated(text, speech_seconds, speech_seconds, lang) {
            return Ok(text.to_owned());
        }
        if self.load(tier)?.is_some() {
            return Ok(text.to_owned());
        }
        let Some(recognizer) = self.residents.get(tier) else {
            return Ok(text.to_owned());
        };
        let mut cuts = Vec::new();
        let parts = pieces
            .iter()
            .map(|piece| {
                let (text, _, cut) = recognizer.decode_noted(piece)?;
                cuts.extend(cut);
                Ok(text)
            })
            .collect::<Result<Vec<String>, AsrError>>()?;
        for cut in cuts {
            self.note_cut(tier, Some(cut));
        }
        let joined = join_pieces(&parts, lang);
        if retry_is_better(text, &joined, speech_seconds, lang) {
            Ok(joined)
        } else {
            Ok(text.to_owned())
        }
    }

    /// The text of `samples`, or a recovery of it when it looks like the
    /// recognizer dropped the beginning. `tier` is the tier that made `text`.
    pub fn retry_if_truncated(
        &mut self,
        tier: Tier,
        lang: &str,
        text: &str,
        samples: &[f32],
    ) -> Result<String, AsrError> {
        let seconds = samples.len() as f64 / f64::from(SAMPLE_RATE);
        if !looks_truncated(text, seconds, seconds, lang) {
            return Ok(text.to_owned());
        }
        self.split_retry(tier, lang, text, samples)
    }

    /// Decodes `samples` in language `lang` with `tier`: the whole-buffer
    /// decode, then the split retry when the text looks cut short.
    pub fn decode(&mut self, tier: Tier, lang: &str, samples: &[f32]) -> Result<Decoded, AsrError> {
        let whole = self.decode_whole(tier, samples)?;
        // The whole decode times itself after the tier is resident, so the
        // sum is the recognizer's time and a model load is not in it.
        let retrying = Instant::now();
        let text = self.retry_if_truncated(whole.tier, lang, &whole.text, samples)?;
        Ok(Decoded {
            text,
            lang: whole.lang,
            tier: whole.tier,
            decode_ms: whole.decode_ms + millis(retrying),
        })
    }

    /// The milliseconds spent building recognizers since the last call. A
    /// caller that times a decode asks before it starts, to drop what came
    /// earlier, and after it ends, to take a load that happened inside it out
    /// of the time.
    pub fn take_load_ms(&mut self) -> f64 {
        std::mem::take(&mut self.load_ms)
    }

    /// Drops every recognizer from memory.
    pub fn release_all(&mut self) {
        self.residents.clear();
    }

    /// The tiers in memory, in tier order.
    pub fn resident(&self) -> Vec<Tier> {
        self.residents.tiers()
    }

    /// What happened since the last call, in order.
    pub fn take_events(&mut self) -> Vec<ResidentEvent> {
        std::mem::take(&mut self.events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHOLE: &str = "植物がなければ動物は生きていけません";
    const RECOVERED: &str = concat!(
        "私たちは植物で家を作り植物から衣類を作ります日々食べる食材の植物です",
        "植物がなければ動物は生きていけません"
    );

    fn parts(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| (*text).to_owned()).collect()
    }

    #[test]
    fn join_pieces_concatenates_cjk_without_spaces() {
        let pieces = parts(&["東京の天気は晴れです", "明日の会議は十時からです"]);
        assert_eq!(
            join_pieces(&pieces, "ja"),
            "東京の天気は晴れです明日の会議は十時からです"
        );
        for lang in ["zh", "yue", "ko"] {
            assert!(!join_pieces(&pieces, lang).contains(' '), "{lang}");
        }
    }

    #[test]
    fn join_pieces_space_joins_word_delimited_languages() {
        assert_eq!(
            join_pieces(&parts(&["hello there", "how are you"]), "en"),
            "hello there how are you"
        );
        assert_eq!(
            join_pieces(&parts(&["guten tag", "wie geht"]), "de"),
            "guten tag wie geht"
        );
    }

    #[test]
    fn join_pieces_drops_empty_and_whitespace_only_pieces() {
        assert_eq!(
            join_pieces(&parts(&["  ", "hi", "", "  there "]), "en"),
            "hi there"
        );
        assert_eq!(join_pieces(&[], "ja"), "");
    }

    #[test]
    fn fixed_chunks_returns_none_for_short_audio() {
        assert!(fixed_chunks(&vec![0.0; 2 * 16_000]).is_none());
        assert!(fixed_chunks(&vec![0.0; 8 * 16_000]).is_none());
    }

    #[test]
    fn fixed_chunks_cover_the_whole_buffer_with_overlap() {
        let audio: Vec<f32> = (0..30 * 16_000).map(|n| n as f32).collect();
        let chunks = fixed_chunks(&audio);
        let Some(chunks) = chunks else {
            panic!("a 30 s buffer was not cut");
        };
        assert!(chunks.len() > 1);
        // every chunk is at most one chunk long, and together they reach the end
        assert!(chunks.iter().all(|chunk| chunk.len() <= 8 * 16_000));
        let last = chunks.last().and_then(|chunk| chunk.last());
        assert_eq!(last, audio.last());
        // consecutive chunks overlap, so a word on a seam is whole in one of them
        let step = 124_000;
        assert_eq!(chunks[1][0], audio[step]);
        assert!(step < 8 * 16_000);
    }

    #[test]
    fn a_tail_shorter_than_the_least_chunk_is_dropped() {
        // Chunks start every 124_000 samples; the third would hold 3_000, under 0.2 s (3_200).
        let audio = vec![0.0_f32; 2 * 124_000 + 3_000];
        let Some(chunks) = fixed_chunks(&audio) else {
            panic!("the buffer was not cut");
        };
        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|chunk| chunk.len() > 3_200));
    }

    #[test]
    fn text_density_counts_only_alphanumerics() {
        assert_eq!(text_density("あいう", 1.0), 3.0);
        assert_eq!(text_density("あ、い。う！", 1.0), 3.0);
        assert!(text_density("anything", 0.0).is_infinite());
    }

    #[test]
    fn short_buffers_are_never_suspicious() {
        assert!(!looks_truncated("あ", 1.0, 4.0, "ja"));
        assert!(looks_truncated("あ", 5.0, 4.01, "ja"));
    }

    #[test]
    fn empty_text_is_not_suspicious() {
        assert!(!looks_truncated("", 10.0, 20.0, "ja"));
        assert!(!looks_truncated("   ", 10.0, 20.0, "ja"));
    }

    #[test]
    fn sparse_cjk_output_over_a_long_buffer_is_suspicious() {
        // 18 characters over 10.6 s of speech is 1.7 per second, against a
        // healthy population measured at 3.46 to 14.22.
        assert!(looks_truncated(WHOLE, 10.6, 18.3, "ja"));
    }

    #[test]
    fn normal_density_cjk_output_is_not_suspicious() {
        let text = "私たちは植物で家を作り植物から衣類を作ります日々食べる食材の植物です".repeat(2);
        assert!(!looks_truncated(&text, 10.6, 18.3, "ja"));
    }

    #[test]
    fn latin_languages_use_their_own_higher_floor() {
        let text = "a".repeat(30); // 3 per second over 10 s
        assert!(looks_truncated(&text, 10.0, 10.5, "en"));
        assert!(!looks_truncated(&text, 10.0, 10.5, "ja"));
    }

    #[test]
    fn the_density_floor_is_exclusive() {
        // 24 characters over 10 s is exactly 2.4 per second: healthy.
        assert!(!looks_truncated(&"あ".repeat(24), 10.0, 10.0, "ja"));
        assert!(looks_truncated(&"あ".repeat(23), 10.0, 10.0, "ja"));
    }

    #[test]
    fn the_longest_common_run_is_found() {
        let chars = |text: &str| text.chars().collect::<Vec<char>>();
        assert_eq!(
            longest_common_substring(&chars("abcdef"), &chars("xxcdeyy")),
            3
        );
        assert_eq!(longest_common_substring(&chars("abc"), &chars("xyz")), 0);
        assert_eq!(longest_common_substring(&[], &chars("xyz")), 0);
    }

    #[test]
    fn retry_accepted_when_it_prepends_to_the_surviving_tail() {
        assert!(retry_is_better(WHOLE, RECOVERED, 10.6, "ja"));
    }

    #[test]
    fn retry_rejected_when_not_longer() {
        assert!(!retry_is_better(WHOLE, WHOLE, 10.6, "ja"));
        assert!(!retry_is_better(WHOLE, "植物がなければ", 10.6, "ja"));
        assert!(!retry_is_better(WHOLE, "", 10.6, "ja"));
    }

    #[test]
    fn retry_rejected_when_it_loses_the_whole_decodes_tail() {
        // longer and dense, but a different reading of the audio
        let other = "まったくちがうぶんしょうがここにながながとつづいていくばかりです".repeat(2);
        assert!(!retry_is_better(WHOLE, &other, 10.6, "ja"));
    }

    #[test]
    fn retry_rejected_when_it_is_still_sparse() {
        // longer and tail-preserving, but the density never recovers
        let still_sparse = format!("あ{WHOLE}");
        assert!(!retry_is_better(WHOLE, &still_sparse, 10.6, "ja"));
    }

    #[test]
    fn the_tail_match_threshold_is_inclusive() {
        // The tail is 12 characters; 8 shared of 12 is 0.667, 7 of 12 is 0.583.
        let whole = "あいうえおかきくけこさし";
        assert!(retry_is_better(
            whole,
            "xxxxあいうえおかきくxxxx",
            1.0,
            "ja"
        ));
        assert!(!retry_is_better(
            whole,
            "xxxxあいうえおかきxxxxxx",
            1.0,
            "ja"
        ));
    }

    #[test]
    fn cjkchar_units_splits_cjk_chars_and_keeps_ascii_words() {
        assert_eq!(cjkchar_units("屈足湖"), ["屈", "足", "湖"]);
        assert_eq!(cjkchar_units("GANKE FES"), ["GANKE", "FES"]);
        assert_eq!(cjkchar_units("屈足 FES"), ["屈", "足", "FES"]);
    }

    const SCRATCH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/unit-tmp");

    /// Writes the hot word file and the tokens file of one case into its own directory.
    fn hotword_case(
        name: &str,
        hotwords: &str,
        tokens: &str,
    ) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
        let directory = Path::new(SCRATCH).join(format!("hotwords-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let (words, vocabulary) = (directory.join("hotwords.txt"), directory.join("tokens.txt"));
        std::fs::write(&words, hotwords)?;
        std::fs::write(&vocabulary, tokens)?;
        Ok((words, vocabulary))
    }

    #[test]
    fn check_hotwords_encodable_all_missing() -> Result<(), Box<dyn std::error::Error>> {
        let (words, tokens) = hotword_case(
            "missing",
            "屈足湖\nGANKE FES\n# comment\n\n",
            "<blk> 0\n<unk> 1\n▁ƊĢĥ 2\n",
        )?;
        assert_eq!(check_hotwords_encodable(&words, &tokens)?, (2, 2));
        Ok(())
    }

    #[test]
    fn check_hotwords_encodable_some_found() -> Result<(), Box<dyn std::error::Error>> {
        let (words, tokens) = hotword_case("some", "屈足\n屈足湖\n", "<blk> 0\n屈 1\n足 2\n")?;
        // "屈足" is fully found; "屈足湖" lacks 湖.
        assert_eq!(check_hotwords_encodable(&words, &tokens)?, (2, 1));
        Ok(())
    }

    #[test]
    fn check_hotwords_encodable_empty_path() -> Result<(), AsrError> {
        assert_eq!(
            check_hotwords_encodable(Path::new(""), Path::new("tokens.txt"))?,
            (0, 0)
        );
        Ok(())
    }

    #[test]
    fn a_token_that_holds_a_space_stays_whole() {
        let vocabulary = token_vocabulary("a b 3\nx 4\n\n");
        assert!(vocabulary.contains("a b"));
        assert!(vocabulary.contains("x"));
        assert_eq!(vocabulary.len(), 2);
    }

    fn nothing_installed() -> Recognizers {
        Recognizers::new(Path::new("no-such-models"), 1, None, DecodeCap::Off)
    }

    #[test]
    fn split_retry_keeps_the_whole_text_when_there_is_nothing_to_split() -> Result<(), AsrError> {
        // A buffer of 3 s cannot hold two utterances: no detector, no decode.
        let mut recognizers = nothing_installed();
        let kept = recognizers.retry_if_truncated(Tier::Rz, "ja", WHOLE, &vec![0.0; 3 * 16_000])?;
        assert_eq!(kept, WHOLE);
        assert!(recognizers.take_events().is_empty());
        Ok(())
    }

    #[test]
    fn split_retry_stands_down_when_the_speech_measure_clears_the_buffer_alarm()
    -> Result<(), AsrError> {
        // The buffer-length alarm: 18 characters over 30 s is sparse.
        assert!(looks_truncated(WHOLE, 30.0, 30.0, "ja"));
        let mut recognizers = nothing_installed();
        // The detector found 6 s of speech in it: the same text is dense enough,
        // so no tier is looked for and nothing is decoded.
        let pieces = Some(vec![vec![0.0; 16_000]; 2]);
        let kept = recognizers.retry_pieces(Tier::Rz, "ja", WHOLE, pieces, 6.0)?;
        assert_eq!(kept, WHOLE);
        assert!(recognizers.unavailable.is_empty());
        // The control: a speech measure that keeps the alarm up goes on to look
        // for the tier (it is not installed here, so the text still stands).
        let pieces = Some(vec![vec![0.0; 16_000]; 2]);
        let kept = recognizers.retry_pieces(Tier::Rz, "ja", WHOLE, pieces, 30.0)?;
        assert_eq!(kept, WHOLE);
        assert!(recognizers.unavailable.contains_key(&Tier::Rz));
        Ok(())
    }

    #[test]
    fn a_text_that_is_not_suspicious_is_returned_as_it_is() -> Result<(), AsrError> {
        let dense = "あ".repeat(100);
        let mut recognizers = nothing_installed();
        let kept =
            recognizers.retry_if_truncated(Tier::Rz, "ja", &dense, &vec![0.0; 10 * 16_000])?;
        assert_eq!(kept, dense);
        Ok(())
    }

    #[test]
    fn without_the_cutting_detector_a_long_buffer_is_cut_into_chunks_and_it_is_said()
    -> Result<(), AsrError> {
        let mut recognizers = nothing_installed();
        let (pieces, seconds) = recognizers.pieces(&vec![0.0; 20 * 16_000])?;
        assert_eq!(seconds, 20.0);
        assert_eq!(pieces.map(|chunks| chunks.len()), Some(3));
        assert_eq!(
            recognizers.take_events(),
            [ResidentEvent::CutIntoChunks {
                reason: Reason::Missing
            }]
        );
        // said once: the second long buffer goes straight to chunks
        recognizers.pieces(&vec![0.0; 20 * 16_000])?;
        assert!(recognizers.take_events().is_empty());
        Ok(())
    }

    #[test]
    fn the_load_time_is_handed_over_once() {
        let mut recognizers = nothing_installed();
        recognizers.load_ms = 12.5;
        assert_eq!(recognizers.take_load_ms(), 12.5);
        assert_eq!(recognizers.take_load_ms(), 0.0);
    }

    #[test]
    fn a_tier_that_is_not_installed_costs_no_load_time() {
        let mut recognizers = nothing_installed();
        assert_eq!(recognizers.load(Tier::V3).ok(), Some(Some(Reason::Missing)));
        assert_eq!(recognizers.take_load_ms(), 0.0);
    }

    #[test]
    fn a_tier_that_is_not_installed_gives_way_to_nothing_and_loads_nothing_when_decoded() {
        let mut recognizers = nothing_installed();
        let said = recognizers.decode(Tier::V3, "en", &vec![0.0; 16_000]).err();
        assert!(matches!(said, Some(AsrError::NoRecognizer)), "{said:?}");
        assert_eq!(recognizers.take_load_ms(), 0.0);
    }

    #[test]
    fn a_tier_that_is_not_installed_is_unavailable_and_nothing_gives_way_to_nothing() {
        let mut recognizers = nothing_installed();
        let said = recognizers.decode(Tier::Pz, "zh", &vec![0.0; 16_000]).err();
        assert!(matches!(said, Some(AsrError::NoRecognizer)), "{said:?}");
        assert!(recognizers.resident().is_empty());
        for tier in Tier::ALL {
            assert_eq!(
                recognizers.load(tier).ok(),
                Some(Some(Reason::Missing)),
                "{tier:?}"
            );
        }
    }
}
