//! The style sheets of the three pages.
//!
//! Every page carries `BASE` and its own sheet; the dashboard and the
//! transcript also carry `TOKENS`, `MOTION` and `PARTS`. `BASE` holds only the
//! two rules all three share, so that the overlay stays transparent for a
//! browser source. Nothing here names a host: fonts are the ones the machine
//! has, and no sheet imports or fetches anything.

/// What every page has: no margin and a transparent ground.
pub const BASE: &str = "html, body { margin: 0; background: transparent; }\n";

/// Colours, fonts and spacings of the dashboard and the transcript.
pub const TOKENS: &str = r#"
:root {
  --ink: #0d0f13; --ink-2: #14171d; --line: #2a2f3a; --line-soft: rgba(42,47,58,.7);
  --paper: #ece5d8; --paper-dim: #b0a898; --paper-faint: #8a8473;
  --shu: #e04f2f;
  --c-ja: #e04f2f; --c-en: #5b7fd4; --c-zh: #d4a13c; --c-ko: #3fae9d; --c-yue: #b06bc6; --c-xx: #7a8194;
  --font-serif: "Shippori Mincho B1", "Yu Mincho", "Hiragino Mincho ProN", "Noto Serif CJK JP", serif;
  --font-sans: "Zen Kaku Gothic New", "Yu Gothic UI", "Meiryo", "Noto Sans CJK JP", sans-serif;
  --font-mono: "IBM Plex Mono", "Cascadia Mono", Consolas, ui-monospace, monospace;
}
* { box-sizing: border-box; }
"#;

/// The four motions, and the rule that stops all of them.
pub const MOTION: &str = r#"
@keyframes pulse { from { box-shadow: 0 0 0 0 rgba(224,79,47,.5); } to { box-shadow: 0 0 0 10px rgba(224,79,47,0); } }
@keyframes blink { 50% { opacity: 0; } }
@keyframes rise { from { opacity: 0; transform: translateY(4px); } }
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after { animation: none !important; transition: none !important; }
}
"#;

/// Parts the dashboard and the transcript share: marks, translation rows and
/// refined paragraphs.
pub const PARTS: &str = r#"
.c-ja { --lc: var(--c-ja); } .c-en { --lc: var(--c-en); } .c-zh { --lc: var(--c-zh); }
.c-ko { --lc: var(--c-ko); } .c-yue { --lc: var(--c-yue); } .c-xx { --lc: var(--c-xx); }
.lang-badge { font-family: var(--font-mono); font-size: 12px; font-weight: 600; line-height: 1.4;
  letter-spacing: .08em; padding: 2px 6px; border-radius: 2px; color: var(--ink); background: var(--lc); }
.spk { font-family: var(--font-mono); font-size: 12px; line-height: 1.4; padding: 1px 5px;
  border-radius: 2px; border: 1px solid var(--line); color: var(--paper-dim); }
.badges { display: flex; gap: 4px; align-items: center; }
.tr { display: flex; align-items: baseline; gap: 8px; font-size: 15px; line-height: 1.5; }
.tr-arrow { flex: none; font-family: var(--font-mono); font-size: 12px; line-height: 1.4; color: var(--lc); }
.tr-text { color: var(--paper-dim); transition: color 160ms; }
.tr-text.confirmed { color: var(--paper); }
.tr-done { color: var(--paper-dim); }
.tr-rest { color: var(--paper-faint); }
.tr-note { color: var(--paper-dim); }
.tr-mark { font-size: 12px; line-height: 1.4; padding: 0 4px; border-radius: 2px;
  border: 1px dashed var(--paper-dim); color: var(--paper-dim); }
.tr-reason { font-size: 12px; line-height: 1.4; color: var(--paper-dim); }
.refine { margin: 0 0 16px; animation: rise 120ms ease-out both; }
.refine-meta { display: block; font-family: var(--font-mono); font-size: 12px; line-height: 1.4;
  letter-spacing: .12em; color: var(--paper-dim); }
.refine-meta i { font-style: normal; color: var(--lc); }
.refine-text { font-family: var(--font-serif); font-weight: 600; font-size: 16px; line-height: 1.9; }
.refine .tr { margin-top: 2px; }
.empty { padding-top: 32px; text-align: center; font-size: 15px; line-height: 1.5; color: var(--paper-dim); }
.latest { position: absolute; right: 24px; bottom: 16px; font-family: var(--font-sans); font-size: 12px;
  line-height: 1.4; color: var(--paper); background: var(--ink-2); border: 1px solid var(--line);
  border-radius: 6px; padding: 4px 12px; cursor: pointer; }
.scroll { scrollbar-width: thin; scrollbar-color: var(--line) transparent; }
"#;

/// The overlay. The rules for `html, body`, `#box`, `#final-line` and
/// `#partial-line` are those of the mobile page and are compared with it by
/// `tests/overlay_parity.rs`; the rest belongs to the translation lines and the
/// fallback mark, which the mobile page does not have.
pub const OVERLAY: &str = r#"
html, body { overflow: hidden; }
#box {
  position: absolute; left: 0; right: 0; bottom: 4vh;
  text-align: center; font-family: "Yu Gothic UI", "Meiryo", sans-serif;
  font-size: 5vh; line-height: 1.4; color: #fff;
  text-shadow: 0 0 8px #000, 0 0 4px #000, 2px 2px 2px #000;
}
#final-line, #partial-line { display: block; min-height: 1.4em; }
#partial-line { opacity: 0.75; font-style: italic; font-size: 0.8em; }
#final-tr, #partial-tr { display: block; min-height: 1.4em; font-size: 0.8em; line-height: 1.4; font-style: normal; }
.tr-faint { opacity: 0.5; }
#fallback-badge {
  position: absolute; top: 2vh; right: 2vw;
  font-family: "Yu Gothic UI", "Meiryo", sans-serif; font-size: 2.4vh; line-height: 1.4; color: #fff;
  text-shadow: 0 0 8px #000, 0 0 4px #000, 2px 2px 2px #000;
  border: 0.15vh dashed #fff; border-radius: 0.4em; padding: 0.3em 0.6em;
}
"#;

/// The dashboard.
pub const DASHBOARD: &str = r#"
.dash { height: 100vh; display: flex; flex-direction: column; overflow: hidden;
  background: var(--ink); color: var(--paper); font-family: var(--font-sans); font-size: 16px; line-height: 1.65; }
.top { flex: none; display: flex; align-items: baseline; gap: 16px; padding: 12px 24px;
  border-bottom: 1px solid var(--line); }
.dot { width: 10px; height: 10px; border-radius: 50%; align-self: center; background: var(--c-xx); }
.dot.on { background: var(--shu); animation: pulse 1.6s ease-out infinite; }
.logotype { font-family: var(--font-serif); font-weight: 800; font-size: 28px; line-height: 1.2; letter-spacing: .12em; }
.logotype em { font-style: normal; color: var(--shu); }
.romaji { font-family: var(--font-mono); font-size: 12px; line-height: 1.4; letter-spacing: .3em; color: var(--paper-dim); }
.spacer { flex: 1; }
.chip { font-family: var(--font-mono); font-size: 12px; line-height: 1.4; color: var(--paper-dim);
  border: 1px solid var(--line); border-radius: 4px; padding: 4px 10px; }
.chip b { color: var(--paper); font-weight: 600; }
.fb-band { flex: none; display: flex; align-items: baseline; gap: 12px; padding: 8px 24px;
  background: var(--ink-2); border-top: 1px solid var(--line); border-bottom: 1px solid var(--line); }
.fb-label { font-size: 12px; line-height: 1.4; color: var(--paper-dim); }
.fb-chips { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; margin-left: auto; }
.fb-chip { font-family: var(--font-mono); font-size: 12px; line-height: 1.4; color: var(--paper);
  border: 1px dashed var(--line); border-radius: 4px; padding: 2px 8px; }
.strip { flex: none; min-height: 88px; padding: 20px 24px 16px; background: var(--ink-2);
  border-bottom: 1px solid var(--line); }
.strip-label { font-size: 12px; line-height: 1.4; letter-spacing: .35em; color: var(--paper-dim); }
.strip-text { margin-top: 16px; font-family: var(--font-serif); font-weight: 600; font-size: 24px; line-height: 1.5; }
.strip .tr { margin-top: 4px; }
.strip.quiet .strip-text, .strip.quiet .tr { opacity: .5; color: var(--paper-dim); }
.caret { display: inline-block; width: .5em; height: 1.05em; margin-left: .18em; vertical-align: -0.12em;
  background: var(--shu); animation: blink 1s steps(2) infinite; }
.main { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 7fr) minmax(0, 5fr); }
.col { position: relative; min-height: 0; display: flex; flex-direction: column; }
.col + .col { border-left: 1px solid var(--line); }
h2 { flex: none; margin: 0; padding: 12px 24px; display: flex; justify-content: space-between;
  font-size: 12px; font-weight: 700; line-height: 1.4; letter-spacing: .35em; color: var(--paper-dim);
  border-bottom: 1px solid var(--line); }
h2 span { color: var(--shu); }
.scroll { flex: 1; overflow-y: auto; padding: 16px 24px 32px; }
.row { display: grid; grid-template-columns: auto 1fr auto; column-gap: 12px; row-gap: 2px;
  align-items: baseline; padding: 8px 0; border-bottom: 1px dashed var(--line-soft);
  animation: rise 120ms ease-out both; }
.row-text { font-size: 16px; font-weight: 400; line-height: 1.65; }
.lat { min-width: 5ch; text-align: right; font-family: var(--font-mono); font-size: 12px; line-height: 1.4;
  color: var(--paper-dim); }
.row .tr { grid-column: 2; }
.foot { flex: none; display: flex; gap: 24px; padding: 8px 24px; border-top: 1px solid var(--line);
  font-family: var(--font-mono); font-size: 12px; line-height: 1.4; color: var(--paper-dim); }
.foot a { color: inherit; text-decoration: none; }
.foot a:focus-visible { outline: 2px solid var(--paper); outline-offset: 2px; }
@media (max-width: 860px) {
  .main { grid-template-columns: 1fr; grid-template-rows: minmax(0, 1fr) minmax(0, 1fr); }
  .col + .col { border-left: 0; border-top: 1px solid var(--line); }
}
"#;

/// The transcript.
pub const TRANSCRIPT: &str = r#"
.transcript { height: 100vh; overflow-y: auto; padding: 32px 24px; background: var(--ink); color: var(--paper);
  font-family: var(--font-sans); font-size: 16px; line-height: 1.65; }
.transcript-body { max-width: 720px; margin-inline: 0; }
.transcript .refine-text { font-size: 18px; }
.transcript .latest { position: fixed; }
"#;

/// The sheet of the overlay page.
pub fn overlay() -> String {
    [BASE, OVERLAY].concat()
}

/// The sheet of the dashboard page.
pub fn dashboard() -> String {
    [BASE, TOKENS, MOTION, PARTS, DASHBOARD].concat()
}

/// The sheet of the transcript page.
pub fn transcript() -> String {
    [BASE, TOKENS, MOTION, PARTS, TRANSCRIPT].concat()
}

/// The class that carries the colour of a language.
pub fn lang_class(lang: &str) -> &'static str {
    match lang {
        "ja" => "c-ja",
        "en" => "c-en",
        "zh" => "c-zh",
        "ko" => "c-ko",
        "yue" => "c-yue",
        _ => "c-xx",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_sheet_names_a_host_or_fetches_anything() {
        for sheet in [overlay(), dashboard(), transcript()] {
            for needle in ["http:", "https:", "//", "@import", "url("] {
                assert!(!sheet.contains(needle), "{needle}");
            }
        }
    }

    #[test]
    fn the_shared_rule_leaves_the_ground_transparent_and_the_overlay_carries_no_ink() {
        assert!(BASE.contains("background: transparent"));
        assert!(!overlay().contains("var(--ink"));
        assert!(dashboard().contains("var(--ink)"));
    }

    #[test]
    fn the_four_motions_stop_under_reduced_motion() {
        for name in ["pulse", "blink", "rise"] {
            assert!(
                dashboard().contains(&format!("@keyframes {name}")),
                "{name}"
            );
        }
        assert!(dashboard().contains("transition: color 160ms"));
        assert!(dashboard().contains("prefers-reduced-motion: reduce"));
        assert!(dashboard().contains("animation: none !important; transition: none !important"));
    }

    #[test]
    fn a_language_has_a_class_and_the_rest_share_one() {
        assert_eq!(lang_class("ja"), "c-ja");
        assert_eq!(lang_class("yue"), "c-yue");
        assert_eq!(lang_class("fr"), "c-xx");
        assert_eq!(lang_class(""), "c-xx");
    }
}
