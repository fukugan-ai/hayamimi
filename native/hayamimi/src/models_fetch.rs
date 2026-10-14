//! `hayamimi models fetch`: gets the model files the stages read, with no
//! Python and no script around it.
//!
//! A group is fetched from its sources in order: the release of sherpa-onnx
//! first, a Hugging Face repository after it. A group is written to a staging
//! directory and each file is renamed into place only when all of them have
//! arrived, so a fetch that breaks half way leaves the models directory as it
//! was. A group that is already complete is not fetched again. When no source
//! gives a group, the files already there stay, one `models.fallback.previous`
//! event says so, and the exit code is 0: the stages that lack their model are
//! left out when `hayamimi` runs, as the design says for a missing model.
//!
//! `manifest.json` in the models directory lists every file with its stage,
//! sha256 and size; it is rewritten only when its content changes.

mod archive;
mod error;
mod items;
mod keys;
mod manifest;
mod net;
mod options;
mod report;
mod source;
mod stage;

use std::fs;
use std::path::Path;

use error::FetchError;
use items::{Files, Item, Source, selected};
use net::{Http, Net};
use options::{Options, Parsed};
use report::Tally;
use stage::Stage;

use crate::log;
use crate::notice::Notice;
use crate::strings::{self, Strings};

/// Every group wanted is there, or the files already there were kept.
pub const EXIT_OK: u8 = 0;
/// The models directory or the manifest could not be written.
pub const EXIT_FAILED: u8 = 1;
/// The command line was refused.
pub const EXIT_REFUSED: u8 = 2;

/// What happened to one group.
#[derive(Debug)]
enum Outcome {
    Present,
    Fetched { source: Source, files: usize },
    Unavailable,
}

/// One group's outcome, with the sources that failed before it.
#[derive(Debug)]
struct Step {
    outcome: Outcome,
    failures: Vec<(Source, FetchError)>,
}

/// Names in the order they are installed: a snapshot's marker file goes last,
/// so that a group is never complete while a file of it is still missing.
fn install_order(item: &Item, mut names: Vec<String>) -> Vec<String> {
    if let Files::Snapshot { marker } = item.files {
        names.sort_by_key(|name| name == marker);
    }
    names
}

/// Gets the group from one source and installs it; returns the file count.
fn try_source(
    net: &dyn Net,
    options: &Options,
    item: &Item,
    source: Source,
) -> Result<usize, FetchError> {
    let models = options.dir.as_path();
    let stage = Stage::create(models, item.stage)?;
    let names = source::gather(net, options, item, source, &stage)?;
    let count = names.len();
    stage.install(&item.dest(models), &install_order(item, names))?;
    Ok(count)
}

/// A complete group is left alone; otherwise each source is tried in order.
fn fetch_item(net: &dyn Net, options: &Options, item: &Item) -> Step {
    let mut failures = Vec::new();
    if item.is_present(&options.dir) {
        return Step {
            outcome: Outcome::Present,
            failures,
        };
    }
    for &source in item.sources {
        match try_source(net, options, item, source) {
            Ok(files) => {
                return Step {
                    outcome: Outcome::Fetched { source, files },
                    failures,
                };
            }
            Err(error) => failures.push((source, error)),
        }
    }
    Step {
        outcome: Outcome::Unavailable,
        failures,
    }
}

fn say(strings: &Strings, key: &str, item: &Item) {
    eprintln!("{} {}", strings.cli_text(key), item.stage);
}

/// Prints the events and lines of one group.
fn announce(strings: &Strings, item: &Item, step: &Step) {
    for (source, error) in &step.failures {
        log::print(&report::source_failed_line(strings, item, *source, error));
        say(strings, keys::SOURCE_FAILED, item);
    }
    match step.outcome {
        Outcome::Present => {
            log::print(&report::present_line(strings, item));
            say(strings, keys::PRESENT, item);
        }
        Outcome::Fetched { source, files } => {
            log::print(&report::fetched_line(strings, item, source, files));
            say(strings, keys::FETCHED, item);
        }
        Outcome::Unavailable => say(strings, keys::GROUP_FAILED, item),
    }
}

/// Counts the step and returns the manifest with the group's files brought up to date.
fn settle(
    models: &Path,
    item: &Item,
    step: &Step,
    entries: Vec<manifest::Entry>,
    tally: &mut Tally,
) -> Result<Vec<manifest::Entry>, FetchError> {
    match step.outcome {
        Outcome::Unavailable => {
            tally.unavailable += 1;
            tally.empty += usize::from(!item.has_any_file(models));
            Ok(entries)
        }
        Outcome::Present | Outcome::Fetched { .. } => {
            if matches!(step.outcome, Outcome::Present) {
                tally.present += 1;
            } else {
                tally.fetched += 1;
            }
            let names = item.names_on_disk(models)?;
            let group = manifest::entries_of(models, item, &names, &entries)?;
            Ok(manifest::replace_group(entries, item.stage, group))
        }
    }
}

/// Fetches every selected group and writes the manifest.
fn fetch_all(net: &dyn Net, strings: &Strings, options: &Options) -> Result<Tally, FetchError> {
    let models = options.dir.as_path();
    let mut entries = manifest::read(models);
    let mut tally = Tally::default();
    for item in selected(options) {
        let step = fetch_item(net, options, item);
        announce(strings, item, &step);
        entries = settle(models, item, &step, entries, &mut tally)?;
    }
    tally.manifest_written = manifest::write_if_changed(models, entries)?;
    Ok(tally)
}

/// Says how the run ended and returns the exit code.
fn finish(strings: &Strings, tally: &Tally) -> u8 {
    if tally.unavailable > 0 {
        let notice = Notice::models_previous();
        log::print(&report::previous_line(strings, &notice, tally));
        for key in notice.stderr_keys() {
            eprintln!("{}", strings.cli_text(&key));
        }
    }
    log::print(&report::finished_line(strings, tally));
    eprintln!("{}", strings.cli_text(keys::DONE));
    EXIT_OK
}

fn fetch_command(net: &dyn Net, strings: &Strings, options: &Options) -> u8 {
    if fs::create_dir_all(&options.dir).is_err() {
        eprintln!("{}", strings.cli_text(keys::DIR_FAILED));
        eprintln!("{}", options.dir.display());
        return EXIT_FAILED;
    }
    match fetch_all(net, strings, options) {
        Ok(tally) => finish(strings, &tally),
        Err(error) => {
            eprintln!("{}", strings.cli_text(keys::MANIFEST_FAILED));
            eprintln!("{error}");
            EXIT_FAILED
        }
    }
}

/// Runs `hayamimi models <args>` and returns the exit code.
pub fn run(args: &[String], locale_tag: Option<&str>) -> u8 {
    let Ok(strings) = Strings::embedded(strings::resolve(locale_tag)) else {
        // The compiled-in tables are checked by a test; without one there is no text to print.
        return EXIT_FAILED;
    };
    match options::parse(args) {
        Ok(Parsed::Help) => {
            println!("{}", strings.cli_text(keys::HELP));
            EXIT_OK
        }
        Ok(Parsed::Fetch(options)) => fetch_command(&Http::new(), &strings, &options),
        Err(error) => {
            eprintln!("{}", strings.cli_text(error.key()));
            eprintln!("{}", strings.cli_text(strings::keys::USAGE_HINT));
            EXIT_REFUSED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::archive::tar_bz2;
    use super::items::{MINIMAL, PUNCT_4CLASS};
    use super::*;
    use hayamimi_relay::lang::Lang;
    use std::cell::RefCell;
    use std::io::Cursor;
    use std::path::PathBuf;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// A network that serves a few addresses from memory and records every address asked for.
    struct Fake {
        served: Vec<(String, Vec<u8>)>,
        asked: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(served: Vec<(String, Vec<u8>)>) -> Fake {
            Fake {
                served,
                asked: RefCell::new(Vec::new()),
            }
        }

        fn asked_for(&self, part: &str) -> bool {
            self.asked.borrow().iter().any(|url| url.contains(part))
        }

        fn count(&self) -> usize {
            self.asked.borrow().len()
        }
    }

    impl Net for Fake {
        fn open(&self, url: &str) -> Result<Box<dyn std::io::Read>, FetchError> {
            self.asked.borrow_mut().push(url.to_owned());
            match self.served.iter().find(|(address, _)| address == url) {
                Some((_, bytes)) => Ok(Box::new(Cursor::new(bytes.clone()))),
                None => Err(FetchError::Status {
                    url: url.to_owned(),
                    code: 404,
                }),
            }
        }
    }

    fn scratch(name: &str) -> Result<PathBuf, std::io::Error> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/unit-tmp")
            .join(format!("fetch-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        fs::create_dir_all(&root)?;
        Ok(root)
    }

    fn options_in(root: &Path) -> Options {
        Options {
            dir: root.to_path_buf(),
            release_url: String::from("http://release"),
            hf_url: String::from("http://hf"),
            ..Options::new()
        }
    }

    /// The group of two files that the tarball tests fetch.
    static TWO_FILES: Item = Item {
        stage: "recognizer",
        dir: "model-dir",
        files: Files::Named(&["model.int8.onnx", "tokens.txt"]),
        sources: &[Source::Tar {
            tag: "tag",
            file: "model.tar.bz2",
        }],
    };

    fn tar_url() -> String {
        String::from("http://release/tag/model.tar.bz2")
    }

    fn names_in(dir: &Path) -> Result<Vec<String>, std::io::Error> {
        let mut found = Vec::new();
        for entry in fs::read_dir(dir)? {
            found.push(entry?.file_name().to_string_lossy().into_owned());
        }
        found.sort();
        Ok(found)
    }

    #[test]
    fn tarball_without_a_wanted_file_raises_and_leaves_no_partial_dir() -> TestResult {
        let root = scratch("missing")?;
        let options = options_in(&root);
        let tarball = tar_bz2(&[("m/model.int8.onnx", b"ONNX")])?;
        let net = Fake::new(vec![(tar_url(), tarball)]);
        let step = fetch_item(&net, &options, &TWO_FILES);
        assert!(matches!(step.outcome, Outcome::Unavailable));
        assert!(matches!(
            step.failures.as_slice(),
            [(_, FetchError::MissingMember { names })] if names == &["tokens.txt"]
        ));
        assert!(
            names_in(&root)?.is_empty(),
            "neither the group's directory nor a staging directory is left"
        );
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn complete_tarball_extracts_only_the_wanted_files() -> TestResult {
        let root = scratch("complete")?;
        let options = options_in(&root);
        let tarball = tar_bz2(&[
            ("m/model.int8.onnx", b"ONNX"),
            ("m/model.onnx", b"FLOAT"),
            ("m/tokens.txt", b"a 0"),
            ("m/test_wavs/0.wav", b"WAV"),
        ])?;
        let net = Fake::new(vec![(tar_url(), tarball)]);
        let step = fetch_item(&net, &options, &TWO_FILES);
        assert!(matches!(step.outcome, Outcome::Fetched { files: 2, .. }));
        assert_eq!(names_in(&root)?, ["model-dir"]);
        assert_eq!(
            names_in(&root.join("model-dir"))?,
            ["model.int8.onnx", "tokens.txt"]
        );
        assert_eq!(fs::read(root.join("model-dir/tokens.txt"))?, b"a 0");
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn partial_dir_is_refetched_and_complete_dir_is_skipped() -> TestResult {
        let root = scratch("partial")?;
        let options = options_in(&root);
        fs::create_dir_all(root.join("model-dir"))?;
        fs::write(root.join("model-dir/model.int8.onnx"), b"OLD")?;
        let tarball = tar_bz2(&[("m/model.int8.onnx", b"NEW"), ("m/tokens.txt", b"t")])?;
        let net = Fake::new(vec![(tar_url(), tarball)]);
        let first = fetch_item(&net, &options, &TWO_FILES);
        assert!(matches!(first.outcome, Outcome::Fetched { .. }));
        assert_eq!(net.count(), 1);
        assert_eq!(fs::read(root.join("model-dir/model.int8.onnx"))?, b"NEW");
        let second = fetch_item(&net, &options, &TWO_FILES);
        assert!(matches!(second.outcome, Outcome::Present));
        assert_eq!(net.count(), 1, "a complete group is not asked for again");
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    /// Runs a whole fetch against a network that has nothing, to see which addresses are asked for.
    fn asked_by(name: &str, minimal: bool, v2: bool, class4: bool) -> Result<Fake, FetchError> {
        let root = scratch(name).map_err(|e| FetchError::io("scratch", e))?;
        let options = Options {
            minimal,
            en_parakeet_v2: v2,
            punct_4class: class4,
            ..options_in(&root)
        };
        let strings = Strings::embedded(Lang::En).map_err(|message| FetchError::Net {
            url: String::from("strings"),
            message,
        })?;
        let net = Fake::new(Vec::new());
        let tally = fetch_all(&net, &strings, &options)?;
        assert_eq!(tally.fetched, 0);
        fs::remove_dir_all(&root).map_err(|e| FetchError::io("scratch", e))?;
        Ok(net)
    }

    const V2_TAR: &str = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2";
    const CLASS4: &str = "oboroge0/hayamimi-punct-ja-4class";
    const ZH_TAR: &str = "sherpa-onnx-paraformer-zh-int8-2025-10-07.tar.bz2";

    #[test]
    fn minimal_still_runs_opt_in_downloads() -> TestResult {
        let net = asked_by("both", true, true, true)?;
        assert!(net.asked_for(V2_TAR));
        assert!(net.asked_for(CLASS4));
        assert!(
            !net.asked_for(ZH_TAR),
            "minimal leaves the other languages out"
        );
        Ok(())
    }

    #[test]
    fn minimal_with_en_parakeet_v2_downloads_v2() -> TestResult {
        let net = asked_by("v2", true, true, false)?;
        assert!(net.asked_for(V2_TAR));
        assert!(!net.asked_for(CLASS4));
        assert!(!net.asked_for(ZH_TAR));
        Ok(())
    }

    #[test]
    fn minimal_with_punct_4class_downloads_it() -> TestResult {
        let net = asked_by("class4", true, false, true)?;
        assert!(net.asked_for(CLASS4));
        assert!(!net.asked_for(V2_TAR));
        assert!(!net.asked_for(ZH_TAR));
        Ok(())
    }

    #[test]
    fn the_default_run_asks_for_the_other_languages_and_neither_opt_in() -> TestResult {
        let net = asked_by("default", false, false, false)?;
        assert!(net.asked_for(ZH_TAR));
        assert!(!net.asked_for(V2_TAR));
        assert!(!net.asked_for(CLASS4));
        Ok(())
    }

    #[test]
    fn a_snapshot_marker_is_installed_last() {
        let names = vec![
            String::from("punct_4class.onnx"),
            String::from("a.json"),
            String::from("z.txt"),
        ];
        let ordered = install_order(&PUNCT_4CLASS, names);
        assert_eq!(
            ordered.last().map(String::as_str),
            Some("punct_4class.onnx")
        );
        assert_eq!(ordered.len(), 3);
    }

    #[test]
    fn a_group_that_only_a_repository_has_is_fetched_file_by_file() -> TestResult {
        let root = scratch("second")?;
        let options = options_in(&root);
        let item = &MINIMAL[3];
        let net = Fake::new(vec![
            (
                String::from(
                    "http://hf/ishiki-emo/mojicast-punct-onnx/resolve/main/punct_bert.onnx",
                ),
                b"BERT".to_vec(),
            ),
            (
                String::from("http://hf/ishiki-emo/mojicast-punct-onnx/resolve/main/vocab.txt"),
                b"v".to_vec(),
            ),
        ]);
        let step = fetch_item(&net, &options, item);
        assert!(matches!(step.outcome, Outcome::Fetched { files: 2, .. }));
        assert!(step.failures.is_empty());
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn an_empty_body_is_not_a_file_and_the_next_source_is_tried() -> TestResult {
        let root = scratch("emptybody")?;
        let options = options_in(&root);
        let item = &MINIMAL[2];
        let net = Fake::new(vec![
            (
                String::from("http://release/asr-models/silero_vad.onnx"),
                Vec::new(),
            ),
            (
                String::from("http://hf/csukuangfj/vad/resolve/main/silero_vad.onnx"),
                b"V".to_vec(),
            ),
        ]);
        let step = fetch_item(&net, &options, item);
        assert!(matches!(step.outcome, Outcome::Fetched { .. }));
        assert_eq!(step.failures.len(), 1);
        assert_eq!(fs::read(root.join("silero_vad.onnx"))?, b"V");
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn a_run_with_no_source_keeps_going_and_says_which_groups_are_empty() -> TestResult {
        let root = scratch("nosource")?;
        let options = Options {
            minimal: true,
            ..options_in(&root)
        };
        let strings = Strings::embedded(Lang::En)?;
        let net = Fake::new(Vec::new());
        let tally = fetch_all(&net, &strings, &options)?;
        assert_eq!((tally.unavailable, tally.empty, tally.fetched), (4, 4, 0));
        assert!(
            !root.join(manifest::FILE_NAME).exists(),
            "a run that got nothing makes no record"
        );
        fs::remove_dir_all(&root)?;
        Ok(())
    }
}
