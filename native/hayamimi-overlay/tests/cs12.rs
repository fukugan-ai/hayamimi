//! CS-12: Japanese text is set in cinecaption and English text in UDEV Gothic 35.
//!
//! The font files are not in the repository. They are taken from the two zips
//! in `hayamimi_fonts` of the user's profile (the files the fonts were
//! downloaded as) into a directory of the test, with the `tar.exe` of Windows,
//! which reads zips. The overlay is started as `--layout-probe` with an
//! `overlay.toml` of the test that names them, and what the webview reports in
//! `overlay.layout.measured` is read:
//!
//! - the faces of cinecaption and UDEV Gothic 35 are `loaded`;
//! - the rows of the upper lane (`loopback-`, Japanese) start their
//!   `font-family` with cinecaption, those of the lower lane (`mic-`, English)
//!   with UDEV Gothic 35;
//! - no `overlay.font.missing` is written, and `missing_glyphs`, the number of
//!   probe characters that cinecaption has no glyph for, is 0 (the sample is
//!   Japanese only, so this is the negative control of the count).
//!
//! Negative control: the same run with the cinecaption path broken writes one
//! `overlay.font.missing`, and the cinecaption face has status `error`.
//!
//! Positive control of `missing_glyphs` (design H28): the same run with the
//! cinecaption face made from a font that has no Japanese glyph (Arial, which
//! ships with Windows). The face is `loaded` and no `overlay.font.missing` is
//! written, yet `missing_glyphs` must be at least 1; a count that asks whether
//! a face is loaded, and not whether it holds the glyph, answers 0 here. The
//! broken-path run is not a positive control: a machine that has cinecaption
//! installed still draws the glyphs from the installed font, so the count of
//! that run says nothing about the face.
//!
//! Opens windows: run it with `--ignored` when nobody is using the screen.

#![cfg(windows)]

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use support::{
    Log, Measured, Overlay, TestResult, first_family, is_event, parse_measured, scratch, sleep_ms,
};

/// The `tar.exe` that ships with Windows. A `tar` found on `PATH` may be GNU tar, which does not read zips.
const TAR: &str = r"C:\Windows\System32\tar.exe";
const CINECAPTION_ZIP: &str = "cinecaption2.28.zip";
const CINECAPTION_MEMBER: &str = "cinecaption2.28/cinecaption2.28.ttf";
const UDEV_ZIP: &str = "UDEVGothic_v2.2.0.zip";
const UDEV_REGULAR: &str = "UDEVGothic_v2.2.0/UDEVGothic35-Regular.ttf";
const UDEV_BOLD: &str = "UDEVGothic_v2.2.0/UDEVGothic35-Bold.ttf";
/// A font of Windows that has no Japanese glyph: the face of the positive control of `missing_glyphs`.
const NO_JAPANESE_FONT: &str = r"C:\Windows\Fonts\arial.ttf";

#[derive(Clone)]
struct Fonts {
    cinecaption: PathBuf,
    regular: PathBuf,
    bold: PathBuf,
}

/// Takes `members` out of `zip` into `into`, and returns where each one landed.
fn extract(zip: &Path, members: &[&str], into: &Path) -> Result<Vec<PathBuf>, String> {
    let status = Command::new(TAR)
        .arg("-xf")
        .arg(zip)
        .arg("-C")
        .arg(into)
        .args(members)
        .status()
        .map_err(|error| format!("could not run {TAR}: {error}"))?;
    if !status.success() {
        return Err(format!(
            "{TAR} could not take {members:?} out of {}: {status}",
            zip.display()
        ));
    }
    members
        .iter()
        .map(|member| {
            let landed = into.join(member);
            if landed.is_file() {
                return Ok(landed);
            }
            Err(format!("{} was not extracted", landed.display()))
        })
        .collect()
}

fn extract_fonts(into: &Path) -> Result<Fonts, String> {
    let Some(home) = dirs::home_dir() else {
        return Err(String::from(
            "this account has no home directory to find hayamimi_fonts in",
        ));
    };
    let folder = home.join("hayamimi_fonts");
    let mut cinecaption = extract(&folder.join(CINECAPTION_ZIP), &[CINECAPTION_MEMBER], into)?;
    let mut udev = extract(&folder.join(UDEV_ZIP), &[UDEV_REGULAR, UDEV_BOLD], into)?;
    let (Some(bold), Some(regular), Some(cinecaption)) =
        (udev.pop(), udev.pop(), cinecaption.pop())
    else {
        return Err(String::from(
            "the font extraction returned fewer files than were asked for",
        ));
    };
    Ok(Fonts {
        cinecaption,
        regular,
        bold,
    })
}

/// The settings file that names the three font files (literal strings, so backslashes stay).
fn settings(fonts: &Fonts) -> String {
    format!(
        "cinecaption = '{}'\nudev_gothic = '{}'\nudev_gothic_bold = '{}'\n",
        fonts.cinecaption.display(),
        fonts.regular.display(),
        fonts.bold.display()
    )
}

/// What a probe run measured, and how many `overlay.font.missing` it wrote.
struct Run {
    measured: Measured,
    font_missing: usize,
}

fn probe_with(dir: &Path, name: &str, fonts: &Fonts) -> Result<Run, String> {
    let config = dir.join(format!("{name}.toml"));
    std::fs::write(&config, settings(fonts)).map_err(|error| error.to_string())?;
    let args = [
        String::from("--layout-probe"),
        String::from("--config"),
        config.to_string_lossy().into_owned(),
    ];
    let mut overlay = Overlay::launch(args, Log::at(dir.join(format!("{name}.jsonl"))))?;
    overlay.wait_opened()?;
    let event = overlay.wait(
        "overlay.layout.measured",
        Duration::from_secs(30),
        |event| is_event(event, "overlay.layout.measured"),
    )?;
    sleep_ms(500);
    let font_missing = overlay.log().named("overlay.font.missing").len();
    overlay.close()?;
    Ok(Run {
        measured: parse_measured(&event)?,
        font_missing,
    })
}

fn face_is(measured: &Measured, family: &str, status: &str) -> bool {
    measured
        .fonts
        .iter()
        .any(|face| face.family.to_lowercase().contains(family) && face.status == status)
}

/// Whether every row whose id starts with `prefix` starts its font-family with `family`, and at least one exists.
fn rows_start_with(measured: &Measured, prefix: &str, family: &str) -> Result<(), String> {
    let rows: Vec<_> = measured
        .rows
        .iter()
        .filter(|row| row.id.starts_with(prefix))
        .collect();
    if rows.is_empty() {
        return Err(format!(
            "the probe drew no row with an id starting with {prefix}"
        ));
    }
    for row in rows {
        if !first_family(&row.font_family)
            .to_lowercase()
            .contains(family)
        {
            return Err(format!(
                "row {} is set in {:?}, not {family}",
                row.id, row.font_family
            ));
        }
    }
    Ok(())
}

fn check_with_the_files(run: &Run) -> Result<(), String> {
    let measured = &run.measured;
    for family in ["cinecaption", "udev gothic 35"] {
        if !face_is(measured, family, "loaded") {
            return Err(format!(
                "{family} is not a loaded face: {:?}",
                measured.fonts
            ));
        }
    }
    rows_start_with(measured, "loopback-", "cinecaption")?;
    rows_start_with(measured, "mic-", "udev gothic 35")?;
    if run.font_missing != 0 {
        return Err(format!(
            "overlay.font.missing was written {} times with every file in place",
            run.font_missing
        ));
    }
    eprintln!(
        "CS-12: {} probe characters are not in cinecaption (missing_glyphs)",
        measured.missing_glyphs
    );
    if measured.missing_glyphs != 0 {
        return Err(format!(
            "{} of the Japanese probe characters have no glyph in cinecaption",
            measured.missing_glyphs
        ));
    }
    Ok(())
}

/// A face that is `loaded` but holds no Japanese glyph must be counted: the count asks the glyphs, not the face.
fn check_with_a_face_without_the_glyphs(run: &Run) -> Result<(), String> {
    let measured = &run.measured;
    if !face_is(measured, "cinecaption", "loaded") {
        return Err(format!(
            "the positive control needs a loaded face, and cinecaption is not one: {:?}",
            measured.fonts
        ));
    }
    eprintln!(
        "CS-12: {} probe characters are not in the stand-in for cinecaption (missing_glyphs)",
        measured.missing_glyphs
    );
    if measured.missing_glyphs == 0 {
        return Err(String::from(
            "the measure does not work: cinecaption is a font with no Japanese glyph, yet missing_glyphs is 0",
        ));
    }
    Ok(())
}

fn check_with_a_broken_path(run: &Run) -> Result<(), String> {
    if run.font_missing != 1 {
        return Err(format!(
            "overlay.font.missing was written {} times for one broken path",
            run.font_missing
        ));
    }
    if !face_is(&run.measured, "cinecaption", "error") {
        return Err(format!(
            "the cinecaption face is not `error`: {:?}",
            run.measured.fonts
        ));
    }
    if !face_is(&run.measured, "udev gothic 35", "loaded") {
        return Err(format!(
            "UDEV Gothic 35 stopped loading with another font broken: {:?}",
            run.measured.fonts
        ));
    }
    Ok(())
}

#[test]
#[ignore = "opens windows; run it when nobody is using the screen"]
fn cs12_japanese_in_cinecaption_english_in_udev_gothic() -> TestResult {
    let dir = scratch()?;
    let fonts = extract_fonts(dir.path())?;
    let with_files = probe_with(dir.path(), "files", &fonts)?;
    check_with_the_files(&with_files)?;

    let broken = Fonts {
        cinecaption: dir
            .path()
            .join("no-such-folder")
            .join("cinecaption2.28.ttf"),
        ..fonts.clone()
    };
    let without = probe_with(dir.path(), "broken", &broken)?;
    check_with_a_broken_path(&without)?;

    let stand_in = PathBuf::from(NO_JAPANESE_FONT);
    if !stand_in.is_file() {
        return Err(format!("{NO_JAPANESE_FONT} is not on this machine; the positive control of missing_glyphs needs it").into());
    }
    let glyphless = Fonts {
        cinecaption: stand_in,
        ..fonts
    };
    let stood_in = probe_with(dir.path(), "glyphless", &glyphless)?;
    check_with_a_face_without_the_glyphs(&stood_in)?;
    Ok(())
}
