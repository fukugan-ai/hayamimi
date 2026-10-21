//! The audio a refine group puts into the re-cluster pool when the group has
//! one speaker or the local diarizer declined.
//!
//! Every group must give the pool one entry, or its label never takes part in
//! the rewrite and the session splits into two label spaces. A group that has
//! turns gives exactly its speech; a group without turns gives its whole
//! buffer, because an entry from noisy audio is better than none.

use std::ops::Range;

use crate::diarize::SampleTurn;

/// The audio to embed for one group, and the seconds of speech it stands for.
///
/// An empty `audio` means there is nothing to embed; the caller adds no pool
/// entry then.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolAudio {
    pub audio: Vec<f32>,
    pub seconds: f64,
}

/// Picks the pool audio of a group.
///
/// `turns` are the diarizer's own speech turns, already filtered to the
/// minimum turn length and lying inside `buf` (see `diarize::sample_turns`).
/// When there are none, the whole `buf` is taken; its length in seconds is
/// that of `group_span` (sample indexes into the original recording) when the
/// caller has one, else that of `buf`. `sample_rate` is in hertz and not zero.
pub fn pool_audio_for_group(
    buf: &[f32],
    sample_rate: u32,
    turns: &[SampleTurn],
    group_span: Option<Range<usize>>,
) -> PoolAudio {
    if turns.is_empty() {
        whole_buffer(buf, sample_rate, group_span)
    } else {
        turn_speech(buf, sample_rate, turns)
    }
}

/// The turns one after the other. The caller owns the result, so the copy is the point.
fn turn_speech(buf: &[f32], sample_rate: u32, turns: &[SampleTurn]) -> PoolAudio {
    let audio: Vec<f32> = turns
        .iter()
        .flat_map(|turn| buf[turn.start..turn.end].iter().copied())
        .collect();
    PoolAudio {
        seconds: seconds_of(audio.len(), sample_rate),
        audio,
    }
}

/// The whole buffer. The caller owns the result, so the copy is the point.
fn whole_buffer(buf: &[f32], sample_rate: u32, group_span: Option<Range<usize>>) -> PoolAudio {
    let samples = match group_span {
        Some(span) => span.end.saturating_sub(span.start),
        None => buf.len(),
    };
    PoolAudio {
        audio: buf.to_vec(),
        seconds: seconds_of(samples, sample_rate),
    }
}

fn seconds_of(samples: usize, sample_rate: u32) -> f64 {
    samples as f64 / f64::from(sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0.0, 1.0, ..` as samples, so that a sample says where it came from.
    fn ramp(len: u16) -> Vec<f32> {
        (0..len).map(f32::from).collect()
    }

    fn turn(start: usize, end: usize) -> SampleTurn {
        SampleTurn {
            local: 0,
            start,
            end,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn pool_audio_for_group_uses_diarizer_turns_when_present() {
        let buf = ramp(100);
        let turns = [turn(10, 20), turn(50, 65)];
        let pooled = pool_audio_for_group(&buf, 10, &turns, None);
        let expected: Vec<f32> = (10_u16..20).chain(50..65).map(f32::from).collect();
        assert_eq!(pooled.audio, expected);
        // 10 samples + 15 samples at 10 Hz -> 1.0 s + 1.5 s
        assert!(close(pooled.seconds, 2.5), "{}", pooled.seconds);
    }

    #[test]
    fn pool_audio_for_group_falls_back_to_full_buffer_when_declined() {
        let buf = ramp(20);
        let pooled = pool_audio_for_group(&buf, 10, &[], None);
        assert_eq!(pooled.audio, buf);
        assert!(close(pooled.seconds, 2.0), "{}", pooled.seconds);
    }

    #[test]
    fn pool_audio_for_group_declined_duration_prefers_group_span_over_buffer_length() {
        let buf = ramp(20);
        let pooled = pool_audio_for_group(&buf, 10, &[], Some(100..135));
        // (135 - 100) / 10, not buf.len() / 10 == 2.0
        assert!(close(pooled.seconds, 3.5), "{}", pooled.seconds);
    }

    #[test]
    fn pool_audio_for_group_empty_group_yields_empty_audio() {
        let pooled = pool_audio_for_group(&[], 10, &[], None);
        assert!(pooled.audio.is_empty());
    }
}
