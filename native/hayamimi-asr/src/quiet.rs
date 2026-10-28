//! A cheap cue that the voice has fallen quiet, taken from the level of each
//! window.
//!
//! The early detector tells that a stretch of voice ended only after the speech
//! probability of its model has decayed, which takes a few hundred
//! milliseconds after the last sound on the test recordings. The level of the
//! windows falls at once. [`Quiet`] watches the level and says, once for each
//! stretch of quiet that follows voice, that the voice fell quiet, so that a
//! reading of the voice so far can start before the detector's notice comes.
//! It never ends an utterance: what ends one is still the detectors.

use std::collections::VecDeque;

/// Quiet windows in a row that make a fall: 3 windows, 96 ms.
pub const HANG_WINDOWS: usize = 3;
/// How far below the loudest recent window a window is quiet, in dB. A room
/// whose noise sits closer than this to the speech never falls quiet, and the
/// reading simply does not start early.
pub const DROP_DB: f32 = 20.0;
/// The windows over which the loudest level is kept: 32 windows, about a second.
const PEAK_WINDOWS: usize = 32;
/// A level at or below this is quiet whatever came before, in dBFS.
const FLOOR_DB: f32 = -60.0;
/// The level of a window of digital silence, in dBFS.
const SILENT_DB: f32 = -140.0;

/// The level of a window in dBFS (the mean power, so a full-scale square wave is 0).
fn level_db(window: &[f32]) -> f32 {
    if window.is_empty() {
        return SILENT_DB;
    }
    let power = window.iter().map(|sample| sample * sample).sum::<f32>() / window.len() as f32;
    (10.0 * power.max(1e-14).log10()).max(SILENT_DB)
}

/// What the levels of the windows say about the voice.
#[derive(Debug, Clone, Default)]
pub struct Quiet {
    /// The levels of the last windows, oldest first.
    recent: VecDeque<f32>,
    /// Quiet windows in a row.
    run: usize,
    /// Whether a loud window came since the last fall.
    armed: bool,
    /// The sample at which the last loud window ended.
    loud_end: usize,
}

impl Quiet {
    /// The loudest level of the last windows, before the window now coming.
    fn peak(&self) -> f32 {
        self.recent.iter().copied().fold(SILENT_DB, f32::max)
    }

    fn is_quiet(&self, level: f32) -> bool {
        level <= FLOOR_DB || level < self.peak() - DROP_DB
    }

    fn remember(&mut self, level: f32) {
        if self.recent.len() == PEAK_WINDOWS {
            self.recent.pop_front();
        }
        self.recent.push_back(level);
    }

    /// Takes the window that ends at sample `end`. Returns `true` once, at the
    /// window that completes [`HANG_WINDOWS`] quiet windows after a loud one.
    pub fn observe(&mut self, window: &[f32], end: usize) -> bool {
        let level = level_db(window);
        let quiet = self.is_quiet(level);
        self.remember(level);
        if !quiet {
            self.run = 0;
            self.armed = true;
            self.loud_end = end;
            return false;
        }
        self.run += 1;
        let fell = self.armed && self.run == HANG_WINDOWS;
        if fell {
            self.armed = false;
        }
        fell
    }

    /// [`Quiet::observe`] for a run that may not look at the windows at all:
    /// when `enabled` is false the window is not taken, so no fall is said and
    /// no level is kept.
    pub fn watch(&mut self, enabled: bool, window: &[f32], end: usize) -> bool {
        enabled && self.observe(window, end)
    }

    /// The sample at which the last loud window ended. A reading of the audio
    /// up to a sample at or after it has heard no voice that the window
    /// levels call loud beyond what it took in.
    pub fn loud_end(&self) -> usize {
        self.loud_end
    }

    /// Forgets everything seen, as at the start of a session.
    pub fn clear(&mut self) {
        *self = Quiet::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::WINDOW;

    /// The amplitude of the voice in these tests: about -10.5 dBFS.
    const VOICE: f32 = 0.3;

    /// A window of a steady tone at `amplitude`.
    fn tone(amplitude: f32) -> Vec<f32> {
        (0..WINDOW)
            .map(|at| if at % 2 == 0 { amplitude } else { -amplitude })
            .collect()
    }

    fn silence() -> Vec<f32> {
        vec![0.0; WINDOW]
    }

    /// The amplitude of a tone that is `db` below the voice.
    fn below_voice(db: f32) -> f32 {
        VOICE * 10f32.powf(-db / 20.0)
    }

    /// Feeds `windows` and returns the window numbers (from 1) at which it fell.
    fn falls(windows: &[Vec<f32>]) -> Vec<usize> {
        let mut quiet = Quiet::default();
        let mut found = Vec::new();
        for (at, window) in windows.iter().enumerate() {
            if quiet.observe(window, (at + 1) * WINDOW) {
                found.push(at + 1);
            }
        }
        found
    }

    /// `speech` windows of the voice, then `rest`.
    fn run_of(speech: usize, rest: &[Vec<f32>]) -> Vec<Vec<f32>> {
        let mut windows = vec![tone(VOICE); speech];
        windows.extend_from_slice(rest);
        windows
    }

    /// T-BR-43-023: the window numbers are written out (10 of voice and 3 of quiet),
    /// not taken from the constant, which is one place that can drift.
    #[test]
    fn digital_silence_after_voice_falls_at_the_third_quiet_window() {
        let windows = run_of(10, &vec![silence(); 6]);
        assert_eq!(falls(&windows), vec![13]);
    }

    /// T-BR-43-024: a dip of 2 windows is no fall.
    #[test]
    fn a_dip_shorter_than_the_hang_is_not_a_fall() {
        let mut windows = run_of(10, &vec![silence(); 2]);
        windows.extend(vec![tone(VOICE); 10]);
        assert!(falls(&windows).is_empty());
    }

    /// T-BR-43-025
    #[test]
    fn a_fall_is_said_once_for_each_stretch_of_quiet() {
        let mut windows = run_of(10, &vec![silence(); 8]);
        windows.extend(vec![tone(VOICE); 10]);
        windows.extend(vec![silence(); 8]);
        assert_eq!(falls(&windows), vec![13, 31]);
    }

    /// T-BR-43-026
    #[test]
    fn quiet_before_any_voice_is_not_a_fall() {
        assert!(falls(&vec![silence(); 20]).is_empty());
    }

    /// T-BR-43-027
    #[test]
    fn a_room_noise_20_db_below_the_voice_is_quiet() {
        // 0.3 is about -10.5 dBFS; 0.01 is -40 dBFS, 29.5 dB below.
        let windows = run_of(10, &vec![tone(0.01); 6]);
        assert_eq!(falls(&windows), vec![13]);
    }

    /// T-BR-43-028
    #[test]
    fn a_room_noise_closer_to_the_voice_is_never_a_fall() {
        // 0.1 is -20 dBFS, 9.5 dB below the voice and above the floor.
        let windows = run_of(10, &vec![tone(0.1); 12]);
        assert!(falls(&windows).is_empty());
    }

    /// T-BR-43-029
    #[test]
    fn the_end_of_the_last_loud_window_is_kept() {
        let mut quiet = Quiet::default();
        for at in 1..=10 {
            quiet.observe(&tone(VOICE), at * WINDOW);
        }
        for at in 11..=14 {
            quiet.observe(&silence(), at * WINDOW);
        }
        assert_eq!(quiet.loud_end(), 10 * WINDOW);
        quiet.observe(&tone(VOICE), 15 * WINDOW);
        assert_eq!(quiet.loud_end(), 15 * WINDOW);
    }

    /// T-BR-43-030: the voice is the reference for 32 windows (written out), no
    /// more and no less. A noise 30 dB below it is quiet while the voice is
    /// remembered; once the voice is forgotten the noise is the reference, and a
    /// fall from the noise into silence is a fall of its own.
    #[test]
    fn the_peak_is_remembered_for_32_windows_and_then_forgotten() {
        let noise_then_silence = |noise: usize| {
            let mut windows = run_of(10, &vec![tone(0.01); noise]);
            windows.extend(vec![silence(); 6]);
            windows
        };
        // 24 noise windows: the voice is still remembered when the silence comes.
        assert_eq!(falls(&noise_then_silence(24)), vec![13]);
        // 40 noise windows: the voice left the memory at window 43.
        assert_eq!(falls(&noise_then_silence(40)), vec![13, 53]);
    }

    /// T-BR-43-031
    #[test]
    fn clear_forgets_the_voice_so_the_next_quiet_is_no_fall() {
        let mut quiet = Quiet::default();
        for at in 1..=10 {
            quiet.observe(&tone(VOICE), at * WINDOW);
        }
        quiet.clear();
        let fell = (11..=16).any(|at| quiet.observe(&silence(), at * WINDOW));
        assert!(!fell);
        assert_eq!(quiet.loud_end(), 0);
    }

    /// T-BR-43-032: the levels are written out in dBFS, not taken from the constant.
    #[test]
    fn the_level_of_digital_silence_is_the_lowest_and_a_full_square_wave_is_zero() {
        assert_eq!(level_db(&silence()), -140.0);
        assert!(level_db(&tone(1.0)).abs() < 1e-3);
        assert!((level_db(&tone(VOICE)) + 10.4576).abs() < 1e-2);
        assert_eq!(level_db(&[]), -140.0);
    }

    /// T-BR-43-034: the drop of 20 dB, written out. A noise 19.5 dB below the
    /// voice is not quiet, one 20.5 dB below is.
    #[test]
    fn the_drop_that_makes_a_window_quiet_is_20_db() {
        let not_enough = run_of(10, &vec![tone(below_voice(19.5)); 12]);
        assert!(falls(&not_enough).is_empty());
        let enough = run_of(10, &vec![tone(below_voice(20.5)); 6]);
        assert_eq!(falls(&enough), vec![13]);
    }

    /// T-BR-43-036: a run without `predecode` does not look at the windows at
    /// all. It says no fall and keeps no level, so turning it on later finds no
    /// voice behind it. The same windows with it on fall at the third quiet one.
    #[test]
    fn a_run_without_predecode_says_no_fall_and_keeps_no_level() {
        let windows = run_of(10, &vec![silence(); 6]);
        let mut off = Quiet::default();
        let mut fell_off = 0;
        for (at, window) in windows.iter().enumerate() {
            if off.watch(false, window, (at + 1) * WINDOW) {
                fell_off += 1;
            }
        }
        assert_eq!(fell_off, 0);
        assert_eq!(off.loud_end(), 0);
        // Turned on after the voice: the quiet that follows has no voice before it.
        let later = (17..=22).any(|at| off.watch(true, &silence(), at * WINDOW));
        assert!(!later);
        let mut on = Quiet::default();
        let mut fell_on = Vec::new();
        for (at, window) in windows.iter().enumerate() {
            if on.watch(true, window, (at + 1) * WINDOW) {
                fell_on.push(at + 1);
            }
        }
        assert_eq!(fell_on, vec![13]);
    }

    /// T-BR-43-035: the floor of -60 dBFS, written out. After a voice that is
    /// itself only -54 dBFS, a window at -66 dBFS is quiet (a drop of 12 dB) and
    /// one at -59 dBFS is not.
    #[test]
    fn a_window_at_or_below_minus_60_dbfs_is_quiet_whatever_came_before() {
        let after_soft_voice = |amplitude: f32, count: usize| {
            let mut windows = vec![tone(0.002); 10];
            windows.extend(vec![tone(amplitude); count]);
            windows
        };
        // 0.0005 is -66 dBFS and 0.0011 is -59.2 dBFS.
        assert_eq!(falls(&after_soft_voice(0.0005, 6)), vec![13]);
        assert!(falls(&after_soft_voice(0.0011, 12)).is_empty());
    }
}
