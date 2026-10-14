//! The pages carry the values of the board (`docs/design/r6_board.ja.md`):
//! the colours give the contrasts of its table, and the words are the words of
//! its section 11.

use std::collections::BTreeMap;
use std::path::Path;

use hayamimi_ui::css;
use hayamimi_ui::strings::{Locale, Strings};

type Failure = Box<dyn std::error::Error>;

fn board() -> Result<String, Failure> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/r6_board.ja.md");
    Ok(std::fs::read_to_string(path)?)
}

/// The colours of `:root` by name, as (r, g, b).
fn colours() -> Result<BTreeMap<String, [u8; 3]>, Failure> {
    let mut found = BTreeMap::new();
    for declaration in css::TOKENS.split(';') {
        let Some((name, value)) = declaration.rsplit_once(':') else {
            continue;
        };
        let (Some(name), value) = (name.split_whitespace().last(), value.trim()) else {
            continue;
        };
        let Some(hex) = value.strip_prefix('#') else {
            continue;
        };
        if hex.len() == 6 && name.starts_with("--") {
            let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16);
            found.insert(
                name.trim_start_matches("--").to_owned(),
                [channel(0)?, channel(2)?, channel(4)?],
            );
        }
    }
    Ok(found)
}

fn luminance([r, g, b]: [u8; 3]) -> f64 {
    let linear = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// BD-9: the pairs of the board's table (section 3.1) and the contrast it gives.
const PAIRS: [(&str, &str, f64); 17] = [
    ("paper", "ink", 15.31),
    ("paper-dim", "ink", 8.13),
    ("paper-dim", "ink-2", 7.61),
    ("paper-faint", "ink", 5.14),
    ("paper-faint", "ink-2", 4.81),
    ("ink", "c-ja", 4.87),
    ("ink", "c-en", 4.96),
    ("ink", "c-zh", 8.19),
    ("ink", "c-ko", 7.08),
    ("ink", "c-yue", 5.27),
    ("ink", "c-xx", 4.93),
    ("c-ja", "ink-2", 4.55),
    ("c-en", "ink-2", 4.64),
    ("c-zh", "ink-2", 7.66),
    ("c-ko", "ink-2", 6.63),
    ("c-yue", "ink-2", 4.93),
    ("c-xx", "ink-2", 4.61),
];

#[test]
fn bd9_the_colours_give_the_contrasts_of_the_board_and_all_reach_four_and_a_half()
-> Result<(), Failure> {
    let by_name = colours()?;
    assert!(by_name.len() >= 12, "{by_name:?}");
    for (front, back, written) in PAIRS {
        let (Some(a), Some(b)) = (by_name.get(front), by_name.get(back)) else {
            return Err(format!("the colour {front} or {back} is not in the tokens").into());
        };
        let made = contrast(*a, *b);
        assert!(
            (made - written).abs() < 0.011,
            "{front} on {back}: {made:.3} against {written}"
        );
        assert!(made >= 4.5, "{front} on {back}: {made:.3}");
    }
    Ok(())
}

#[test]
fn bd9_a_pair_with_the_wrong_ground_is_told_apart() -> Result<(), Failure> {
    // The negative control: the faint text on the lighter ground of a language mark.
    let by_name = colours()?;
    let (Some(faint), Some(xx)) = (by_name.get("paper-faint"), by_name.get("c-xx")) else {
        return Err("a token is missing".into());
    };
    assert!(contrast(*faint, *xx) < 4.5);
    Ok(())
}

/// The rows of the table of words in section 11 of the board: key, ja, en.
fn board_words() -> Result<Vec<(String, String, String)>, Failure> {
    let text = board()?;
    let (_, section) = text
        .split_once("## 11. 画面の文言")
        .ok_or("no section 11")?;
    let (section, _) = section.split_once("## 12.").ok_or("no section 12")?;
    let mut rows = Vec::new();
    for line in section.lines().filter(|line| line.starts_with("| `ui.")) {
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        let [key, ja, en] = cells[..] else {
            return Err(format!("a row without three cells: {line}").into());
        };
        rows.push((
            key.trim_matches('`').to_owned(),
            ja.to_owned(),
            en.to_owned(),
        ));
    }
    Ok(rows)
}

#[test]
fn the_words_of_both_tables_are_the_words_of_section_11_of_the_board() -> Result<(), Failure> {
    let rows = board_words()?;
    assert_eq!(rows.len(), 24, "{rows:?}");
    let ja = Strings::embedded(Locale::Ja)?;
    let en = Strings::embedded(Locale::En)?;
    for (key, written_ja, written_en) in &rows {
        assert_eq!(ja.text(key), written_ja, "{key}");
        assert_eq!(en.text(key), written_en, "{key}");
    }
    // Both tables hold these words and no others.
    assert_eq!(ja.keys().count(), rows.len());
    Ok(())
}
