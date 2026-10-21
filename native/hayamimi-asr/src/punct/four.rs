//! The four-class Japanese punctuation model, chosen with `--punct-model 4class`.
//!
//! One token classifier reads the text and says, for each token, which of
//! `、` `。` `？` `！` follows it (or none). Unlike the BERT restorer next to
//! it, there is no second pass for the question mark: the model predicts it.
//!
//! The parts that need no model come first: the labels, the decode of the
//! scores into labels (with the gates that hold back the marks the model
//! over-uses on speech), and the rebuild of the text from the labels. Then the
//! tokenizer, read from the model's `tokenizer.json` (a Unigram model with
//! byte fallback; no tokenizer crate is linked for it), and [`PunctFour`],
//! which runs the model on an `ort` session.

use std::path::{Path, PathBuf};

use ort::session::Session;
use ort::value::Tensor;

use crate::error::AsrError;
use crate::fallback::{Fallback, Reason, Stage, To};
use crate::ort_runtime::{Device, OrtRuntime};

use self::unigram::Unigram;

/// What follows a token, as the classifier head numbers it. The order is the
/// order of the head's output classes; changing it would relabel every
/// prediction without an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// No mark follows.
    None,
    /// `、`
    Comma,
    /// `。`
    Period,
    /// `？`
    Question,
    /// `！`
    Exclaim,
}

/// The labels in class order.
pub const LABELS: [Label; CLASSES] = [
    Label::None,
    Label::Comma,
    Label::Period,
    Label::Question,
    Label::Exclaim,
];

/// The classes of the head: none, and the four marks.
pub const CLASSES: usize = 5;

impl Label {
    /// The character this label inserts, or `None` for no mark.
    pub fn mark(self) -> Option<char> {
        match self {
            Label::None => None,
            Label::Comma => Some('、'),
            Label::Period => Some('。'),
            Label::Question => Some('？'),
            Label::Exclaim => Some('！'),
        }
    }
}

/// The class a gate holds back.
const COMMA_CLASS: usize = 1;
const EXCLAIM_CLASS: usize = 4;

/// The probability a mark must reach to be eligible; `None` leaves the mark
/// ungated. The model was trained on dense web prose and over-uses two marks
/// on speech: `！` fires on emphatic narration as confidently as on a real
/// exclamation, so it is held back for good by default (a value above 1 means
/// "never"), and `、` has to reach 0.8.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gates {
    pub exclaim: Option<f32>,
    pub comma: Option<f32>,
}

/// The `！` gate of a run that does not say otherwise: above 1, so never.
pub const EXCLAIM_THRESHOLD: f32 = 1.01;
/// The `、` gate of a run that does not say otherwise.
pub const COMMA_THRESHOLD: f32 = 0.8;

impl Default for Gates {
    fn default() -> Self {
        Gates {
            exclaim: Some(EXCLAIM_THRESHOLD),
            comma: Some(COMMA_THRESHOLD),
        }
    }
}

/// The softmax of one row of scores.
fn softmax(row: &[f32; CLASSES]) -> [f32; CLASSES] {
    let top = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut exps = row.map(|score| (score - top).exp());
    let sum: f32 = exps.iter().sum();
    exps.iter_mut().for_each(|e| *e /= sum);
    exps
}

/// Whether `class` may be chosen: a gated mark needs its probability to reach
/// its threshold, and every other class (`None` above all) is always eligible.
fn is_eligible(class: usize, probs: &[f32; CLASSES], gates: Gates) -> bool {
    let gate = match class {
        COMMA_CLASS => gates.comma,
        EXCLAIM_CLASS => gates.exclaim,
        _ => None,
    };
    gate.is_none_or(|threshold| probs[class] >= threshold)
}

/// The eligible class with the highest score, the first of equals. A score
/// that is not a number is never the highest. `None` is always eligible, so a
/// class always remains.
fn best_class(row: &[f32; CLASSES], gates: Gates) -> usize {
    let probs = softmax(row);
    let mut best = 0;
    for class in 1..CLASSES {
        if is_eligible(class, &probs, gates) && (row[best].is_nan() || row[class] > row[best]) {
            best = class;
        }
    }
    best
}

/// The label of each row of `logits`, a flat `[rows, 5]` array.
///
/// A gated mark is removed from the candidates before the highest is chosen,
/// so a comma that fails its gate cannot hand the position to a weak `！`.
pub fn decode(logits: &[f32], gates: Gates) -> Result<Vec<Label>, AsrError> {
    let (rows, rest) = logits.as_chunks::<CLASSES>();
    if !rest.is_empty() {
        return Err(AsrError::Shape(format!(
            "{} scores do not make rows of {CLASSES}",
            logits.len()
        )));
    }
    Ok(rows
        .iter()
        .map(|row| LABELS[best_class(row, gates)])
        .collect())
}

/// The text with the marks of `labels` put after the last character each
/// token covers. `offsets` are the `(start, end)` character positions of the
/// tokens in `text`, parallel to `labels`; an empty span is a special token
/// and is skipped. A position gets one mark: the first token that ends there
/// decides it.
pub fn reconstruct(text: &str, offsets: &[(usize, usize)], labels: &[Label]) -> String {
    let mut mark_at: Vec<Option<char>> = vec![None; text.chars().count()];
    for (&(start, end), label) in offsets.iter().zip(labels) {
        if end <= start {
            continue;
        }
        let slot = end.checked_sub(1).and_then(|last| mark_at.get_mut(last));
        if let (Some(slot), Some(mark)) = (slot, label.mark()) {
            slot.get_or_insert(mark);
        }
    }
    text.chars()
        .zip(mark_at)
        .flat_map(|(c, mark)| [Some(c), mark])
        .flatten()
        .collect()
}

/// The directory under the models directory that holds the four-class model,
/// as `hayamimi models fetch --punct-4class` lays it out.
pub const MODEL_DIR: &str = "punct-ja-4class-permissive";
/// The fp32 model.
pub const FP32_FILE: &str = "punct_4class.onnx";
/// The int8 model: the same F1 as the fp32 one within a thousandth, a quarter
/// of the size, so it is the one used when it was fetched.
pub const INT8_FILE: &str = "quantized_ort/punct_4class.int8.onnx";
/// The tokenizer the model was trained with, under the same directory.
const TOKENIZER_FILE: &str = "hf/tokenizer.json";
/// The model reads at most this many characters; the rest of a longer text
/// comes back as it is, without marks.
pub const MAX_CHARS: usize = 450;
/// Threads of the session; the recognizer shares the machine.
const THREADS: usize = 2;

/// The model file to load: the int8 one when the directory has it, else fp32.
pub fn onnx_file(directory: &Path) -> &'static str {
    if directory.join(INT8_FILE).is_file() {
        INT8_FILE
    } else {
        FP32_FILE
    }
}

/// The model file and the tokenizer file under `directory`, or the first one
/// that is not there, named by its path. They are checked before `ort` is
/// asked to open anything.
fn locate(directory: &Path, file: &str) -> Result<(PathBuf, PathBuf), AsrError> {
    let model = directory.join(file);
    let tokenizer = directory.join(TOKENIZER_FILE);
    for path in [&model, &tokenizer] {
        if !path.is_file() {
            return Err(AsrError::MissingModel(path.clone()));
        }
    }
    Ok((model, tokenizer))
}

fn ort_error(error: impl std::fmt::Display) -> AsrError {
    AsrError::Ort(error.to_string())
}

/// Builds the thing on `device`, and on the CPU when the CUDA session cannot
/// be built. The CUDA provider is registered with `error_on_failure`
/// ([`OrtRuntime::session`]), so a provider that is not there is an error here
/// and not a silent run on the CPU; the step is returned as the stage's
/// `session_failed` fallback, the record the notice of a run is made from.
/// An error that is not about the session (a missing file) is not retried.
pub fn cuda_then_cpu<T>(
    device: Device,
    mut open: impl FnMut(Device) -> Result<T, AsrError>,
) -> Result<(T, Device, Option<Fallback>), AsrError> {
    match open(device) {
        Err(AsrError::Ort(_)) if device == Device::Cuda => {
            let opened = open(Device::Cpu)?;
            let fallback = Fallback {
                stage: Stage::Punct,
                reason: Reason::SessionFailed,
                to: To::Cpu,
            };
            Ok((opened, Device::Cpu, Some(fallback)))
        }
        other => other.map(|opened| (opened, device, None)),
    }
}

/// A four-class punctuation model opened on a device, and what it fell back to.
pub struct Opened {
    pub punctuator: PunctFour,
    /// The device the session runs on.
    pub device: Device,
    /// Set when the CUDA session could not be built and the CPU one runs.
    pub fallback: Option<Fallback>,
}

/// The four-class model: a tokenizer, an `ort` session and the gates.
pub struct PunctFour {
    tokenizer: Unigram,
    session: Session,
    gates: Gates,
}

impl PunctFour {
    /// Opens the model under `models` (the file [`onnx_file`] picks) on `device`.
    pub fn open(
        runtime: &OrtRuntime,
        models: &Path,
        device: Device,
        verbose: bool,
    ) -> Result<Self, AsrError> {
        let directory = models.join(MODEL_DIR);
        Self::open_file(runtime, &directory, onnx_file(&directory), device, verbose)
    }

    /// [`PunctFour::open`] on `device`, and on the CPU when the CUDA session
    /// cannot be built.
    pub fn open_with_fallback(
        runtime: &OrtRuntime,
        models: &Path,
        device: Device,
        verbose: bool,
    ) -> Result<Opened, AsrError> {
        let (punctuator, device, fallback) = cuda_then_cpu(device, |device| {
            Self::open(runtime, models, device, verbose)
        })?;
        Ok(Opened {
            punctuator,
            device,
            fallback,
        })
    }

    /// Opens `file` (relative to `directory`) with the tokenizer of `directory`.
    pub fn open_file(
        runtime: &OrtRuntime,
        directory: &Path,
        file: &str,
        device: Device,
        verbose: bool,
    ) -> Result<Self, AsrError> {
        let (model, tokenizer) = locate(directory, file)?;
        let text = std::fs::read_to_string(&tokenizer)
            .map_err(|error| AsrError::Io(format!("{}: {error}", tokenizer.display())))?;
        let tokenizer = Unigram::from_json(&text)?;
        let session = runtime.session(&model, device, THREADS, verbose)?;
        Ok(Self {
            tokenizer,
            session,
            gates: Gates::default(),
        })
    }

    /// The same model with other gates.
    pub fn with_gates(mut self, gates: Gates) -> Self {
        self.gates = gates;
        self
    }

    /// One forward pass over `ids` with an all-ones attention mask. Returns
    /// the scores as a flat `[ids.len(), 5]` array.
    fn logits(&mut self, ids: &[i64]) -> Result<Vec<f32>, AsrError> {
        let len = ids.len();
        let input_ids = Tensor::from_array(([1_usize, len], ids.to_vec())).map_err(ort_error)?;
        let mask = Tensor::from_array(([1_usize, len], vec![1_i64; len])).map_err(ort_error)?;
        let outputs = self
            .session
            .run(ort::inputs!["input_ids" => input_ids, "attention_mask" => mask])
            .map_err(ort_error)?;
        let (shape, data) = outputs["logits"]
            .try_extract_tensor::<f32>()
            .map_err(|error| AsrError::Shape(format!("the logits are not float32 ({error})")))?;
        let rows = i64::try_from(len).map_err(|_| AsrError::Overflow("the sequence length"))?;
        let classes = i64::try_from(CLASSES).map_err(|_| AsrError::Overflow("the class count"))?;
        if shape[..] != [1, rows, classes] {
            return Err(AsrError::Shape(format!(
                "shape {shape:?}, want [1, {rows}, {classes}]"
            )));
        }
        Ok(data.to_vec())
    }

    /// Inserts `、` `。` `？` `！` into `text`. Every character of `text`
    /// comes back unchanged: only marks are added. Text that is empty after
    /// trimming comes back empty without running the model. The model is
    /// trained on NFKC-normalised text and the text is not normalised here,
    /// because that would change characters.
    pub fn restore(&mut self, text: &str) -> Result<String, AsrError> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(String::new());
        }
        let (head, tail) = split_after_chars(text, MAX_CHARS);
        let encoded = self.tokenizer.encode(head)?;
        let logits = self.logits(&encoded.ids)?;
        let labels = decode(&logits, self.gates)?;
        let mut restored = reconstruct(head, &encoded.offsets, &labels);
        restored.push_str(tail);
        Ok(restored)
    }
}

/// `text` cut after its first `chars` characters, or whole with nothing after.
fn split_after_chars(text: &str, chars: usize) -> (&str, &str) {
    match text.char_indices().nth(chars) {
        Some((at, _)) => text.split_at(at),
        None => (text, ""),
    }
}

/// A small JSON reader, for the tokenizer file only. The file is the one input
/// of this crate that is JSON, and it is read once at start-up.
mod json {
    use crate::error::AsrError;

    /// How deep arrays and objects may nest. The tokenizer file nests a few
    /// levels; a file that goes further is not one.
    const MAX_DEPTH: usize = 32;

    #[derive(Debug, Clone, PartialEq)]
    pub(super) enum Value {
        Null,
        Bool(bool),
        Number(f64),
        String(String),
        Array(Vec<Value>),
        Object(Vec<(String, Value)>),
    }

    impl Value {
        /// The member `key` of an object, the first of that name.
        pub(super) fn get(&self, key: &str) -> Option<&Value> {
            match self {
                Value::Object(members) => members
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value),
                _ => None,
            }
        }

        pub(super) fn as_array(&self) -> Option<&[Value]> {
            match self {
                Value::Array(items) => Some(items),
                _ => None,
            }
        }

        pub(super) fn as_str(&self) -> Option<&str> {
            match self {
                Value::String(text) => Some(text),
                _ => None,
            }
        }

        pub(super) fn as_f64(&self) -> Option<f64> {
            match self {
                Value::Number(number) => Some(*number),
                _ => None,
            }
        }
    }

    fn fault(what: &str, at: usize) -> AsrError {
        AsrError::Vocab(format!("tokenizer file: {what} at byte {at}"))
    }

    /// Parses one JSON value that fills the whole of `text`.
    pub(super) fn parse(text: &str) -> Result<Value, AsrError> {
        let mut reader = Reader { text, at: 0 };
        let value = reader.value(0)?;
        reader.skip_space();
        if reader.at != text.len() {
            return Err(fault("text after the value", reader.at));
        }
        Ok(value)
    }

    struct Reader<'a> {
        text: &'a str,
        at: usize,
    }

    impl Reader<'_> {
        fn peek(&self) -> Option<u8> {
            self.text.as_bytes().get(self.at).copied()
        }

        fn eat(&mut self, byte: u8) -> bool {
            let found = self.peek() == Some(byte);
            self.at += usize::from(found);
            found
        }

        /// Reads `byte`, or fails saying `what` was wanted.
        fn need(&mut self, byte: u8, what: &str) -> Result<(), AsrError> {
            if self.eat(byte) {
                Ok(())
            } else {
                Err(fault(what, self.at))
            }
        }

        fn skip_space(&mut self) {
            while self
                .peek()
                .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
            {
                self.at += 1;
            }
        }

        /// The text from `start` to the position, which sits on a character
        /// boundary because it only ever stops at an ASCII byte.
        fn since(&self, start: usize) -> Result<&str, AsrError> {
            self.text
                .get(start..self.at)
                .ok_or_else(|| fault("a cut inside a character", start))
        }

        fn word(&mut self, word: &str, value: Value) -> Result<Value, AsrError> {
            let here = self
                .text
                .get(self.at..)
                .is_some_and(|rest| rest.starts_with(word));
            if !here {
                return Err(fault("an unknown word", self.at));
            }
            self.at += word.len();
            Ok(value)
        }

        fn value(&mut self, depth: usize) -> Result<Value, AsrError> {
            if depth > MAX_DEPTH {
                return Err(fault("nesting too deep", self.at));
            }
            self.skip_space();
            match self.peek() {
                Some(b'{') => self.object(depth),
                Some(b'[') => self.array(depth),
                Some(b'"') => self.string().map(Value::String),
                Some(b't') => self.word("true", Value::Bool(true)),
                Some(b'f') => self.word("false", Value::Bool(false)),
                Some(b'n') => self.word("null", Value::Null),
                Some(_) => self.number(),
                None => Err(fault("the text ends inside a value", self.at)),
            }
        }

        fn array(&mut self, depth: usize) -> Result<Value, AsrError> {
            self.at += 1;
            let mut items = Vec::new();
            self.skip_space();
            if self.eat(b']') {
                return Ok(Value::Array(items));
            }
            loop {
                items.push(self.value(depth + 1)?);
                self.skip_space();
                if !self.eat(b',') {
                    self.need(b']', "wanted , or ]")?;
                    return Ok(Value::Array(items));
                }
            }
        }

        fn object(&mut self, depth: usize) -> Result<Value, AsrError> {
            self.at += 1;
            let mut members = Vec::new();
            self.skip_space();
            if self.eat(b'}') {
                return Ok(Value::Object(members));
            }
            loop {
                self.skip_space();
                if self.peek() != Some(b'"') {
                    return Err(fault("wanted a member name", self.at));
                }
                let name = self.string()?;
                self.skip_space();
                self.need(b':', "wanted :")?;
                members.push((name, self.value(depth + 1)?));
                self.skip_space();
                if !self.eat(b',') {
                    self.need(b'}', "wanted , or }")?;
                    return Ok(Value::Object(members));
                }
            }
        }

        fn number(&mut self) -> Result<Value, AsrError> {
            let start = self.at;
            while self
                .peek()
                .is_some_and(|byte| matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
            {
                self.at += 1;
            }
            self.since(start)?
                .parse::<f64>()
                .map(Value::Number)
                .map_err(|_| fault("not a number", start))
        }

        /// Reads the string whose opening quote is at the position.
        fn string(&mut self) -> Result<String, AsrError> {
            self.at += 1;
            let mut out = String::new();
            loop {
                let start = self.at;
                while self
                    .peek()
                    .is_some_and(|byte| byte != b'"' && byte != b'\\')
                {
                    self.at += 1;
                }
                out.push_str(self.since(start)?);
                match self.peek() {
                    Some(b'"') => {
                        self.at += 1;
                        return Ok(out);
                    }
                    Some(_) => {
                        self.at += 1;
                        out.push(self.escape()?);
                    }
                    None => return Err(fault("a string is not closed", self.at)),
                }
            }
        }

        /// The character an escape stands for; the backslash is already read.
        fn escape(&mut self) -> Result<char, AsrError> {
            let Some(code) = self.peek() else {
                return Err(fault("an escape ends the text", self.at));
            };
            self.at += 1;
            match code {
                b'"' => Ok('"'),
                b'\\' => Ok('\\'),
                b'/' => Ok('/'),
                b'b' => Ok('\u{8}'),
                b'f' => Ok('\u{c}'),
                b'n' => Ok('\n'),
                b'r' => Ok('\r'),
                b't' => Ok('\t'),
                b'u' => self.unicode(),
                _ => Err(fault("an unknown escape", self.at)),
            }
        }

        fn hex4(&mut self) -> Result<u32, AsrError> {
            let digits = self
                .text
                .get(self.at..self.at + 4)
                .filter(|digits| digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .ok_or_else(|| fault("wanted four hex digits", self.at))?;
            self.at += 4;
            u32::from_str_radix(digits, 16).map_err(|_| fault("wanted four hex digits", self.at))
        }

        /// The character of `\uXXXX`, joined with the `\uXXXX` after it when it
        /// is the first half of a surrogate pair.
        fn unicode(&mut self) -> Result<char, AsrError> {
            let first = self.hex4()?;
            let code = if (0xD800..0xDC00).contains(&first) {
                if !(self.eat(b'\\') && self.eat(b'u')) {
                    return Err(fault("a lone surrogate", self.at));
                }
                let second = self.hex4()?;
                if !(0xDC00..0xE000).contains(&second) {
                    return Err(fault("a lone surrogate", self.at));
                }
                0x1_0000 + ((first - 0xD800) << 10) + (second - 0xDC00)
            } else {
                first
            };
            char::from_u32(code).ok_or_else(|| fault("not a character", self.at))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn nested_values_are_read() -> Result<(), AsrError> {
            let text =
                r#" {"a": [1, -2.5, 3e2], "b": {"c": true, "d": null, "e": false}, "f": "x"} "#;
            let value = parse(text)?;
            assert_eq!(value.get("f").and_then(Value::as_str), Some("x"));
            let numbers = [
                Value::Number(1.0),
                Value::Number(-2.5),
                Value::Number(300.0),
            ];
            assert_eq!(value.get("a").and_then(Value::as_array), Some(&numbers[..]));
            let inner = |key: &str| value.get("b").and_then(|b| b.get(key));
            assert_eq!(inner("c"), Some(&Value::Bool(true)));
            assert_eq!(inner("d"), Some(&Value::Null));
            assert_eq!(inner("e"), Some(&Value::Bool(false)));
            assert_eq!(value.get("missing"), None);
            Ok(())
        }

        #[test]
        fn an_empty_array_and_an_empty_object_are_read() -> Result<(), AsrError> {
            assert_eq!(parse("[]")?, Value::Array(Vec::new()));
            assert_eq!(parse(" { } ")?, Value::Object(Vec::new()));
            Ok(())
        }

        #[test]
        fn escapes_and_a_surrogate_pair_are_read() -> Result<(), AsrError> {
            let value = parse(r#""a\"b\\c\/d\n\tあ😀""#)?;
            assert_eq!(value.as_str(), Some("a\"b\\c/d\n\tあ😀"));
            Ok(())
        }

        #[test]
        fn text_that_is_not_one_json_value_is_refused() {
            let nested = format!("{}{}", "[".repeat(40), "]".repeat(40));
            let bad = [
                "",
                "[1,",
                "[1 2]",
                r#"{"a" 1}"#,
                "{1: 2}",
                r#""abc"#,
                "tru",
                "[1] x",
                r#""\q""#,
                r#""\ud800""#,
                r#""\ud800A""#,
                r#""\u12G4""#,
                "-",
                nested.as_str(),
            ];
            for text in bad {
                assert!(
                    matches!(parse(text), Err(AsrError::Vocab(_))),
                    "{text:?} was read"
                );
            }
        }

        #[test]
        fn nesting_up_to_the_limit_is_read() {
            let nested = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
            assert!(parse(&nested).is_ok());
        }
    }
}

/// The Unigram tokenizer of the model, as `tokenizer.json` describes it: the
/// pre-tokenizer puts `▁` in place of each space, the model cuts the text into
/// the pieces whose scores add up highest, a character that no piece covers
/// becomes its bytes (byte fallback), and the begin and end markers are added.
mod unigram {
    use std::collections::HashMap;

    use super::json::{self, Value};
    use crate::error::AsrError;

    /// How far below the lowest piece score a character the vocabulary lacks is scored.
    const UNKNOWN_PENALTY: f64 = 10.0;
    /// What the pre-tokenizer puts in place of a space.
    const SPACE_MARK: char = '▁';
    /// The markers the post-processor puts around the text.
    const BEGIN: &str = "<s>";
    const END: &str = "</s>";
    /// The most tokens of an encoding, markers included (`truncation.max_length` of the file).
    const MAX_TOKENS: usize = 320;
    /// The most tokens of the text itself.
    const MAX_CONTENT: usize = MAX_TOKENS - 2;

    #[derive(Debug)]
    struct Piece {
        id: i64,
        score: f64,
    }

    /// The ids the model reads, and the character span of each in the text.
    /// The two markers span `(0, 0)`.
    #[derive(Debug, PartialEq, Eq)]
    pub(super) struct Encoded {
        pub(super) ids: Vec<i64>,
        pub(super) offsets: Vec<(usize, usize)>,
    }

    #[derive(Debug)]
    pub(super) struct Unigram {
        pieces: HashMap<String, Piece>,
        /// The longest piece, in characters.
        longest: usize,
        unknown_score: f64,
        /// The id of the unknown token, if the file names one.
        unk: Option<i64>,
        /// The id of the token `<0xHH>` of each byte; `None` when byte fallback is off.
        byte_ids: Option<[Option<i64>; 256]>,
        begin: i64,
        end: i64,
    }

    /// The text with its spaces marked, and where each character starts.
    struct Spaced {
        text: String,
        /// `bounds[i]` is the byte offset of character `i`; the last is the length.
        bounds: Vec<usize>,
    }

    impl Spaced {
        fn new(text: &str) -> Spaced {
            let text: String = text
                .chars()
                .map(|c| if c == ' ' { SPACE_MARK } else { c })
                .collect();
            let mut bounds: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
            bounds.push(text.len());
            Spaced { text, bounds }
        }

        /// The number of characters.
        fn len(&self) -> usize {
            self.bounds.len() - 1
        }

        /// Characters `from..to`.
        fn piece(&self, from: usize, to: usize) -> Option<&str> {
            self.text
                .get(*self.bounds.get(from)?..*self.bounds.get(to)?)
        }
    }

    /// The best way to reach a position: the score, where the last piece
    /// starts, and which piece it is (`None` for a character no piece covers).
    #[derive(Debug, Clone, Copy)]
    struct Step {
        score: f64,
        from: usize,
        piece: Option<i64>,
    }

    /// One piece of the best path: the characters `from..to`.
    #[derive(Debug, Clone, Copy)]
    struct Span {
        from: usize,
        to: usize,
        piece: Option<i64>,
    }

    /// One token of the text: an id and the characters `from..to` it covers.
    #[derive(Debug)]
    struct Token {
        id: i64,
        from: usize,
        to: usize,
    }

    fn missing(what: &str) -> AsrError {
        AsrError::Vocab(format!("tokenizer file: no {what}"))
    }

    /// An id in the file: a whole number from zero.
    fn as_id(value: &Value) -> Option<i64> {
        let number = value.as_f64()?;
        let whole = number.fract() == 0.0 && (0.0..=f64::from(i32::MAX)).contains(&number);
        // The range check above keeps the cast exact.
        whole.then_some(number as i64)
    }

    /// The pieces of `model.vocab` in id order, with the longest in characters
    /// and the lowest score.
    fn read_pieces(entries: &[Value]) -> Result<(HashMap<String, Piece>, usize, f64), AsrError> {
        let mut pieces = HashMap::with_capacity(entries.len());
        let (mut longest, mut lowest) = (0, f64::INFINITY);
        for (index, entry) in entries.iter().enumerate() {
            let id = i64::try_from(index).map_err(|_| AsrError::Overflow("a vocab index"))?;
            let pair = entry.as_array().filter(|pair| pair.len() == 2);
            let (Some(text), Some(score)) = (
                pair.and_then(|pair| pair[0].as_str()),
                pair.and_then(|pair| pair[1].as_f64()),
            ) else {
                return Err(AsrError::Vocab(format!(
                    "tokenizer file: vocab entry {index} is not [piece, score]"
                )));
            };
            longest = longest.max(text.chars().count());
            lowest = lowest.min(score);
            pieces.insert(text.to_owned(), Piece { id, score });
        }
        Ok((pieces, longest, lowest))
    }

    /// Whether the model falls back to bytes. The format leaves the key out
    /// when it is off.
    fn byte_fallback(model: &Value) -> Result<bool, AsrError> {
        match model.get("byte_fallback") {
            Some(Value::Bool(on)) => Ok(*on),
            // DEFAULT: tokenizer.json omits byte_fallback when it is off; the format fixes false for a missing key.
            None => Ok(false),
            Some(_) => Err(AsrError::Vocab(String::from(
                "tokenizer file: byte_fallback is not true or false",
            ))),
        }
    }

    fn id_of(pieces: &HashMap<String, Piece>, name: &str) -> Result<i64, AsrError> {
        pieces
            .get(name)
            .map(|piece| piece.id)
            .ok_or_else(|| missing(&format!("{name} piece")))
    }

    /// Keeps the way to `to` through `from` when it scores strictly higher.
    fn relax(best: &mut [Step], from: usize, to: usize, score: f64, piece: Option<i64>) {
        if score > best[to].score {
            best[to] = Step { score, from, piece };
        }
    }

    /// The pieces of the best path, first to last.
    fn backtrack(best: &[Step]) -> Vec<Span> {
        let mut spans = Vec::new();
        let mut to = best.len() - 1;
        while to > 0 {
            let step = best[to];
            spans.push(Span {
                from: step.from,
                to,
                piece: step.piece,
            });
            to = step.from;
        }
        spans.reverse();
        spans
    }

    impl Unigram {
        /// Reads `tokenizer.json`. The model must be a Unigram one that has
        /// the begin and end markers.
        pub(super) fn from_json(text: &str) -> Result<Unigram, AsrError> {
            let root = json::parse(text)?;
            let model = root.get("model").ok_or_else(|| missing("model"))?;
            if model.get("type").and_then(Value::as_str) != Some("Unigram") {
                return Err(AsrError::Vocab(String::from(
                    "tokenizer file: the model is not a Unigram model",
                )));
            }
            let entries = model
                .get("vocab")
                .and_then(Value::as_array)
                .ok_or_else(|| missing("model.vocab"))?;
            let (pieces, longest, lowest) = read_pieces(entries)?;
            let byte_ids = byte_fallback(model)?.then(|| {
                std::array::from_fn(|byte| {
                    pieces.get(&format!("<0x{byte:02X}>")).map(|piece| piece.id)
                })
            });
            Ok(Unigram {
                begin: id_of(&pieces, BEGIN)?,
                end: id_of(&pieces, END)?,
                unk: model.get("unk_id").and_then(as_id),
                unknown_score: lowest - UNKNOWN_PENALTY,
                longest,
                byte_ids,
                pieces,
            })
        }

        /// The ids and character spans of `text`, with the markers around
        /// them, cut at the most tokens the model reads.
        pub(super) fn encode(&self, text: &str) -> Result<Encoded, AsrError> {
            let spaced = Spaced::new(text);
            let spans = backtrack(&self.best_path(&spaced));
            let mut tokens = self.tokens(&spaced, &spans)?;
            tokens.truncate(MAX_CONTENT);
            Ok(self.with_markers(&tokens))
        }

        /// The best way to reach each position, by the Viterbi search over
        /// the pieces. Of two ways with the same score the one whose last
        /// piece is longer is kept.
        fn best_path(&self, spaced: &Spaced) -> Vec<Step> {
            let start = Step {
                score: f64::NEG_INFINITY,
                from: 0,
                piece: None,
            };
            let mut best = vec![start; spaced.len() + 1];
            best[0].score = 0.0;
            for from in 0..spaced.len() {
                self.extend(spaced, &mut best, from);
            }
            best
        }

        /// Adds every piece that starts at `from` to the search, and the
        /// character itself as unknown when no piece is just that character.
        fn extend(&self, spaced: &Spaced, best: &mut [Step], from: usize) {
            let base = best[from].score;
            let mut single = false;
            for to in from + 1..=(from + self.longest).min(spaced.len()) {
                let Some(piece) = spaced.piece(from, to).and_then(|key| self.pieces.get(key))
                else {
                    continue;
                };
                single |= to == from + 1;
                relax(best, from, to, base + piece.score, Some(piece.id));
            }
            if !single {
                relax(best, from, from + 1, base + self.unknown_score, None);
            }
        }

        /// The tokens of the pieces of the best path. A run of characters no
        /// piece covers is one span, as the reference tokenizer fuses it.
        fn tokens(&self, spaced: &Spaced, spans: &[Span]) -> Result<Vec<Token>, AsrError> {
            let mut tokens = Vec::with_capacity(spans.len());
            let mut rest = spans;
            while let Some(first) = rest.first() {
                if let Some(id) = first.piece {
                    tokens.push(Token {
                        id,
                        from: first.from,
                        to: first.to,
                    });
                    rest = &rest[1..];
                    continue;
                }
                let run = rest.iter().take_while(|span| span.piece.is_none()).count();
                let to = rest[run - 1].to;
                self.unknown(spaced, first.from, to, &mut tokens)?;
                rest = &rest[run..];
            }
            Ok(tokens)
        }

        /// The tokens of characters `from..to`, which no piece covers: one per
        /// byte when every byte has its token, else the unknown token. All of
        /// them span the same characters.
        fn unknown(
            &self,
            spaced: &Spaced,
            from: usize,
            to: usize,
            tokens: &mut Vec<Token>,
        ) -> Result<(), AsrError> {
            let text = spaced.piece(from, to).ok_or_else(|| missing("text span"))?;
            let bytes: Option<Vec<i64>> = self
                .byte_ids
                .as_ref()
                .and_then(|table| text.bytes().map(|byte| table[usize::from(byte)]).collect());
            let ids = match (bytes, self.unk) {
                (Some(bytes), _) => bytes,
                (None, Some(unk)) => vec![unk],
                (None, None) => {
                    return Err(AsrError::Vocab(format!(
                        "tokenizer file: no unknown token for {text:?}"
                    )));
                }
            };
            tokens.extend(ids.into_iter().map(|id| Token { id, from, to }));
            Ok(())
        }

        fn with_markers(&self, tokens: &[Token]) -> Encoded {
            let mut ids = Vec::with_capacity(tokens.len() + 2);
            let mut offsets = Vec::with_capacity(tokens.len() + 2);
            ids.push(self.begin);
            offsets.push((0, 0));
            for token in tokens {
                ids.push(token.id);
                offsets.push((token.from, token.to));
            }
            ids.push(self.end);
            offsets.push((0, 0));
            Encoded { ids, offsets }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Ids: `<unk>` 0, `<s>` 1, `</s>` 2, `▁` 3, あ 4, い 5, あい 6, う 7,
        /// いう 8, then the byte tokens of 猫 (E7 8C AB) at 9 to 11 and the
        /// two bytes of 犬 (E7 8A AC) that 猫 lacks at 12 and 13.
        const MINI: &str = r#"{"model": {"type": "Unigram", "unk_id": 0, "byte_fallback": true, "vocab": [
            ["<unk>", 0.0], ["<s>", 0.0], ["</s>", 0.0], ["▁", -1.0],
            ["あ", -2.0], ["い", -2.0], ["あい", -3.0], ["う", -2.0], ["いう", -3.0],
            ["<0xE7>", 0.0], ["<0x8C>", 0.0], ["<0xAB>", 0.0], ["<0x8A>", 0.0], ["<0xAC>", 0.0]]}}"#;

        fn mini() -> Result<Unigram, AsrError> {
            Unigram::from_json(MINI)
        }

        fn encoded(ids: &[i64], offsets: &[(usize, usize)]) -> Encoded {
            Encoded {
                ids: ids.to_vec(),
                offsets: offsets.to_vec(),
            }
        }

        #[test]
        fn the_higher_scoring_cut_wins() -> Result<(), AsrError> {
            // あい (-3) beats あ + い (-4).
            let got = mini()?.encode("あい")?;
            assert_eq!(got, encoded(&[1, 6, 2], &[(0, 0), (0, 2), (0, 0)]));
            Ok(())
        }

        #[test]
        fn of_two_cuts_with_one_score_the_longer_last_piece_wins() -> Result<(), AsrError> {
            // あい + う and あ + いう both score -5; the cut that ends in the longer piece stays.
            let got = mini()?.encode("あいう")?;
            assert_eq!(
                got,
                encoded(&[1, 4, 8, 2], &[(0, 0), (0, 1), (1, 3), (0, 0)])
            );
            Ok(())
        }

        #[test]
        fn a_space_is_the_mark_and_spans_count_characters() -> Result<(), AsrError> {
            let got = mini()?.encode("あ い")?;
            let spans = [(0, 0), (0, 1), (1, 2), (2, 3), (0, 0)];
            assert_eq!(got, encoded(&[1, 4, 3, 5, 2], &spans));
            Ok(())
        }

        #[test]
        fn empty_text_is_only_the_two_markers() -> Result<(), AsrError> {
            assert_eq!(mini()?.encode("")?, encoded(&[1, 2], &[(0, 0), (0, 0)]));
            Ok(())
        }

        #[test]
        fn a_character_no_piece_covers_becomes_its_bytes_over_its_span() -> Result<(), AsrError> {
            let got = mini()?.encode("あ猫")?;
            let spans = [(0, 0), (0, 1), (1, 2), (1, 2), (1, 2), (0, 0)];
            assert_eq!(got, encoded(&[1, 4, 9, 10, 11, 2], &spans));
            Ok(())
        }

        #[test]
        fn a_run_of_such_characters_is_one_span() -> Result<(), AsrError> {
            let got = mini()?.encode("猫犬")?;
            assert_eq!(got.ids, [1, 9, 10, 11, 9, 12, 13, 2]);
            assert!(got.offsets[1..7].iter().all(|span| *span == (0, 2)));
            Ok(())
        }

        #[test]
        fn a_character_whose_bytes_lack_tokens_is_the_unknown_token() -> Result<(), AsrError> {
            let got = mini()?.encode("虎")?;
            assert_eq!(got, encoded(&[1, 0, 2], &[(0, 0), (0, 1), (0, 0)]));
            Ok(())
        }

        #[test]
        fn without_byte_fallback_it_is_the_unknown_token() -> Result<(), AsrError> {
            let off = MINI.replace(r#""byte_fallback": true, "#, "");
            let got = Unigram::from_json(&off)?.encode("猫")?;
            assert_eq!(got, encoded(&[1, 0, 2], &[(0, 0), (0, 1), (0, 0)]));
            Ok(())
        }

        #[test]
        fn an_unknown_character_with_no_unknown_token_is_an_error() -> Result<(), AsrError> {
            let none = MINI
                .replace(r#""unk_id": 0, "#, r#""unk_id": null, "#)
                .replace(r#""byte_fallback": true, "#, "");
            let said = Unigram::from_json(&none)?
                .encode("猫")
                .err()
                .map(|error| error.to_string());
            assert!(
                said.as_deref()
                    .is_some_and(|text| text.contains("no unknown token")),
                "{said:?}"
            );
            Ok(())
        }

        #[test]
        fn the_text_is_cut_at_the_most_tokens_the_model_reads() -> Result<(), AsrError> {
            let got = mini()?.encode(&"あ".repeat(400))?;
            assert_eq!(got.ids.len(), MAX_TOKENS);
            assert_eq!(got.ids.first(), Some(&1));
            assert_eq!(got.ids.last(), Some(&2));
            let last_token = got.offsets.len() - 2;
            assert_eq!(
                got.offsets.get(last_token),
                Some(&(MAX_CONTENT - 1, MAX_CONTENT))
            );
            assert_eq!(got.offsets.last(), Some(&(0, 0)));
            Ok(())
        }

        #[test]
        fn exactly_the_most_tokens_is_not_cut() -> Result<(), AsrError> {
            let got = mini()?.encode(&"あ".repeat(MAX_CONTENT))?;
            assert_eq!(got.ids.len(), MAX_TOKENS);
            assert_eq!(
                got.offsets.get(MAX_CONTENT),
                Some(&(MAX_CONTENT - 1, MAX_CONTENT))
            );
            Ok(())
        }

        /// The best score by trying every cut, for the search to be judged against.
        fn exhaustive(unigram: &Unigram, spaced: &Spaced, from: usize) -> f64 {
            if from == spaced.len() {
                return 0.0;
            }
            let mut best = f64::NEG_INFINITY;
            let mut single = false;
            for to in from + 1..=(from + unigram.longest).min(spaced.len()) {
                if let Some(piece) = spaced
                    .piece(from, to)
                    .and_then(|key| unigram.pieces.get(key))
                {
                    single |= to == from + 1;
                    best = best.max(piece.score + exhaustive(unigram, spaced, to));
                }
            }
            if !single {
                best = best.max(unigram.unknown_score + exhaustive(unigram, spaced, from + 1));
            }
            best
        }

        #[test]
        fn the_search_finds_the_best_score_of_all_cuts() -> Result<(), AsrError> {
            let unigram = mini()?;
            for text in [
                "あいう",
                "いういあ",
                "あ い う",
                "猫あいう犬",
                "うう虎いあい",
                "あいあいあいう",
            ] {
                let spaced = Spaced::new(text);
                let found = unigram.best_path(&spaced)[spaced.len()].score;
                let want = exhaustive(&unigram, &spaced, 0);
                assert!((found - want).abs() < 1e-9, "{text}: {found} vs {want}");
            }
            Ok(())
        }

        #[test]
        fn a_file_that_is_not_the_expected_tokenizer_is_refused() {
            let broken = [
                MINI.replace("Unigram", "BPE"),
                MINI.replace(r#"["</s>", 0.0], "#, ""),
                MINI.replace(r#"["あ", -2.0]"#, r#"["あ"]"#),
                MINI.replace(r#""byte_fallback": true"#, r#""byte_fallback": 1"#),
                String::from(r#"{"model": {"type": "Unigram"}}"#),
                String::from("{}"),
            ];
            for text in &broken {
                assert!(
                    matches!(Unigram::from_json(text), Err(AsrError::Vocab(_))),
                    "{text}"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const O: Label = Label::None;
    const COMMA: Label = Label::Comma;
    const PERIOD: Label = Label::Period;
    const Q: Label = Label::Question;
    const EXCLAIM: Label = Label::Exclaim;

    #[test]
    fn reconstruct_inserts_period_and_comma_after_token_end() {
        let text = "きょうはいいてんきです";
        // Three tokens: きょうは (0, 4), いいてんきです (4, 11), and the end marker (0, 0).
        let offsets = [(0, 4), (4, 11), (0, 0)];
        let labels = [COMMA, PERIOD, O];
        assert_eq!(
            reconstruct(text, &offsets, &labels),
            "きょうは、いいてんきです。"
        );
    }

    #[test]
    fn reconstruct_inserts_question_mark() {
        assert_eq!(
            reconstruct("だいじょうぶですか", &[(0, 9)], &[Q]),
            "だいじょうぶですか？"
        );
    }

    #[test]
    fn reconstruct_inserts_exclamation_mark() {
        assert_eq!(reconstruct("すごい", &[(0, 3)], &[EXCLAIM]), "すごい！");
    }

    #[test]
    fn reconstruct_no_mark_is_noop() {
        let text = "なにもおきません";
        let length = text.chars().count();
        assert_eq!(reconstruct(text, &[(0, length)], &[O]), text);
    }

    #[test]
    fn reconstruct_ignores_special_token_offsets() {
        // The begin and end markers report (0, 0): they are not "a mark before
        // the first character", whatever label they carry.
        let offsets = [(0, 0), (0, 3), (0, 0)];
        let labels = [PERIOD, O, PERIOD];
        assert_eq!(reconstruct("abc", &offsets, &labels), "abc");
    }

    #[test]
    fn reconstruct_only_first_mark_per_position_wins() {
        // Two tokens end at the same character. The earlier one is kept and
        // the later one never adds a second mark.
        let out = reconstruct("ab", &[(0, 2), (0, 2)], &[PERIOD, Q]);
        assert_eq!(out, "ab。");
    }

    #[test]
    fn reconstruct_multi_sentence_marks_mid_and_end() {
        let text = "きょうはあめですあしたははれるでしょうか";
        let length = text.chars().count();
        let offsets = [(0, 8), (8, length)];
        assert_eq!(
            reconstruct(text, &offsets, &[PERIOD, Q]),
            "きょうはあめです。あしたははれるでしょうか？"
        );
    }

    #[test]
    fn a_span_past_the_end_of_the_text_adds_no_mark() {
        assert_eq!(reconstruct("ab", &[(0, 5)], &[PERIOD]), "ab");
        assert_eq!(reconstruct("", &[(0, 1)], &[PERIOD]), "");
    }

    #[test]
    fn labels_longer_than_the_offsets_are_not_read() {
        assert_eq!(reconstruct("ab", &[(0, 2)], &[PERIOD, Q, COMMA]), "ab。");
    }

    #[test]
    fn punct4_labels_order() {
        // The head's classes are numbered in this order; a different order
        // would relabel every prediction.
        let marks = LABELS.map(Label::mark);
        assert_eq!(
            marks,
            [None, Some('、'), Some('。'), Some('？'), Some('！')]
        );
        assert_eq!(COMMA_CLASS, 1);
        assert_eq!(LABELS[COMMA_CLASS], Label::Comma);
        assert_eq!(LABELS[EXCLAIM_CLASS], Label::Exclaim);
    }

    /// Scores whose softmax is `probs`: the classes are `O 、 。 ？ ！`.
    fn log_probs(probs: [f32; CLASSES]) -> Vec<f32> {
        probs.iter().map(|p| (p + 1e-9).ln()).collect()
    }

    fn gated(exclaim: f32, comma: f32) -> Gates {
        Gates {
            exclaim: Some(exclaim),
            comma: Some(comma),
        }
    }

    const UNGATED: Gates = Gates {
        exclaim: None,
        comma: None,
    };

    #[test]
    fn decode_gates_remove_weak_marks_before_argmax() -> Result<(), AsrError> {
        // A comma at 0.52 fails a 0.8 gate and `！` at 0.45 is second: the
        // position must fall to 。 (or none), never to `！`.
        let weak = log_probs([0.004, 0.523, 0.026, 0.0001, 0.448]);
        assert_eq!(decode(&weak, gated(1.01, 0.8))?, [PERIOD]);
        // Gates off: plain argmax.
        assert_eq!(decode(&weak, UNGATED)?, [COMMA]);
        // A confident `！` passes its gate.
        let sure = log_probs([0.005, 0.005, 0.02, 0.0, 0.97]);
        assert_eq!(decode(&sure, gated(0.9, 0.8))?, [EXCLAIM]);
        assert_eq!(decode(&sure, gated(0.99, 0.8))?, [PERIOD]);
        // None is never gated, so a row where every mark is weak yields none.
        let faint = log_probs([0.4, 0.3, 0.1, 0.05, 0.15]);
        assert_eq!(decode(&faint, gated(0.5, 0.5))?, [O]);
        Ok(())
    }

    #[test]
    fn the_default_gates_never_emit_an_exclamation_and_hold_the_comma_at_eight_tenths()
    -> Result<(), AsrError> {
        let gates = Gates::default();
        assert_eq!(gates.exclaim, Some(1.01));
        assert_eq!(gates.comma, Some(0.8));
        let sure = log_probs([0.005, 0.005, 0.02, 0.0, 0.97]);
        assert_eq!(decode(&sure, gates)?, [PERIOD]);
        Ok(())
    }

    #[test]
    fn decode_reads_one_label_per_row() -> Result<(), AsrError> {
        let mut logits = log_probs([0.9, 0.025, 0.025, 0.025, 0.025]);
        logits.extend(log_probs([0.01, 0.01, 0.96, 0.01, 0.01]));
        logits.extend(log_probs([0.01, 0.01, 0.01, 0.96, 0.01]));
        assert_eq!(decode(&logits, UNGATED)?, [O, PERIOD, Q]);
        assert_eq!(decode(&[], UNGATED)?, []);
        Ok(())
    }

    #[test]
    fn a_row_of_the_wrong_width_is_a_shape_error() {
        let said = decode(&[0.0; 7], UNGATED).err().map(|e| e.to_string());
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains("rows of 5")),
            "{said:?}"
        );
    }

    #[test]
    fn a_score_that_is_not_a_number_is_never_chosen() -> Result<(), AsrError> {
        let logits = [f32::NAN, 1.0, 0.0, 0.0, 0.0];
        assert_eq!(decode(&logits, UNGATED)?, [COMMA]);
        Ok(())
    }

    // ---- the model files, the CUDA step and the text limit ----

    /// A fresh directory under the workspace's `target`, never `/tmp`.
    fn scratch(name: &str) -> Result<PathBuf, std::io::Error> {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/unit-tmp")
            .join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        Ok(directory)
    }

    #[test]
    fn missing_model_dir_raises_file_not_found() {
        let missing = Path::new("no-such-models").join(MODEL_DIR);
        let said = locate(&missing, FP32_FILE).err();
        assert!(
            matches!(&said, Some(AsrError::MissingModel(path)) if path.ends_with(FP32_FILE)),
            "{said:?}"
        );
    }

    #[test]
    fn a_model_without_its_tokenizer_names_the_tokenizer() -> Result<(), std::io::Error> {
        let directory = scratch("punct4-no-tokenizer")?;
        std::fs::write(directory.join(FP32_FILE), b"x")?;
        let said = locate(&directory, FP32_FILE).err();
        std::fs::remove_dir_all(&directory)?;
        assert!(
            matches!(&said, Some(AsrError::MissingModel(path)) if path.ends_with("tokenizer.json")),
            "{said:?}"
        );
        Ok(())
    }

    #[test]
    fn punct4_prefers_int8_when_present() -> Result<(), std::io::Error> {
        let directory = scratch("punct4-int8")?;
        std::fs::write(directory.join(FP32_FILE), b"x")?;
        assert_eq!(onnx_file(&directory), FP32_FILE);
        std::fs::create_dir_all(directory.join("quantized_ort"))?;
        std::fs::write(directory.join(INT8_FILE), b"x")?;
        assert_eq!(onnx_file(&directory), INT8_FILE);
        std::fs::remove_dir_all(&directory)
    }

    #[test]
    fn a_cuda_session_that_cannot_be_built_runs_on_the_cpu_and_says_so() -> Result<(), AsrError> {
        let (opened, device, fallback) = cuda_then_cpu(Device::Cuda, |device| match device {
            Device::Cuda => Err(AsrError::Ort(String::from("no CUDA provider"))),
            Device::Cpu => Ok("cpu session"),
        })?;
        assert_eq!((opened, device), ("cpu session", Device::Cpu));
        let wanted = Fallback {
            stage: Stage::Punct,
            reason: Reason::SessionFailed,
            to: To::Cpu,
        };
        assert_eq!(fallback, Some(wanted));
        Ok(())
    }

    #[test]
    fn a_cuda_session_that_is_built_needs_no_fallback() -> Result<(), AsrError> {
        let (_, device, fallback) = cuda_then_cpu(Device::Cuda, |_| Ok(()))?;
        assert_eq!((device, fallback), (Device::Cuda, None));
        Ok(())
    }

    #[test]
    fn a_missing_model_is_not_retried_on_the_cpu() {
        let calls = Cell::new(0);
        let said = cuda_then_cpu(Device::Cuda, |_| {
            calls.set(calls.get() + 1);
            Err::<(), _>(AsrError::MissingModel(PathBuf::from("m.onnx")))
        })
        .err();
        assert!(matches!(said, Some(AsrError::MissingModel(_))), "{said:?}");
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn a_cpu_session_that_fails_after_the_fallback_is_the_error() {
        let calls = Cell::new(0);
        let said = cuda_then_cpu(Device::Cuda, |_| {
            calls.set(calls.get() + 1);
            Err::<(), _>(AsrError::Ort(String::from("broken")))
        })
        .err();
        assert!(matches!(said, Some(AsrError::Ort(_))), "{said:?}");
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn the_cpu_asked_for_is_tried_once_and_is_no_fallback() {
        let calls = Cell::new(0);
        let said = cuda_then_cpu(Device::Cpu, |_| {
            calls.set(calls.get() + 1);
            Err::<(), _>(AsrError::Ort(String::from("broken")))
        })
        .err();
        assert!(said.is_some());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn text_is_cut_after_the_limit_in_characters_not_bytes() {
        assert_eq!(split_after_chars("あいう", 2), ("あい", "う"));
        assert_eq!(split_after_chars("あいう", 3), ("あいう", ""));
        assert_eq!(split_after_chars("あいう", 4), ("あいう", ""));
        assert_eq!(split_after_chars("ab", 0), ("", "ab"));
        assert_eq!(split_after_chars("", 0), ("", ""));
    }

    // ---- the fetched model: `hayamimi models fetch --punct-4class`, then `-- --ignored` ----

    type Check = Result<(), Box<dyn std::error::Error>>;

    /// The `models` directory at or above the crate that holds the fetched
    /// model. Models are not in git, so a worktree reads the main checkout's.
    fn fetched_models() -> Result<PathBuf, Box<dyn std::error::Error>> {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .map(|directory| directory.join("models"))
            .find(|models| models.join(MODEL_DIR).join(FP32_FILE).is_file())
            .ok_or_else(|| {
                "no models/punct-ja-4class-permissive: run `hayamimi models fetch --punct-4class`"
                    .into()
            })
    }

    fn fetched_model() -> Result<PathBuf, Box<dyn std::error::Error>> {
        Ok(fetched_models()?.join(MODEL_DIR))
    }

    /// The fp32 model, on the GPU or, where CUDA cannot be used, the CPU.
    fn open_fp32() -> Result<PunctFour, Box<dyn std::error::Error>> {
        let directory = fetched_model()?;
        let runtime = OrtRuntime::shared()?;
        let (opened, _, _) = cuda_then_cpu(Device::Cuda, |device| {
            PunctFour::open_file(runtime, &directory, FP32_FILE, device, false)
        })?;
        Ok(opened)
    }

    #[test]
    #[ignore = "reads the fetched four-class model"]
    fn restore_smoke_question() -> Check {
        let mut punct = open_fp32()?;
        assert!(
            punct
                .restore("これって本当に大丈夫なんですか")?
                .ends_with('？')
        );
        Ok(())
    }

    #[test]
    #[ignore = "reads the fetched four-class model"]
    fn restore_smoke_exclamation() -> Check {
        // `！` is held back by default; the gate is opened here to see that the head itself is live.
        let text = "本当にありがとうございました";
        let mut punct = open_fp32()?;
        assert!(!punct.restore(text)?.ends_with('！'));
        let gates = Gates {
            exclaim: Some(0.9),
            comma: Some(COMMA_THRESHOLD),
        };
        let mut punct = punct.with_gates(gates);
        assert!(punct.restore(text)?.ends_with('！'));
        Ok(())
    }

    #[test]
    #[ignore = "reads the fetched four-class model"]
    fn restore_smoke_only_inserts_marks() -> Check {
        // Taking the marks out gives the input back, character for character:
        // an offset that is off by one, or a tokenizer that changed the text,
        // would break this before anything else.
        let long = "明日の会議は午後三時から始まります".repeat(30);
        let texts = [
            "明日の会議は午後三時から始まりますので資料の準備をお願いします",
            "きょうはとてもいい天気ですね",
            "ローマ字のABCと数字の123が混ざった文です",
            // Longer than the model reads: the tail comes back as it is.
            long.as_str(),
        ];
        let mut punct = open_fp32()?;
        for text in texts {
            let restored = punct.restore(text)?;
            let without: String = restored
                .chars()
                .filter(|c| !"、。？！".contains(*c))
                .collect();
            assert_eq!(without, text);
        }
        let first = punct.restore(texts[0])?;
        assert!(first.contains('。') || first.contains('、'), "{first}");
        Ok(())
    }

    #[test]
    #[ignore = "reads the fetched four-class model"]
    fn the_model_a_run_opens_is_the_int8_one_and_restores_text() -> Check {
        let models = fetched_models()?;
        assert_eq!(onnx_file(&models.join(MODEL_DIR)), INT8_FILE);
        let runtime = OrtRuntime::shared()?;
        let opened = PunctFour::open_with_fallback(runtime, &models, Device::Cuda, false)?;
        let mut punct = opened.punctuator;
        let restored = punct.restore("これって本当に大丈夫なんですか")?;
        assert!(restored.ends_with('？'), "{restored}");
        Ok(())
    }

    #[test]
    #[ignore = "reads the fetched four-class model"]
    fn restore_smoke_empty_string() -> Check {
        let mut punct = open_fp32()?;
        assert_eq!(punct.restore("")?, "");
        assert_eq!(punct.restore("   ")?, "");
        Ok(())
    }

    #[test]
    #[ignore = "reads the fetched tokenizer"]
    fn the_fetched_tokenizer_covers_a_sentence_without_gaps() -> Check {
        let file = std::fs::read_to_string(fetched_model()?.join(TOKENIZER_FILE))?;
        let tokenizer = Unigram::from_json(&file)?;
        let sentence = "これって本当に大丈夫なんですか ABC 123 猫🐱";
        let encoded = tokenizer.encode(sentence)?;
        assert_eq!(encoded.ids.len(), encoded.offsets.len());
        // The tokens tile the text: each starts where the one before ended,
        // or repeats its span (the byte tokens of one character).
        let inner = &encoded.offsets[1..encoded.offsets.len() - 1];
        let mut previous = (0, 0);
        for &span in inner {
            assert!(span == previous || span.0 == previous.1, "{inner:?}");
            previous = span;
        }
        assert_eq!(previous.1, sentence.chars().count());
        Ok(())
    }
}
