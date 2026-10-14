//! Subtitle files from a video: `<name>.ja.srt` and `<name>.en.srt` in the
//! video's directory.
//!
//! The audio is taken out of the video, recognized, and each English line goes
//! through the same three stages (translate, edit, verify) and annotation as
//! in a live run. Only the settled translation is written. A Japanese line
//! and a line in any other language are written as they were heard.
//!
//! | language of the line | `.ja.srt` | `.en.srt` |
//! |---|---|---|
//! | Japanese | the line | nothing |
//! | English | the translation, or the line under the mark when it was withheld | the line |
//! | any other | the line | nothing |
//!
//! A video that already has either file is left alone, before any audio is
//! taken out. The files are written to a side name and renamed into place.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::deliver::with_annotations;
use crate::lang::Lang;
use crate::line::Line;
use crate::llm::Llm;
use crate::logging::{Level, LogEvent, LogSink};
use crate::pipeline::{LineKind, NoObserver, Pipeline, Status, Translation};
use crate::srt::{Cue, render};

/// The two files of a video.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub ja: PathBuf,
    pub en: PathBuf,
}

impl Paths {
    /// The files beside `video`; `None` for a path with no file name.
    pub fn beside(video: &Path) -> Option<Paths> {
        video.file_name()?;
        Some(Paths {
            ja: video.with_extension("ja.srt"),
            en: video.with_extension("en.srt"),
        })
    }

    fn existing(&self) -> Vec<(&'static str, &Path)> {
        [("ja", self.ja.as_path()), ("en", self.en.as_path())]
            .into_iter()
            .filter(|(_, path)| path.exists())
            .collect()
    }
}

/// A step that did not finish. It has said why in the log.
#[derive(Debug, PartialEq, Eq)]
pub struct StepFailed;

/// Takes the audio out of a video into a wav file.
pub trait Extract {
    fn extract(&self, video: &Path, wav: &Path, sink: &mut dyn LogSink) -> Result<(), StepFailed>;
}

/// Reads the lines out of a wav file.
pub trait Recognize {
    fn recognize(&self, wav: &Path, sink: &mut dyn LogSink) -> Result<Vec<Line>, StepFailed>;
}

/// What a job works from.
pub struct Job<'a> {
    pub video: &'a Path,
    /// The file the audio goes to and is read from.
    pub wav: &'a Path,
    pub extract: &'a dyn Extract,
    pub recognize: &'a dyn Recognize,
    /// Heads the line of a translation that was withheld; the Japanese one,
    /// because it is written into the Japanese file.
    pub withheld_mark: &'a str,
    pub now_epoch: i64,
}

/// Why no file was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SrtError {
    /// The video's path has no file name.
    NoName,
    /// One of the two files is there already.
    Exists,
    Extract,
    Recognize,
    /// The video holds no speech, so there is nothing to write.
    NoSpeech,
    Write,
}

/// How many cues went into each file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Written {
    pub ja: usize,
    pub en: usize,
}

/// The cues of both files.
#[derive(Debug, Default, PartialEq)]
pub struct Cues {
    pub ja: Vec<Cue>,
    pub en: Vec<Cue>,
}

/// The Japanese line for an English one: the translation that was settled, or
/// the line itself under the mark when there is none.
fn japanese_for(line: &Line, translation: Option<&Translation>, mark: &str) -> String {
    match translation {
        Some(found) if found.status == Status::Ok => {
            with_annotations(&found.text, &found.annotations)
        }
        _ => format!("{mark} {}", line.text),
    }
}

/// The cue of `text` over the span of `line`.
fn cue_over(line: &Line, text: &str) -> Option<Cue> {
    let (start, end) = line.span?;
    Cue::new(start, end, text)
}

impl Cues {
    /// Adds the cues of one line. `false` when the line has no usable span
    /// and so has no cue.
    pub fn add(&mut self, line: &Line, translation: Option<&Translation>, mark: &str) -> bool {
        let (ja_text, en_text) = match line.lang {
            Some(Lang::En) => (
                japanese_for(line, translation, mark),
                Some(line.text.as_str()),
            ),
            _ => (line.text.clone(), None),
        };
        let Some(ja) = cue_over(line, &ja_text) else {
            return false;
        };
        self.ja.push(ja);
        self.en
            .extend(en_text.and_then(|text| cue_over(line, text)));
        true
    }
}

fn is_english(line: &Line) -> bool {
    line.lang == Some(Lang::En)
}

/// The settled translation of every English line, in the order of `lines`;
/// `None` for the others.
fn translate_english<L: Llm>(
    lines: &[Line],
    pipeline: &mut Pipeline<L>,
    now_epoch: i64,
    sink: &mut dyn LogSink,
) -> Vec<Option<Translation>> {
    lines
        .iter()
        .map(|line| {
            is_english(line).then(|| {
                let request = line.request(Lang::En, now_epoch);
                pipeline.run(&request, &mut NoObserver, sink)
            })
        })
        .collect()
}

fn skipped(line: &Line) -> LogEvent {
    LogEvent::new(Level::Info, "srt.cue.skipped", "storage")
        .with("id", line.id.as_str())
        .with("reason", "no_time")
}

/// The cues of `lines` with their translations, saying which lines have none.
fn collect_cues(
    lines: &[Line],
    translations: &[Option<Translation>],
    mark: &str,
    sink: &mut dyn LogSink,
) -> Cues {
    let mut cues = Cues::default();
    for (line, translation) in lines.iter().zip(translations) {
        if !cues.add(line, translation.as_ref(), mark) {
            sink.emit(skipped(line));
        }
    }
    cues
}

fn render_all(cues: &[Cue]) -> String {
    cues.iter()
        .enumerate()
        .map(|(index, cue)| render(index + 1, cue))
        .collect()
}

/// The side name a file is written to before it is renamed into place.
fn side_name(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Writes each file to its side name, then renames them into place. A side
/// file left behind by a failure is removed.
fn write_files(files: &[(&Path, String)]) -> io::Result<()> {
    let result = stage_and_place(files);
    if result.is_err() {
        for (path, _) in files {
            // A side file that was never made has nothing to remove.
            let _ = fs::remove_file(side_name(path));
        }
    }
    result
}

fn stage_and_place(files: &[(&Path, String)]) -> io::Result<()> {
    for (path, text) in files {
        fs::write(side_name(path), text)?;
    }
    for (path, _) in files {
        fs::rename(side_name(path), path)?;
    }
    Ok(())
}

fn exists_event(which: &str) -> LogEvent {
    LogEvent::new(Level::Warn, "srt.output.exists", "storage").with("which", which)
}

/// Stops when either file is there already, saying which.
fn refuse_existing(paths: &Paths, sink: &mut dyn LogSink) -> Result<(), SrtError> {
    let existing = paths.existing();
    for (which, _) in &existing {
        sink.emit(exists_event(which));
    }
    if existing.is_empty() {
        Ok(())
    } else {
        Err(SrtError::Exists)
    }
}

/// Keeps the lines that are settled sentences; the others are not cues.
fn finals(lines: Vec<Line>) -> Vec<Line> {
    lines
        .into_iter()
        .filter(|line| line.kind == LineKind::Final)
        .collect()
}

fn write_result(paths: &Paths, cues: &Cues, sink: &mut dyn LogSink) -> Result<Written, SrtError> {
    let mut files = vec![(paths.ja.as_path(), render_all(&cues.ja))];
    if !cues.en.is_empty() {
        files.push((paths.en.as_path(), render_all(&cues.en)));
    }
    if write_files(&files).is_err() {
        sink.emit(
            LogEvent::new(Level::Error, "srt.output.failed", "storage").with_error("SRT_WRITE"),
        );
        return Err(SrtError::Write);
    }
    let written = Written {
        ja: cues.ja.len(),
        en: cues.en.len(),
    };
    sink.emit(
        LogEvent::new(Level::Info, "srt.output.written", "storage")
            .with("ja_cues", written.ja)
            .with("en_cues", written.en),
    );
    Ok(written)
}

/// Makes the subtitle files of a video.
pub fn make_srt<L: Llm>(
    job: &Job<'_>,
    pipeline: &mut Pipeline<L>,
    sink: &mut dyn LogSink,
) -> Result<Written, SrtError> {
    let paths = Paths::beside(job.video).ok_or(SrtError::NoName)?;
    refuse_existing(&paths, sink)?;
    sink.emit(LogEvent::new(Level::Info, "srt.extract.started", "worker"));
    job.extract
        .extract(job.video, job.wav, sink)
        .map_err(|_| SrtError::Extract)?;
    let lines = job
        .recognize
        .recognize(job.wav, sink)
        .map_err(|_| SrtError::Recognize)?;
    let lines = finals(lines);
    let translations = translate_english(&lines, pipeline, job.now_epoch, sink);
    let cues = collect_cues(&lines, &translations, job.withheld_mark, sink);
    if cues.ja.is_empty() {
        sink.emit(LogEvent::new(Level::Warn, "srt.output.empty", "storage"));
        return Err(SrtError::NoSpeech);
    }
    write_result(&paths, &cues, sink)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line::Source;
    use crate::pipeline::Pass;
    use crate::verify::HoldReason;

    const MARK: &str = "〔訳を控えました〕";

    fn line(id: &str, lang: Option<Lang>, text: &str, span: Option<(f64, f64)>) -> Line {
        Line {
            kind: LineKind::Final,
            id: id.to_owned(),
            source: Source::File,
            lang,
            text: text.to_owned(),
            span,
        }
    }

    fn english(text: &str) -> Line {
        line("file-1", Some(Lang::En), text, Some((1.0, 2.5)))
    }

    fn translation(status: Status, text: &str) -> Translation {
        Translation {
            event_type: "translation",
            id: String::from("file-1"),
            of: LineKind::Final,
            source: String::from("file"),
            src_lang: Lang::En,
            lang: "ja",
            src_sha256: String::new(),
            status,
            hold_reason: (status == Status::Held).then_some(HoldReason::Number),
            stage: Pass::E,
            settled: true,
            text: text.to_owned(),
            annotations: Vec::new(),
            skipped: Vec::new(),
        }
    }

    fn texts(cues: &[Cue]) -> Vec<&str> {
        cues.iter().map(|cue| cue.text.as_str()).collect()
    }

    #[test]
    fn the_files_sit_beside_the_video_under_its_name() {
        let paths = Paths::beside(Path::new("/v/talk.mp4"));
        assert_eq!(
            paths,
            Some(Paths {
                ja: PathBuf::from("/v/talk.ja.srt"),
                en: PathBuf::from("/v/talk.en.srt"),
            })
        );
        let dotted = Paths::beside(Path::new("a.b.mkv"));
        assert_eq!(
            dotted.map(|found| found.ja),
            Some(PathBuf::from("a.b.ja.srt"))
        );
    }

    #[test]
    fn a_path_with_no_file_name_has_no_files() {
        assert_eq!(Paths::beside(Path::new("/")), None);
        assert_eq!(Paths::beside(Path::new("")), None);
    }

    #[test]
    fn a_japanese_line_goes_to_the_japanese_file_only() {
        let mut cues = Cues::default();
        let heard = line("file-1", Some(Lang::Ja), "こんにちは。", Some((0.5, 2.0)));
        assert!(cues.add(&heard, None, MARK));
        assert_eq!(texts(&cues.ja), ["こんにちは。"]);
        assert!(cues.en.is_empty());
    }

    #[test]
    fn a_line_in_another_language_goes_to_the_japanese_file_as_it_was_heard() {
        let mut cues = Cues::default();
        let heard = line("file-1", None, "你好。", Some((0.0, 1.0)));
        assert!(cues.add(&heard, None, MARK));
        assert_eq!(texts(&cues.ja), ["你好。"]);
        assert!(cues.en.is_empty());
    }

    #[test]
    fn an_english_line_has_its_translation_in_the_japanese_file_and_itself_in_the_english() {
        let mut cues = Cues::default();
        let found = translation(Status::Ok, "こんにちは。");
        assert!(cues.add(&english("Hello."), Some(&found), MARK));
        assert_eq!(texts(&cues.ja), ["こんにちは。"]);
        assert_eq!(texts(&cues.en), ["Hello."]);
        assert_eq!((cues.ja[0].start_s, cues.ja[0].end_s), (1.0, 2.5));
        assert_eq!((cues.en[0].start_s, cues.en[0].end_s), (1.0, 2.5));
    }

    #[test]
    fn a_withheld_or_missing_translation_leaves_the_line_under_the_mark() {
        for found in [Some(translation(Status::Held, "")), None] {
            let mut cues = Cues::default();
            assert!(cues.add(&english("I paid 100 dollars."), found.as_ref(), MARK));
            assert_eq!(texts(&cues.ja), [format!("{MARK} I paid 100 dollars.")]);
            assert_eq!(texts(&cues.en), ["I paid 100 dollars."]);
        }
    }

    #[test]
    fn a_line_with_no_span_has_no_cue_in_either_file() {
        let mut cues = Cues::default();
        let timeless = line("file-1", Some(Lang::En), "Hello.", None);
        assert!(!cues.add(&timeless, None, MARK));
        assert_eq!(cues, Cues::default());
    }

    #[test]
    fn cues_are_numbered_from_one() {
        let first = Cue::new(0.0, 1.0, "a");
        let second = Cue::new(1.0, 2.0, "b");
        let (Some(first), Some(second)) = (first, second) else {
            panic!("a cue the test wrote was refused");
        };
        let text = render_all(&[first, second]);
        assert!(
            text.starts_with("1\n00:00:00,000 --> 00:00:01,000\na\n\n2\n"),
            "{text}"
        );
    }

    #[test]
    fn only_final_lines_are_kept() {
        let mut refined = english("Hello.");
        refined.kind = LineKind::Refine;
        let kept = finals(vec![english("Hello."), refined]);
        assert_eq!(kept.len(), 1);
    }

    fn scratch(name: &str) -> io::Result<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/unit-tmp")
            .join(format!("video-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    #[test]
    fn files_are_placed_whole_and_no_side_file_is_left() -> io::Result<()> {
        let dir = scratch("placed")?;
        let (ja, en) = (dir.join("a.ja.srt"), dir.join("a.en.srt"));
        write_files(&[
            (ja.as_path(), String::from("J")),
            (en.as_path(), String::from("E")),
        ])?;
        assert_eq!(fs::read_to_string(&ja)?, "J");
        assert_eq!(fs::read_to_string(&en)?, "E");
        assert!(!side_name(&ja).exists() && !side_name(&en).exists());
        Ok(())
    }

    #[test]
    fn a_failed_write_leaves_no_file_under_the_real_name() -> io::Result<()> {
        let dir = scratch("failed")?;
        let (ok, missing) = (
            dir.join("a.ja.srt"),
            dir.join("no-such-dir").join("a.en.srt"),
        );
        let files = [
            (ok.as_path(), String::from("J")),
            (missing.as_path(), String::from("E")),
        ];
        assert!(write_files(&files).is_err());
        assert!(!ok.exists());
        assert!(!side_name(&ok).exists());
        Ok(())
    }

    #[test]
    fn existing_files_are_found_by_language() -> io::Result<()> {
        let dir = scratch("existing")?;
        let video = dir.join("v.mp4");
        let Some(paths) = Paths::beside(&video) else {
            return Err(io::Error::other("no name"));
        };
        assert!(paths.existing().is_empty());
        fs::write(&paths.en, "x")?;
        let found: Vec<&str> = paths
            .existing()
            .into_iter()
            .map(|(which, _)| which)
            .collect();
        assert_eq!(found, ["en"]);
        Ok(())
    }
}
