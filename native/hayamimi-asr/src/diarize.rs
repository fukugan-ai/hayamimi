//! Who spoke when, in one bounded stretch of audio (a refine group).
//!
//! sherpa-onnx's offline diarizer cuts the stretch into speech turns
//! (pyannote segmentation), embeds each one (CAM++) and clusters the
//! embeddings (FastClustering). The speakers it finds are local to the call:
//! `0`, `1`, ... mean nothing in the next group. [`remap_group`] maps them onto
//! the speakers of the session ([`crate::speaker::Centroids`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};

use crate::audio::SAMPLE_RATE;
use crate::embedder;
use crate::error::AsrError;
use crate::speaker::{Centroids, Match, Source};

const SEGMENTATION_DIR: &str = "sherpa-onnx-pyannote-segmentation-3-0";
const SEGMENTATION_FILE: &str = "model.onnx";
/// Threads of the segmentation and of the embedding.
const THREADS: i32 = 2;
/// Turns shorter than this are not worth a recognizer call of their own.
pub const MIN_TURN_SECONDS: f64 = 0.3;

/// `FastClusteringConfig.threshold`: lower merges more readily into fewer speakers.
pub const DEFAULT_THRESHOLD: f32 = 0.5;
/// Speech turns shorter than this (seconds) are dropped after segmentation.
pub const DEFAULT_MIN_DURATION_ON: f32 = 0.3;
/// Silence gaps shorter than this (seconds) are bridged inside a turn.
pub const DEFAULT_MIN_DURATION_OFF: f32 = 0.5;

/// One speech turn of a group: a local speaker and where the turn runs, in seconds from the start of the audio given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Turn {
    pub local: i32,
    pub start: f32,
    pub end: f32,
}

/// The knobs of the diarizer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiarizeSettings {
    pub threshold: f32,
    /// `-1` lets the clustering choose how many speakers there are.
    pub num_clusters: i32,
    pub min_duration_on: f32,
    pub min_duration_off: f32,
}

impl Default for DiarizeSettings {
    fn default() -> Self {
        DiarizeSettings {
            threshold: DEFAULT_THRESHOLD,
            num_clusters: -1,
            min_duration_on: DEFAULT_MIN_DURATION_ON,
            min_duration_off: DEFAULT_MIN_DURATION_OFF,
        }
    }
}

/// Where the segmentation model is.
pub fn segmentation_path(models: &Path) -> PathBuf {
    models.join(SEGMENTATION_DIR).join(SEGMENTATION_FILE)
}

fn utf8(path: &Path) -> Result<String, AsrError> {
    if !path.is_file() {
        return Err(AsrError::MissingModel(path.to_path_buf()));
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or(AsrError::Create("diarizer (a model path is not UTF-8)"))
}

pub struct GroupDiarizer {
    diarizer: OfflineSpeakerDiarization,
    segmentation: String,
    embedding: String,
    settings: DiarizeSettings,
    /// The `num_clusters` the diarizer's configuration holds now.
    live_clusters: i32,
    sample_rate: usize,
}

impl GroupDiarizer {
    /// Opens the diarizer under `models`. A missing model file is refused
    /// first: the C library ends the whole process on a missing model.
    pub fn open(models: &Path, settings: DiarizeSettings) -> Result<Self, AsrError> {
        let segmentation = utf8(&segmentation_path(models))?;
        let embedding = utf8(&embedder::model_path(models))?;
        let config = build_config(&segmentation, &embedding, &settings, settings.num_clusters);
        let diarizer = OfflineSpeakerDiarization::create(&config)
            .ok_or(AsrError::Create("speaker diarizer"))?;
        let sample_rate = usize::try_from(diarizer.sample_rate())
            .map_err(|_| AsrError::Overflow("the diarizer's sample rate"))?;
        Ok(GroupDiarizer {
            diarizer,
            segmentation,
            embedding,
            live_clusters: settings.num_clusters,
            settings,
            sample_rate,
        })
    }

    /// The turns of 16 kHz `samples`, sorted by start. Audio too short to hold
    /// speech (under half a second) gives none. `num_clusters` overrides the
    /// configured number of speakers for this call.
    pub fn process(
        &mut self,
        samples: &[f32],
        num_clusters: Option<i32>,
    ) -> Result<Vec<Turn>, AsrError> {
        if samples.len() < self.sample_rate / 2 {
            return Ok(Vec::new());
        }
        let wanted = match num_clusters {
            Some(count) => count,
            None => self.settings.num_clusters,
        };
        if wanted != self.live_clusters {
            let config = build_config(&self.segmentation, &self.embedding, &self.settings, wanted);
            self.diarizer.set_config(&config);
            self.live_clusters = wanted;
        }
        let result = self.diarizer.process(samples).ok_or(AsrError::NoResult)?;
        Ok(result
            .sort_by_start_time()
            .into_iter()
            .map(|s| Turn {
                local: s.speaker,
                start: s.start,
                end: s.end,
            })
            .collect())
    }
}

fn build_config(
    segmentation: &str,
    embedding: &str,
    settings: &DiarizeSettings,
    num_clusters: i32,
) -> OfflineSpeakerDiarizationConfig {
    OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(segmentation.to_owned()),
                ..OfflineSpeakerSegmentationPyannoteModelConfig::default()
            },
            num_threads: THREADS,
            ..OfflineSpeakerSegmentationModelConfig::default()
        },
        embedding: SpeakerEmbeddingExtractorConfig {
            model: Some(embedding.to_owned()),
            num_threads: THREADS,
            ..SpeakerEmbeddingExtractorConfig::default()
        },
        clustering: FastClusteringConfig {
            num_clusters,
            threshold: settings.threshold,
            ..FastClusteringConfig::default()
        },
        min_duration_on: settings.min_duration_on,
        min_duration_off: settings.min_duration_off,
    }
}

/// A turn in samples of the group's audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleTurn {
    pub local: i32,
    pub start: usize,
    pub end: usize,
}

/// The turns of at least [`MIN_TURN_SECONDS`], judged on the seconds the diarizer gave.
pub fn long_enough(raw: &[Turn]) -> Vec<Turn> {
    raw.iter()
        .filter(|turn| f64::from(turn.end) - f64::from(turn.start) >= MIN_TURN_SECONDS)
        .copied()
        .collect()
}

/// The turns as sample ranges of audio of `len` samples. A range lies inside
/// the audio and is never reversed; one that starts past the end is empty.
pub fn sample_turns(turns: &[Turn], len: usize) -> Vec<SampleTurn> {
    let rate = f64::from(SAMPLE_RATE);
    turns
        .iter()
        .map(|turn| {
            let start = ((f64::from(turn.start) * rate).round().max(0.0) as usize).min(len);
            let end = ((f64::from(turn.end) * rate).round().max(0.0) as usize).clamp(start, len);
            SampleTurn {
                local: turn.local,
                start,
                end,
            }
        })
        .collect()
}

/// The local speakers of `turns`, in ascending order.
pub fn local_speakers(turns: &[SampleTurn]) -> Vec<i32> {
    let mut ids: Vec<i32> = turns.iter().map(|t| t.local).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The audio of local speaker `local`: its turns, one after the other.
pub fn speaker_audio(audio: &[f32], turns: &[SampleTurn], local: i32) -> Vec<f32> {
    turns
        .iter()
        .filter(|t| t.local == local)
        .flat_map(|t| audio[t.start..t.end].iter().copied())
        .collect()
}

/// Seconds of speech of local speaker `local`.
pub fn speaker_seconds(turns: &[SampleTurn], local: i32) -> f64 {
    let samples: usize = turns
        .iter()
        .filter(|t| t.local == local)
        .map(|t| t.end - t.start)
        .sum();
    samples as f64 / f64::from(SAMPLE_RATE)
}

/// One local speaker of a group with the embedding of its speech.
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    pub local: i32,
    pub embedding: Vec<f32>,
    pub seconds: f64,
}

/// How a group's clusters are mapped onto the session's speakers.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RemapOptions {
    /// A cluster with less speech than this (seconds) only looks and never changes a centroid. `0.0` is off.
    pub min_update_seconds: f64,
    /// Map the clusters together, so that two of them never take one speaker.
    pub joint: bool,
    /// A speaker that has matched once is not a target.
    pub exclude_provisional: bool,
}

/// Maps each local cluster of one group onto a session speaker and returns
/// `(local, label)`. A short cluster takes the speaker it looks like, or
/// `majority` (the group's fast-path label) when there is none. Afterwards
/// speakers that have drifted together are merged, when merging is on.
pub fn remap_group(
    centroids: &mut Centroids,
    clusters: &[Cluster],
    majority: &str,
    options: RemapOptions,
) -> Vec<(i32, String)> {
    let remap = Some(centroids.remap_threshold());
    let mut labels: BTreeMap<i32, String> = BTreeMap::new();
    let (short, long): (Vec<&Cluster>, Vec<&Cluster>) = clusters
        .iter()
        .partition(|c| options.min_update_seconds > 0.0 && c.seconds < options.min_update_seconds);
    for cluster in short {
        let look = Match {
            update: false,
            threshold: remap,
            source: Source::Remap,
            exclude_provisional: options.exclude_provisional,
        };
        let label = match centroids.match_embedding(&cluster.embedding, look) {
            Some(found) => found,
            None => majority.to_owned(),
        };
        labels.insert(cluster.local, label);
    }
    if options.joint {
        let views: Vec<&[f32]> = long.iter().map(|c| c.embedding.as_slice()).collect();
        let found = centroids.match_embeddings_joint(&views, true, remap, Source::Remap);
        for (cluster, label) in long.iter().zip(found) {
            labels.extend(label.map(|l| (cluster.local, l)));
        }
    } else {
        for cluster in long {
            let take = Match {
                update: true,
                threshold: remap,
                source: Source::Remap,
                exclude_provisional: options.exclude_provisional,
            };
            let label = centroids.match_embedding(&cluster.embedding, take);
            labels.extend(label.map(|l| (cluster.local, l)));
        }
    }
    centroids.maybe_merge_centroids();
    clusters
        .iter()
        .filter_map(|c| labels.remove(&c.local).map(|l| (c.local, l)))
        .collect()
}

/// What the tests that open the real models and read the two-speaker recording share.
#[cfg(test)]
pub(crate) mod material {
    use std::path::{Path, PathBuf};

    use crate::audio::read_wav;
    use crate::embedder;

    use super::segmentation_path;

    /// The recording, relative to the directory that holds `testdata/`.
    const RECORDING: &str = "testdata/two_speakers.wav";

    /// The first `models/` at or above this crate that holds both the segmentation
    /// and the embedding model. Model files are not in git, so a worktree reads
    /// the main checkout's.
    pub(crate) fn models_dir() -> Result<PathBuf, String> {
        let start = env!("CARGO_MANIFEST_DIR");
        Path::new(start)
            .ancestors()
            .map(|directory| directory.join("models"))
            .find(|models| {
                segmentation_path(models).is_file() && embedder::model_path(models).is_file()
            })
            .ok_or_else(|| {
                format!("no models/ with the segmentation and embedding models at or above {start}")
            })
    }

    /// The command that cuts the recording from the AMI meetings of `testdata/eval_diar`.
    const MAKE: &str = "cd native && cargo run --release -p hayamimi-eval -- two-speakers --eval-diar ../testdata/eval_diar --out ../testdata";

    /// The samples of `testdata/two_speakers.wav`, found like the models. The
    /// recording is not in git; a missing one is an error, never a skip.
    pub(crate) fn recording() -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let start = env!("CARGO_MANIFEST_DIR");
        let wav = Path::new(start)
            .ancestors()
            .map(|directory| directory.join(RECORDING))
            .find(|wav| wav.is_file())
            .ok_or_else(|| format!("no {RECORDING} at or above {start}; make it with: {MAKE}"))?;
        Ok(read_wav(&wav)?)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn turn(local: i32, start: f32, end: f32) -> Turn {
        Turn { local, start, end }
    }

    #[test]
    fn a_missing_segmentation_model_is_refused_before_the_c_library_is_called() {
        let said =
            GroupDiarizer::open(Path::new("no-such-models"), DiarizeSettings::default()).err();
        assert!(matches!(said, Some(AsrError::MissingModel(_))), "{said:?}");
    }

    #[test]
    fn the_segmentation_model_is_in_its_own_directory() {
        let path = segmentation_path(Path::new("models"));
        assert!(path.ends_with("sherpa-onnx-pyannote-segmentation-3-0/model.onnx"));
    }

    #[test]
    fn the_defaults_are_the_measured_ones() {
        let settings = DiarizeSettings::default();
        assert_eq!(settings.num_clusters, -1);
        assert!((settings.threshold - 0.5).abs() < 1e-6);
        assert!((settings.min_duration_on - 0.3).abs() < 1e-6);
        assert!((settings.min_duration_off - 0.5).abs() < 1e-6);
    }

    #[test]
    fn turns_shorter_than_three_tenths_of_a_second_are_dropped() {
        let raw = [turn(0, 0.0, 0.29), turn(1, 1.0, 1.5), turn(0, 2.0, 3.0)];
        let locals: Vec<i32> = long_enough(&raw).iter().map(|t| t.local).collect();
        assert_eq!(locals, [1, 0]);
    }

    #[test]
    fn a_turn_a_little_over_three_tenths_is_kept_and_a_quarter_second_is_not() {
        // The values are exact in binary, so the difference is not rounded.
        assert_eq!(long_enough(&[turn(0, 0.25, 0.5625)]).len(), 1);
        assert!(long_enough(&[turn(0, 0.25, 0.5)]).is_empty());
    }

    #[test]
    fn a_turn_is_clipped_to_the_audio() {
        let ranges = sample_turns(&[turn(0, 0.5, 5.0)], 16_000 * 2);
        assert_eq!(
            ranges,
            [SampleTurn {
                local: 0,
                start: 8_000,
                end: 32_000
            }]
        );
    }

    #[test]
    fn a_turn_past_the_end_of_the_audio_is_empty_and_not_reversed() {
        let ranges = sample_turns(&[turn(0, 3.0, 4.0)], 16_000 * 2);
        assert_eq!(
            ranges,
            [SampleTurn {
                local: 0,
                start: 32_000,
                end: 32_000
            }]
        );
    }

    #[test]
    fn no_turns_make_no_sample_turns() {
        assert!(sample_turns(&[], 16_000).is_empty());
        assert!(long_enough(&[]).is_empty());
    }

    #[test]
    fn local_speakers_are_sorted_and_distinct() {
        let t = |local| SampleTurn {
            local,
            start: 0,
            end: 1,
        };
        assert_eq!(local_speakers(&[t(2), t(0), t(2), t(1)]), [0, 1, 2]);
        assert!(local_speakers(&[]).is_empty());
    }

    #[test]
    fn a_speakers_audio_is_its_turns_in_order() {
        let audio: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let turns = [
            SampleTurn {
                local: 0,
                start: 10,
                end: 12,
            },
            SampleTurn {
                local: 1,
                start: 50,
                end: 55,
            },
            SampleTurn {
                local: 0,
                start: 60,
                end: 63,
            },
        ];
        assert_eq!(
            speaker_audio(&audio, &turns, 0),
            [10.0, 11.0, 60.0, 61.0, 62.0]
        );
        assert!(speaker_audio(&audio, &turns, 7).is_empty());
        assert!((speaker_seconds(&turns, 0) - 5.0 / 16_000.0).abs() < 1e-12);
    }

    fn cluster(local: i32, v: &[f32], seconds: f64) -> Cluster {
        let length = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        Cluster {
            local,
            embedding: v.iter().map(|x| x / length).collect(),
            seconds,
        }
    }

    fn two_speakers() -> Centroids {
        let mut centroids = Centroids::default();
        centroids.label(&cluster(0, &[1.0, 0.0], 1.0).embedding, Source::Fast);
        centroids.label(&cluster(0, &[0.0, 1.0], 1.0).embedding, Source::Fast);
        centroids
    }

    #[test]
    fn clusters_take_the_speakers_they_look_like() {
        let mut centroids = two_speakers();
        let clusters = [cluster(0, &[0.0, 1.0], 2.0), cluster(1, &[1.0, 0.05], 2.0)];
        let labels = remap_group(&mut centroids, &clusters, "S1", RemapOptions::default());
        assert_eq!(labels, [(0, String::from("S2")), (1, String::from("S1"))]);
    }

    #[test]
    fn joint_remap_keeps_two_clusters_on_two_speakers() {
        let mut centroids = Centroids::new(crate::speaker::Config {
            remap_threshold: Some(0.02),
            ..Default::default()
        });
        centroids.label(&cluster(0, &[1.0, 0.0], 1.0).embedding, Source::Fast);
        centroids.label(&cluster(0, &[0.0, 1.0], 1.0).embedding, Source::Fast);
        let clusters = [cluster(0, &[0.95, 0.05], 2.0), cluster(1, &[0.9, 0.1], 2.0)];
        let options = RemapOptions {
            joint: true,
            ..RemapOptions::default()
        };
        let labels = remap_group(&mut centroids, &clusters, "S1", options);
        assert_eq!(labels, [(0, String::from("S1")), (1, String::from("S2"))]);
    }

    #[test]
    fn a_short_cluster_only_looks_and_otherwise_takes_the_majority() {
        let mut centroids = two_speakers();
        let counts_before = centroids.centroid_summary();
        let clusters = [cluster(0, &[0.0, 1.0], 0.5), cluster(1, &[-1.0, -1.0], 0.5)];
        let options = RemapOptions {
            min_update_seconds: 1.0,
            ..RemapOptions::default()
        };
        let labels = remap_group(&mut centroids, &clusters, "S1", options);
        assert_eq!(labels, [(0, String::from("S2")), (1, String::from("S1"))]);
        assert_eq!(centroids.centroid_summary(), counts_before);
    }

    #[test]
    fn a_cluster_that_matches_nothing_opens_a_new_speaker() {
        let mut centroids = two_speakers();
        let clusters = [cluster(0, &[-1.0, -1.0], 2.0)];
        let labels = remap_group(&mut centroids, &clusters, "S1", RemapOptions::default());
        assert_eq!(labels, [(0, String::from("S3"))]);
        assert_eq!(
            centroids.centroid_open_counts().get("remap").copied(),
            Some(1)
        );
    }

    #[test]
    fn no_clusters_make_no_labels() {
        let mut centroids = two_speakers();
        assert!(remap_group(&mut centroids, &[], "S1", RemapOptions::default()).is_empty());
    }

    type Check = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn group_diarizer_missing_model_raises_filenotfound() {
        let models = Path::new("no-such-models");
        let said = GroupDiarizer::open(models, DiarizeSettings::default()).err();
        // The error names the segmentation model that was looked for.
        assert!(
            matches!(&said, Some(AsrError::MissingModel(path)) if *path == segmentation_path(models)),
            "{said:?}"
        );
    }

    #[test]
    #[ignore = "opens the segmentation and embedding models"]
    fn group_diarizer_short_buffer_returns_empty() -> Check {
        crate::check_library()?;
        let mut diarizer =
            GroupDiarizer::open(&material::models_dir()?, DiarizeSettings::default())?;
        // 0.1 s of silence is under the half second that can hold speech.
        let samples = vec![0.0_f32; diarizer.sample_rate / 10];
        assert!(diarizer.process(&samples, None)?.is_empty());
        Ok(())
    }

    #[test]
    #[ignore = "opens the real models and reads testdata/two_speakers.wav, which is not in git"]
    fn group_diarizer_finds_multiple_speakers_on_two_speaker_audio() -> Check {
        crate::check_library()?;
        let samples = material::recording()?;
        let mut diarizer =
            GroupDiarizer::open(&material::models_dir()?, DiarizeSettings::default())?;
        let turns = diarizer.process(&samples, None)?;
        assert!(turns.len() >= 2, "{turns:?}");
        let speakers: BTreeSet<i32> = turns.iter().map(|turn| turn.local).collect();
        // The recording is of two distinct speakers.
        assert!(speakers.len() >= 2, "{speakers:?}");
        assert!(
            turns.windows(2).all(|pair| pair[0].start <= pair[1].start),
            "{turns:?}"
        );
        assert!(turns.iter().all(|turn| turn.end > turn.start), "{turns:?}");
        Ok(())
    }
}
