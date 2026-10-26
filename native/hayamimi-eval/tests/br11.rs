//! BR-11: every clip of the IP-4 pair run is joined to the `--lang` runs, or
//! named with the reason it could not be (design 10.13.13, 3 and 14).
//!
//! A clip is joined when some repeat of it has `L_asr` in the pair run and in
//! the `--lang` run that read it. A clip is tied by the sha256 of its wav, as
//! `ip04.rs` ties it. The rows of the real run (T-BR-11-002 and 003) read the
//! records in `docs/results/in-person/` and the manifests under `testdata/`, so
//! they are `#[ignore]`d; every other row feeds the tool samples made by hand.

mod support;

use std::path::Path;

use hayamimi_eval::compare::{
    ClipNames, ClipReason, ForcedRun, Lack, SideReason, name_unjoined_clips, read_samples,
};
use hayamimi_eval::latency::{Event, EventKind, Sample, Window, build_samples};
use hayamimi_eval::manifest::read_rows;
use serde_json::Value;
use support::{Failure, REPO, TestResult, find_up, window_sample};

/// What one repeat of a clip did.
#[derive(Clone, Copy)]
enum Repeat {
    /// A `final` closed the utterance.
    Closed,
    /// Nothing was read.
    Silent,
    /// This many `final`s came, the last before the utterance ended.
    Early(u32),
}

fn sample(repeat: u32, utt: u32, what: Repeat) -> Sample {
    let mut sample = window_sample(repeat, utt, 0, 1_000_000);
    match what {
        Repeat::Closed => {
            sample.finals = 1;
            sample.t_final_us = Some(1_400_000);
        }
        Repeat::Silent => {}
        Repeat::Early(finals) => sample.finals = finals,
    }
    sample
}

/// A run of clips, each with what its three repeats did.
fn run(clips: &[[Repeat; 3]]) -> Vec<Sample> {
    let mut samples = Vec::new();
    for (at, repeats) in clips.iter().enumerate() {
        for (number, what) in repeats.iter().enumerate() {
            samples.push(sample(number as u32 + 1, at as u32 + 1, *what));
        }
    }
    samples
}

fn all(what: Repeat) -> [Repeat; 3] {
    [what; 3]
}

fn shas(count: usize) -> Vec<String> {
    (1..=count).map(|at| format!("clip-{at}")).collect()
}

fn names(joined: &ClipNames) -> Vec<(u32, &'static str)> {
    joined
        .named
        .iter()
        .map(|clip| (clip.utt, clip.reason.name()))
        .collect()
}

fn joined_utts(found: &ClipNames, shas: &[String]) -> Vec<u32> {
    shas.iter()
        .enumerate()
        .filter(|(_, sha)| found.joined.contains(sha))
        .map(|(at, _)| at as u32 + 1)
        .collect()
}

fn name_clips(pair: &[Sample], forced: &[Sample], count: usize) -> Result<ClipNames, Failure> {
    let shas = shas(count);
    let forced = [ForcedRun {
        label: "--lang ja",
        samples: forced,
        shas: &shas,
    }];
    name_unjoined_clips(pair, &shas, &forced).map_err(|error| error.to_string())
}

/// T-BR-11-001
#[test]
fn br11_every_clip_of_the_pair_run_is_joined_or_named_with_its_reason() -> TestResult {
    let pair = run(&[
        all(Repeat::Closed),
        all(Repeat::Silent),
        all(Repeat::Early(1)),
        all(Repeat::Early(2)),
        [Repeat::Silent, Repeat::Closed, Repeat::Early(1)],
    ]);
    let forced = run(&[all(Repeat::Closed); 5]);
    let found = name_clips(&pair, &forced, 5)?;
    assert_eq!(joined_utts(&found, &shas(5)), [1, 5]);
    assert_eq!(
        names(&found),
        [
            (2, "no_final"),
            (3, "closed_before_end"),
            (4, "closed_before_end")
        ]
    );
    assert_eq!(found.joined.len() + found.named.len(), 5);
    Ok(())
}

/// The clips of `lat-dialog` read by the three runs of IP-4.
struct Real {
    pair: Vec<Sample>,
    pair_shas: Vec<String>,
    ja: Vec<Sample>,
    ja_shas: Vec<String>,
    en: Vec<Sample>,
    en_shas: Vec<String>,
}

fn sha_of_each_row(manifest: &str) -> Result<Vec<String>, Failure> {
    read_rows(&find_up(manifest)?)
        .map_err(|error| error.to_string())?
        .iter()
        .enumerate()
        .map(|(at, row)| {
            row.get("sha256")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("row {at} of {manifest} has no `sha256`"))
        })
        .collect()
}

fn samples_in(name: &str) -> Result<Vec<Sample>, Failure> {
    let records = Path::new(REPO).join("docs/results/in-person");
    read_samples(&records.join(name)).map_err(|error| error.to_string())
}

fn real() -> Result<Real, Failure> {
    Ok(Real {
        pair: samples_in("ip02_pair.jsonl")?,
        pair_shas: sha_of_each_row("testdata/manifests/lat-dialog.json")?,
        ja: samples_in("ip04_lang_ja.jsonl")?,
        ja_shas: sha_of_each_row("testdata/manifests/lat-ja.json")?,
        en: samples_in("ip04_lang_en.jsonl")?,
        en_shas: sha_of_each_row("testdata/manifests/lat-en.json")?,
    })
}

fn name_real(real: &Real) -> Result<ClipNames, Failure> {
    let forced = [
        ForcedRun {
            label: "--lang ja",
            samples: &real.ja,
            shas: &real.ja_shas,
        },
        ForcedRun {
            label: "--lang en",
            samples: &real.en,
            shas: &real.en_shas,
        },
    ];
    name_unjoined_clips(&real.pair, &real.pair_shas, &forced).map_err(|error| error.to_string())
}

/// What the pair run did in the clip of each named row, side by side with the reason of the clip.
fn pair_side_of(found: &ClipNames) -> Vec<(u32, Option<String>)> {
    found
        .named
        .iter()
        .map(|clip| (clip.utt, clip.reason.pair_side().map(SideReason::text)))
        .collect()
}

/// T-BR-11-002
#[test]
#[ignore = "reads testdata or run records, which are not in git"]
fn the_pair_run_names_row_3_as_no_final_and_rows_9_19_21_as_closed_before_end() -> TestResult {
    let found = name_real(&real()?)?;
    assert_eq!(found.joined.len(), 26);
    assert_eq!(found.named.len(), 4);
    assert_eq!(
        pair_side_of(&found),
        [
            (3, Some(String::from("no_final"))),
            (9, Some(String::from("closed_before_end"))),
            (19, Some(String::from("closed_before_end"))),
            (21, Some(String::from("closed_before_end"))),
        ]
    );
    // The rows the `--lang` runs could not read either are named for both runs.
    assert_eq!(
        names(&found),
        [
            (3, "missing_in_both"),
            (9, "closed_before_end"),
            (19, "missing_in_both"),
            (21, "missing_in_both")
        ]
    );
    Ok(())
}

/// T-BR-11-003
#[test]
#[ignore = "reads testdata or run records, which are not in git"]
fn joined_and_named_clips_add_up_to_the_clips_of_the_manifest() -> TestResult {
    let real = real()?;
    let found = name_real(&real)?;
    assert_eq!(found.joined.len() + found.named.len(), real.pair_shas.len());
    assert_eq!(real.pair_shas.len(), 30);
    Ok(())
}

/// T-BR-11-004
#[test]
fn a_clip_with_no_reason_left_makes_the_record_an_error() {
    let pair = run(&[all(Repeat::Closed); 4]);
    let forced = run(&[all(Repeat::Closed); 5]);
    let said = name_clips(&pair, &forced, 5).err();
    assert!(
        said.as_deref().is_some_and(|text| text.contains("clip-5")),
        "{said:?}"
    );
}

fn event_final(t_us: i64) -> Event {
    Event {
        t_us,
        kind: EventKind::Final {
            text: String::from("x"),
            latency_ms: None,
            id: None,
            t1_us: None,
        },
    }
}

/// T-BR-11-005
#[test]
fn a_final_arriving_exactly_at_the_end_closes_the_utterance() -> TestResult {
    let windows = [
        Window {
            repeat: 1,
            utt: 1,
            t_start_us: 0,
            t_end_us: 1_000_000,
        },
        Window {
            repeat: 1,
            utt: 2,
            t_start_us: 5_000_000,
            t_end_us: 6_000_000,
        },
    ];
    let events = [event_final(1_000_000), event_final(5_999_999)];
    let refs = vec![String::new(); 2];
    let samples = build_samples(&windows, &refs, &events)?;
    let forced = run(&[all(Repeat::Closed); 2]);
    let found = name_clips(&samples, &forced, 2)?;
    assert_eq!(joined_utts(&found, &shas(2)), [1]);
    assert_eq!(names(&found), [(2, "closed_before_end")]);
    Ok(())
}

/// One clip whose three repeats did `what`, in a run of one clip.
fn one_clip(what: [Repeat; 3]) -> Vec<Sample> {
    run(&[what])
}

/// T-BR-11-006
#[test]
fn a_clip_is_named_only_when_every_repeat_lacks_a_closing_final() -> TestResult {
    let cases = [
        ([Repeat::Closed; 3], false),
        ([Repeat::Silent, Repeat::Closed, Repeat::Closed], false),
        ([Repeat::Silent, Repeat::Early(1), Repeat::Closed], false),
        ([Repeat::Silent, Repeat::Early(1), Repeat::Silent], true),
    ];
    for (what, named) in cases {
        let found = name_clips(&one_clip(what), &one_clip(all(Repeat::Closed)), 1)?;
        assert_eq!(!found.named.is_empty(), named);
        assert_eq!(found.joined.len() + found.named.len(), 1);
    }
    Ok(())
}

/// T-BR-11-007
#[test]
fn a_clip_missing_in_the_forced_language_run_is_named_with_its_reason() -> TestResult {
    let pair = one_clip(all(Repeat::Closed));
    let found = name_clips(&pair, &one_clip(all(Repeat::Silent)), 1)?;
    let Some(clip) = found.named.first() else {
        return Err("the clip was not named".into());
    };
    assert_eq!(clip.reason.name(), "missing_in_lang_run");
    assert_eq!(
        clip.reason,
        ClipReason::MissingInLangRun {
            run: String::from("--lang ja"),
            side: SideReason::Same(Lack::NoFinal)
        }
    );
    assert_eq!(clip.reason.pair_side(), None);
    let found = name_clips(&pair, &one_clip(all(Repeat::Early(1))), 1)?;
    assert_eq!(
        found.named.first().map(|clip| clip.reason.text()),
        Some(String::from(
            "missing_in_lang_run (--lang ja): closed_before_end"
        ))
    );
    Ok(())
}

/// T-BR-11-008
#[test]
fn a_clip_whose_repeats_lack_for_different_reasons_is_named_mixed_with_each_reason() -> TestResult {
    let pair = one_clip([Repeat::Silent, Repeat::Early(1), Repeat::Early(1)]);
    let found = name_clips(&pair, &one_clip(all(Repeat::Closed)), 1)?;
    let Some(clip) = found.named.first() else {
        return Err("the clip was not named".into());
    };
    assert_eq!(clip.reason.name(), "mixed");
    assert_eq!(
        clip.reason.text(),
        "mixed: 1 no_final, 2 closed_before_end, 3 closed_before_end"
    );
    Ok(())
}

/// T-BR-11-009
#[test]
fn a_clip_with_the_same_reason_in_every_repeat_is_not_mixed() -> TestResult {
    let counts = one_clip([Repeat::Early(1), Repeat::Early(2), Repeat::Early(1)]);
    let found = name_clips(&counts, &one_clip(all(Repeat::Closed)), 1)?;
    assert_eq!(names(&found), [(1, "closed_before_end")]);
    let silent = name_clips(
        &one_clip(all(Repeat::Silent)),
        &one_clip(all(Repeat::Closed)),
        1,
    )?;
    assert_eq!(names(&silent), [(1, "no_final")]);
    Ok(())
}

/// T-BR-11-010
#[test]
fn the_lang_run_side_of_a_missing_clip_lists_its_repeats_the_same_way() -> TestResult {
    let forced = one_clip([Repeat::Silent, Repeat::Early(1), Repeat::Silent]);
    let found = name_clips(&one_clip(all(Repeat::Closed)), &forced, 1)?;
    assert_eq!(
        found.named.first().map(|clip| clip.reason.text()),
        Some(String::from(
            "missing_in_lang_run (--lang ja): mixed: 1 no_final, 2 closed_before_end, 3 no_final"
        ))
    );
    Ok(())
}

/// The seven clips of T-BR-11-011: how many of each reason, and the clips joined.
fn seven_clips() -> (Vec<Sample>, Vec<Sample>) {
    let pair = run(&[
        all(Repeat::Closed),
        all(Repeat::Closed),
        all(Repeat::Silent),
        all(Repeat::Early(1)),
        [Repeat::Silent, Repeat::Early(1), Repeat::Early(1)],
        all(Repeat::Closed),
        [Repeat::Silent, Repeat::Silent, Repeat::Closed],
    ]);
    let forced = run(&[
        all(Repeat::Closed),
        all(Repeat::Closed),
        all(Repeat::Closed),
        all(Repeat::Closed),
        all(Repeat::Closed),
        all(Repeat::Silent),
        all(Repeat::Closed),
    ]);
    (pair, forced)
}

/// T-BR-11-011
#[test]
fn the_reasons_add_up_to_the_clips_that_could_not_be_joined() -> TestResult {
    let (pair, forced) = seven_clips();
    let found = name_clips(&pair, &forced, 7)?;
    assert_eq!(joined_utts(&found, &shas(7)), [1, 2, 7]);
    assert_eq!(
        names(&found),
        [
            (3, "no_final"),
            (4, "closed_before_end"),
            (5, "mixed"),
            (6, "missing_in_lang_run")
        ]
    );
    assert_eq!(found.joined.len() + found.named.len(), 7);
    Ok(())
}

/// T-BR-11-012
#[test]
fn a_clip_missing_in_both_runs_is_named_missing_in_both_with_each_side() -> TestResult {
    let found = name_clips(
        &one_clip(all(Repeat::Silent)),
        &one_clip(all(Repeat::Early(1))),
        1,
    )?;
    let Some(clip) = found.named.first() else {
        return Err("the clip was not named".into());
    };
    assert_eq!(clip.reason.name(), "missing_in_both");
    assert_eq!(
        clip.reason.pair_side().map(SideReason::text),
        Some(String::from("no_final"))
    );
    assert_eq!(
        clip.reason.text(),
        "missing_in_both: pair no_final; --lang ja closed_before_end"
    );
    Ok(())
}

/// T-BR-11-013
#[test]
fn the_five_reasons_add_up_to_the_clips_that_could_not_be_joined() -> TestResult {
    let pair = run(&[
        all(Repeat::Closed),
        all(Repeat::Closed),
        all(Repeat::Silent),
        all(Repeat::Early(1)),
        [Repeat::Silent, Repeat::Early(1), Repeat::Early(1)],
        all(Repeat::Closed),
        [Repeat::Silent, Repeat::Silent, Repeat::Closed],
        all(Repeat::Silent),
    ]);
    let forced = run(&[
        all(Repeat::Closed),
        all(Repeat::Closed),
        all(Repeat::Closed),
        all(Repeat::Closed),
        all(Repeat::Closed),
        all(Repeat::Silent),
        all(Repeat::Closed),
        all(Repeat::Silent),
    ]);
    let found = name_clips(&pair, &forced, 8)?;
    assert_eq!(joined_utts(&found, &shas(8)), [1, 2, 7]);
    assert_eq!(
        names(&found),
        [
            (3, "no_final"),
            (4, "closed_before_end"),
            (5, "mixed"),
            (6, "missing_in_lang_run"),
            (8, "missing_in_both")
        ]
    );
    assert_eq!(found.joined.len() + found.named.len(), 8);
    Ok(())
}

/// T-BR-11-014
#[test]
fn both_sides_of_a_missing_in_both_clip_use_the_per_repeat_form() -> TestResult {
    let pair = one_clip([Repeat::Silent, Repeat::Early(1), Repeat::Silent]);
    let sha = shas(1);
    let forced = one_clip(all(Repeat::Silent));
    let english = [ForcedRun {
        label: "--lang en",
        samples: &forced,
        shas: &sha,
    }];
    let found = name_unjoined_clips(&pair, &sha, &english)?;
    assert_eq!(
        found.named.first().map(|clip| clip.reason.text()),
        Some(String::from(
            "missing_in_both: pair mixed: 1 no_final, 2 closed_before_end, 3 no_final; --lang en no_final"
        ))
    );
    Ok(())
}
