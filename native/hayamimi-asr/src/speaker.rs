//! The speakers of one session, as running centroids of embeddings.
//!
//! Each finished utterance has one embedding. It joins the nearest centroid
//! when the cosine similarity reaches the threshold, and otherwise opens a new
//! speaker `S1`, `S2` and so on. A speaker that has matched only once is
//! provisional and is shown as `S3?` until a second match; that is a display
//! rule only and never changes which centroid an embedding goes to.
//!
//! This module holds no model: it takes embeddings, so the rules can be tested
//! with vectors made by hand. The embedding itself is `embedder`.

use std::collections::BTreeMap;

use crate::hungarian;

/// Cosine similarity at which an utterance joins an existing speaker.
pub const SIM_THRESHOLD: f64 = 0.45;
/// The threshold of the remap of a group's local clusters onto the session's speakers.
pub const REMAP_THRESHOLD: f64 = 0.35;
/// Cosine similarity above which two speakers are folded into one, when merging is on.
pub const MERGE_THRESHOLD: f64 = 0.80;
/// Matches a new speaker needs to be shown without a mark, when hysteresis is on.
pub const HYSTERESIS_MIN_HITS: u32 = 2;
/// Matches a speaker needs to stop being provisional.
pub const PROVISIONAL_CONFIRM_HITS: u32 = 2;
/// Added to a norm so that a zero vector divides to zero, not to not-a-number.
const EPSILON: f64 = 1e-9;

/// The knobs of the labeler. Merging and hysteresis are off by default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    pub threshold: f64,
    /// `None` takes the value of `threshold`.
    pub remap_threshold: Option<f64>,
    pub merge_enabled: bool,
    pub merge_threshold: f64,
    pub hysteresis_enabled: bool,
    pub hysteresis_min_hits: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            threshold: SIM_THRESHOLD,
            remap_threshold: Some(REMAP_THRESHOLD),
            merge_enabled: false,
            merge_threshold: MERGE_THRESHOLD,
            hysteresis_enabled: false,
            hysteresis_min_hits: HYSTERESIS_MIN_HITS,
        }
    }
}

/// Which path opened a centroid, kept for the session summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Unspecified,
    /// One embedding for each finished utterance.
    Fast,
    /// A local cluster of a refine group, mapped onto the session's speakers.
    Remap,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Unspecified => "",
            Source::Fast => "fast",
            Source::Remap => "remap",
        }
    }
}

/// How one embedding is matched.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Match {
    /// `false` only looks: no centroid changes and none opens.
    pub update: bool,
    /// `None` takes the labeler's own threshold.
    pub threshold: Option<f64>,
    pub source: Source,
    /// Skip a speaker that has matched only once.
    pub exclude_provisional: bool,
}

impl Default for Match {
    fn default() -> Self {
        Match {
            update: true,
            threshold: None,
            source: Source::Unspecified,
            exclude_provisional: false,
        }
    }
}

/// One row of the session summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CentroidSummary {
    pub label: String,
    pub source: Source,
    /// Embeddings folded into the centroid, the one that opened it included.
    pub matches: u32,
}

pub struct Centroids {
    threshold: f64,
    remap_threshold: f64,
    merge_enabled: bool,
    merge_threshold: f64,
    hysteresis_enabled: bool,
    hysteresis_min_hits: u32,
    centroids: Vec<Vec<f32>>,
    counts: Vec<u32>,
    confirmed: Vec<bool>,
    /// Centroids merged away, to the centroid that took them over. Their slots stay so that labels already shown keep their numbers.
    alias: BTreeMap<usize, usize>,
    merge_history: BTreeMap<String, String>,
    open_log: Vec<Source>,
}

fn dot(a: &[f32], b: &[f32]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| f64::from(*x) * f64::from(*y))
        .sum()
}

fn norm(a: &[f32]) -> f64 {
    dot(a, a).sqrt()
}

/// `S{index + 1}`.
fn name_of(index: usize) -> String {
    format!("S{}", index + 1)
}

impl Default for Centroids {
    fn default() -> Self {
        Centroids::new(Config::default())
    }
}

impl Centroids {
    pub fn new(config: Config) -> Centroids {
        let remap_threshold = match config.remap_threshold {
            Some(value) => value,
            None => config.threshold,
        };
        Centroids {
            threshold: config.threshold,
            remap_threshold,
            merge_enabled: config.merge_enabled,
            merge_threshold: config.merge_threshold,
            hysteresis_enabled: config.hysteresis_enabled,
            hysteresis_min_hits: config.hysteresis_min_hits,
            centroids: Vec::new(),
            counts: Vec::new(),
            confirmed: Vec::new(),
            alias: BTreeMap::new(),
            merge_history: BTreeMap::new(),
            open_log: Vec::new(),
        }
    }

    /// The threshold of the remap of a group's local clusters.
    pub fn remap_threshold(&self) -> f64 {
        self.remap_threshold
    }

    /// Forgets every speaker, as if the labeler were new.
    pub fn reset(&mut self) {
        self.centroids.clear();
        self.counts.clear();
        self.confirmed.clear();
        self.alias.clear();
        self.merge_history.clear();
        self.open_log.clear();
    }

    fn is_live(&self, index: usize) -> bool {
        !self.alias.contains_key(&index)
    }

    fn live(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.centroids.len()).filter(|i| self.is_live(*i))
    }

    fn similarity(&self, embedding: &[f32], index: usize) -> f64 {
        let centroid = &self.centroids[index];
        dot(embedding, centroid) / (norm(centroid) + EPSILON)
    }

    /// The live centroid nearest to `embedding` and its similarity.
    fn nearest(&self, embedding: &[f32], exclude_provisional: bool) -> Option<(usize, f64)> {
        let mut best: Option<(usize, f64)> = None;
        for index in self.live() {
            if exclude_provisional && self.counts[index] < PROVISIONAL_CONFIRM_HITS {
                continue;
            }
            let sim = self.similarity(embedding, index);
            if sim > best.map_or(-1.0, |(_, found)| found) {
                best = Some((index, sim));
            }
        }
        best
    }

    /// The centroid `embedding` joins at `threshold`, if any.
    fn lookup(&self, embedding: &[f32], threshold: f64, exclude: bool) -> Option<usize> {
        let (index, sim) = self.nearest(embedding, exclude)?;
        (sim >= threshold).then_some(index)
    }

    /// Folds `embedding` into the running mean of centroid `index`.
    fn fold(&mut self, index: usize, embedding: &[f32]) {
        let n = self.counts[index] as f32;
        for (c, e) in self.centroids[index].iter_mut().zip(embedding) {
            *c = (*c * n + *e) / (n + 1.0);
        }
        self.counts[index] += 1;
        if self.hysteresis_enabled
            && !self.confirmed[index]
            && self.counts[index] >= self.hysteresis_min_hits
        {
            self.confirmed[index] = true;
        }
    }

    /// Opens a new speaker on `embedding` and returns its label.
    fn open(&mut self, embedding: &[f32], source: Source) -> String {
        let index = self.centroids.len();
        let has_confirmed = (0..index).any(|i| self.is_live(i) && self.confirmed[i]);
        self.centroids.push(embedding.to_vec());
        self.counts.push(1);
        self.open_log.push(source);
        // With hysteresis, the first speaker has none to fall back to and is confirmed at once.
        self.confirmed
            .push(!self.hysteresis_enabled || !has_confirmed);
        self.label_for(index)
    }

    /// The label of centroid `index`: its own, or, for a speaker not yet
    /// confirmed under hysteresis, that of the nearest confirmed one.
    fn label_for(&self, index: usize) -> String {
        if !self.hysteresis_enabled || self.confirmed[index] {
            return name_of(index);
        }
        match self.nearest_confirmed(index) {
            Some(other) => name_of(other),
            None => name_of(index),
        }
    }

    fn nearest_confirmed(&self, index: usize) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for other in self.live().filter(|i| *i != index && self.confirmed[*i]) {
            let sim = self.similarity(&self.centroids[index], other);
            if sim > best.map_or(-1.0, |(_, found)| found) {
                best = Some((other, sim));
            }
        }
        best.map(|(other, _)| other)
    }

    /// Matches one embedding to a speaker. `None` only when `update` is
    /// `false` and no speaker is near enough.
    pub fn match_embedding(&mut self, embedding: &[f32], options: Match) -> Option<String> {
        let threshold = match options.threshold {
            Some(value) => value,
            None => self.threshold,
        };
        let found = self.lookup(embedding, threshold, options.exclude_provisional);
        match (found, options.update) {
            (Some(index), true) => {
                self.fold(index, embedding);
                Some(self.label_for(index))
            }
            (Some(index), false) => Some(self.label_for(index)),
            (None, true) => Some(self.open(embedding, options.source)),
            (None, false) => None,
        }
    }

    /// The label of one finished utterance: it joins a speaker or opens one.
    pub fn label(&mut self, embedding: &[f32], source: Source) -> String {
        let threshold = self.threshold;
        match self.lookup(embedding, threshold, false) {
            Some(index) => {
                self.fold(index, embedding);
                self.label_for(index)
            }
            None => self.open(embedding, source),
        }
    }

    /// The similarity of each embedding to each live centroid, row by row.
    fn similarity_matrix(&self, embeddings: &[&[f32]], live: &[usize]) -> Vec<f64> {
        embeddings
            .iter()
            .flat_map(|e| live.iter().map(|index| self.similarity(e, *index)))
            .collect()
    }

    /// Matches all the local clusters of one group at once, so that two of
    /// them never take the same speaker when each has another it could take.
    ///
    /// The assignment that maximises the total similarity is found first. A
    /// cluster whose assigned speaker is below `threshold`, and a cluster that
    /// has no assigned speaker, matches by itself as in [`Self::match_embedding`].
    pub fn match_embeddings_joint(
        &mut self,
        embeddings: &[&[f32]],
        update: bool,
        threshold: Option<f64>,
        source: Source,
    ) -> Vec<Option<String>> {
        let threshold = match threshold {
            Some(value) => value,
            None => self.threshold,
        };
        let live: Vec<usize> = self.live().collect();
        let single = Match {
            update,
            threshold: Some(threshold),
            source,
            exclude_provisional: false,
        };
        if embeddings.len() <= 1 || live.is_empty() {
            return embeddings
                .iter()
                .map(|e| self.match_embedding(e, single))
                .collect();
        }
        let sim = self.similarity_matrix(embeddings, &live);
        let negated: Vec<f64> = sim.iter().map(|s| -s).collect();
        let winners: BTreeMap<usize, usize> =
            hungarian::minimize(&negated, embeddings.len(), live.len())
                .into_iter()
                .filter(|(row, col)| sim[row * live.len() + col] >= threshold)
                .map(|(row, col)| (row, live[col]))
                .collect();
        embeddings
            .iter()
            .enumerate()
            .map(|(row, e)| match winners.get(&row) {
                Some(&index) => Some(self.settle(index, e, update)),
                None => self.match_embedding(e, single),
            })
            .collect()
    }

    /// The label of an embedding assigned to centroid `index`, folded in when `update`.
    fn settle(&mut self, index: usize, embedding: &[f32], update: bool) -> String {
        if update {
            self.fold(index, embedding);
        }
        self.label_for(index)
    }

    /// Live speakers that have matched at least [`PROVISIONAL_CONFIRM_HITS`] times.
    pub fn num_confirmed_speakers(&self) -> usize {
        self.live()
            .filter(|i| self.counts[*i] >= PROVISIONAL_CONFIRM_HITS)
            .count()
    }

    /// How many centroids each path opened.
    pub fn centroid_open_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for source in &self.open_log {
            *counts.entry(source.name()).or_insert(0) += 1;
        }
        counts
    }

    /// Every centroid ever opened, merged-away ones included, in opening order.
    pub fn centroid_summary(&self) -> Vec<CentroidSummary> {
        (0..self.centroids.len())
            .map(|i| CentroidSummary {
                label: self.label_for(i),
                source: self.open_log[i],
                matches: self.counts[i],
            })
            .collect()
    }

    /// The centroid a label such as `S3` names, if this labeler has opened it.
    fn index_of(&self, label: &str) -> Option<usize> {
        let index = label
            .strip_prefix('S')?
            .parse::<usize>()
            .ok()?
            .checked_sub(1)?;
        (index < self.counts.len()).then_some(index)
    }

    /// Whether the speaker has matched fewer than [`PROVISIONAL_CONFIRM_HITS`] times.
    pub fn is_provisional(&self, label: &str) -> bool {
        self.index_of(label)
            .is_some_and(|i| self.counts[i] < PROVISIONAL_CONFIRM_HITS)
    }

    /// The label as it is shown: `S3?` while provisional.
    pub fn display_label(&self, label: &str) -> String {
        if self.is_provisional(label) {
            format!("{label}?")
        } else {
            label.to_owned()
        }
    }

    /// How many live speakers are still provisional.
    pub fn provisional_label_count(&self) -> usize {
        self.live()
            .filter(|i| self.counts[*i] < PROVISIONAL_CONFIRM_HITS)
            .count()
    }

    /// Folds speakers together when merging is on; see [`Self::merge_centroids`].
    pub fn maybe_merge_centroids(&mut self) -> BTreeMap<String, String> {
        if !self.merge_enabled {
            return BTreeMap::new();
        }
        self.merge_centroids(None)
    }

    /// The first pair of live speakers, lower number first, whose similarity reaches `threshold`.
    fn first_mergeable(&self, threshold: f64) -> Option<(usize, usize)> {
        let roots: Vec<usize> = self.live().collect();
        for (at, &i) in roots.iter().enumerate() {
            for &j in &roots[at + 1..] {
                let (ci, cj) = (&self.centroids[i], &self.centroids[j]);
                let sim = dot(ci, cj) / (norm(ci) * norm(cj) + EPSILON);
                if sim >= threshold {
                    return Some((i, j));
                }
            }
        }
        None
    }

    /// Folds together every two live speakers whose similarity reaches the
    /// threshold (this labeler's merge threshold when `None`). The lower
    /// number survives. Returns the speakers merged away by this call, each
    /// with the one that took it over.
    pub fn merge_centroids(&mut self, threshold: Option<f64>) -> BTreeMap<String, String> {
        let threshold = match threshold {
            Some(value) => value,
            None => self.merge_threshold,
        };
        let mut merged = BTreeMap::new();
        while let Some((root, other)) = self.first_mergeable(threshold) {
            self.merge_into(root, other);
            merged.insert(name_of(other), name_of(root));
        }
        self.merge_history
            .extend(merged.iter().map(|(old, new)| (old.clone(), new.clone())));
        merged
    }

    fn merge_into(&mut self, root: usize, other: usize) {
        let (n_root, n_other) = (self.counts[root], self.counts[other]);
        let total = (n_root + n_other).max(1) as f32;
        let mut merged: Vec<f32> = self.centroids[root]
            .iter()
            .zip(&self.centroids[other])
            .map(|(r, o)| (*r * n_root as f32 + *o * n_other as f32) / total)
            .collect();
        let length = norm(&merged) + EPSILON;
        for value in &mut merged {
            *value = (f64::from(*value) / length) as f32;
        }
        self.centroids[root] = merged;
        self.counts[root] = n_root + n_other;
        self.alias.insert(other, root);
        self.confirmed[root] = self.confirmed[root] || self.confirmed[other];
        // Keep the history flat: whatever pointed at `other` now points at `root`.
        let gone = name_of(other);
        let survivor = name_of(root);
        for target in self.merge_history.values_mut().filter(|t| **t == gone) {
            target.clone_from(&survivor);
        }
    }

    /// Every merge so far, each old label with the one it ended up under.
    pub fn merge_history(&self) -> BTreeMap<String, String> {
        self.merge_history.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(v: &[f32]) -> Vec<f32> {
        let length = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / length).collect()
    }

    fn labeler(threshold: f64) -> Centroids {
        Centroids::new(Config {
            threshold,
            ..Config::default()
        })
    }

    fn put(c: &mut Centroids, v: &[f32]) -> Option<String> {
        c.match_embedding(&unit(v), Match::default())
    }

    fn put_with(c: &mut Centroids, v: &[f32], options: Match) -> Option<String> {
        c.match_embedding(&unit(v), options)
    }

    fn seen(c: &mut Centroids, v: &[f32]) -> Option<String> {
        put_with(
            c,
            v,
            Match {
                update: false,
                ..Match::default()
            },
        )
    }

    fn from(label: &str) -> Option<String> {
        Some(label.to_owned())
    }

    fn joint(
        c: &mut Centroids,
        vs: &[&[f32]],
        update: bool,
        threshold: f64,
    ) -> Vec<Option<String>> {
        let units: Vec<Vec<f32>> = vs.iter().map(|v| unit(v)).collect();
        let views: Vec<&[f32]> = units.iter().map(Vec::as_slice).collect();
        c.match_embeddings_joint(&views, update, Some(threshold), Source::Remap)
    }

    #[test]
    fn match_embedding_opens_first_speaker() {
        let mut c = labeler(0.45);
        assert_eq!(put(&mut c, &[1.0, 0.0]), from("S1"));
        assert_eq!(c.centroids.len(), 1);
        assert_eq!(c.counts, [1]);
    }

    #[test]
    fn match_embedding_joins_existing_within_threshold() {
        let mut c = labeler(0.9);
        put(&mut c, &[1.0, 0.0]);
        assert_eq!(put(&mut c, &[0.99, 0.01]), from("S1"));
        assert_eq!(c.centroids.len(), 1);
        assert_eq!(c.counts, [2]);
    }

    #[test]
    fn match_embedding_opens_new_speaker_below_threshold() {
        let mut c = labeler(0.9);
        put(&mut c, &[1.0, 0.0]);
        assert_eq!(put(&mut c, &[0.0, 1.0]), from("S2"));
        assert_eq!(c.centroids.len(), 2);
    }

    #[test]
    fn match_embedding_update_false_does_not_mutate_on_hit() {
        let mut c = labeler(0.9);
        put(&mut c, &[1.0, 0.0]);
        assert_eq!(seen(&mut c, &[0.99, 0.01]), from("S1"));
        assert_eq!(c.counts, [1]);
    }

    #[test]
    fn match_embedding_update_false_returns_empty_on_miss() {
        let mut c = labeler(0.9);
        put(&mut c, &[1.0, 0.0]);
        assert_eq!(seen(&mut c, &[0.0, 1.0]), None);
        assert_eq!(c.centroids.len(), 1);
    }

    #[test]
    fn match_embedding_picks_nearest_of_several_centroids() {
        let mut c = labeler(0.5);
        put(&mut c, &[1.0, 0.0, 0.0]);
        put(&mut c, &[0.0, 1.0, 0.0]);
        assert_eq!(put(&mut c, &[0.05, 0.95, 0.0]), from("S2"));
    }

    #[test]
    fn exclude_provisional_skips_a_one_off_centroid_as_a_match_target() {
        let mut c = labeler(0.3);
        put(&mut c, &[1.0, 0.0]);
        put(&mut c, &[0.99, 0.01]);
        put(&mut c, &[0.0, 1.0]);
        let options = Match {
            update: false,
            exclude_provisional: true,
            ..Match::default()
        };
        assert_eq!(put_with(&mut c, &[0.05, 0.95], options), None);
    }

    #[test]
    fn exclude_provisional_still_matches_a_confirmed_centroid() {
        let mut c = labeler(0.3);
        put(&mut c, &[1.0, 0.0]);
        put(&mut c, &[0.99, 0.01]);
        let options = Match {
            update: false,
            exclude_provisional: true,
            ..Match::default()
        };
        assert_eq!(put_with(&mut c, &[0.98, 0.02], options), from("S1"));
    }

    #[test]
    fn exclude_provisional_false_is_a_no_op_default() {
        let mut c = labeler(0.3);
        put(&mut c, &[0.0, 1.0]);
        assert_eq!(seen(&mut c, &[0.05, 0.95]), from("S1"));
    }

    #[test]
    fn joint_remap_single_cluster_matches_independent_match_embedding() {
        let mut c = labeler(0.3);
        put(&mut c, &[1.0, 0.0]);
        let labels = joint(&mut c, &[&[0.99, 0.01]], true, 0.3);
        assert_eq!(labels, [from("S1")]);
    }

    #[test]
    fn joint_remap_no_live_centroids_opens_distinct_new_speakers() {
        let mut c = labeler(0.3);
        let labels = joint(&mut c, &[&[1.0, 0.0], &[0.0, 1.0]], true, 0.3);
        assert_eq!(labels, [from("S1"), from("S2")]);
        assert_eq!(c.centroid_open_counts(), BTreeMap::from([("remap", 2)]));
    }

    #[test]
    fn joint_remap_keeps_two_clusters_on_distinct_speakers_when_both_eligible() {
        let mut c = labeler(0.02);
        put(&mut c, &[1.0, 0.0]);
        put(&mut c, &[0.0, 1.0]);
        let labels = joint(&mut c, &[&[0.95, 0.05], &[0.9, 0.1]], true, 0.02);
        assert_eq!(labels, [from("S1"), from("S2")]);
    }

    #[test]
    fn joint_remap_falls_back_to_independent_match_when_below_threshold() {
        let mut c = labeler(0.3);
        put(&mut c, &[1.0, 0.0]);
        put(&mut c, &[0.0, 1.0]);
        let labels = joint(&mut c, &[&[0.95, 0.05], &[0.9, 0.1]], true, 0.3);
        assert_eq!(labels, [from("S1"), from("S1")]);
    }

    #[test]
    fn joint_remap_update_false_does_not_mutate_centroids() {
        let mut c = labeler(0.02);
        put(&mut c, &[1.0, 0.0]);
        put(&mut c, &[0.0, 1.0]);
        let before = c.centroids.clone();
        let counts_before = c.counts.clone();
        let labels = joint(&mut c, &[&[0.95, 0.05], &[0.1, 0.99]], false, 0.02);
        assert_eq!(labels, [from("S1"), from("S2")]);
        assert_eq!(c.counts, counts_before);
        assert_eq!(c.centroids, before);
    }

    #[test]
    fn joint_remap_empty_input_returns_empty_list() {
        let mut c = labeler(0.3);
        assert!(joint(&mut c, &[], true, 0.3).is_empty());
    }

    #[test]
    fn match_embedding_default_source_is_empty_and_only_logged_on_open() {
        let mut c = labeler(0.9);
        put(&mut c, &[1.0, 0.0]);
        put(&mut c, &[0.99, 0.01]);
        assert_eq!(c.open_log, [Source::Unspecified]);
    }

    #[test]
    fn centroid_open_counts_tags_fast_and_remap_sources_separately() {
        let mut c = labeler(0.9);
        let fast = Match {
            source: Source::Fast,
            ..Match::default()
        };
        let remap = Match {
            source: Source::Remap,
            ..Match::default()
        };
        put_with(&mut c, &[1.0, 0.0], fast);
        put_with(&mut c, &[0.0, 1.0], fast);
        put_with(&mut c, &[-1.0, 0.0], remap);
        assert_eq!(
            c.centroid_open_counts(),
            BTreeMap::from([("fast", 2), ("remap", 1)])
        );
    }

    #[test]
    fn label_forwards_source_to_match_embedding() {
        let mut c = labeler(0.9);
        c.label(&unit(&[1.0, 0.0]), Source::Fast);
        c.label(&unit(&[0.0, 1.0]), Source::Remap);
        assert_eq!(c.open_log, [Source::Fast, Source::Remap]);
    }

    #[test]
    fn centroid_summary_flags_one_off_opens_with_a_final_match_count_of_one() {
        let mut c = labeler(0.9);
        let fast = Match {
            source: Source::Fast,
            ..Match::default()
        };
        put_with(&mut c, &[1.0, 0.0], fast);
        put_with(&mut c, &[0.99, 0.01], fast);
        put_with(&mut c, &[0.0, 1.0], fast);
        let expected = [("S1", 2), ("S2", 1)].map(|(label, matches)| CentroidSummary {
            label: label.to_owned(),
            source: Source::Fast,
            matches,
        });
        assert_eq!(c.centroid_summary(), expected);
    }

    #[test]
    fn new_centroid_is_provisional_until_second_hit() {
        let mut c = labeler(0.9);
        assert_eq!(put(&mut c, &[1.0, 0.0]), from("S1"));
        assert!(c.is_provisional("S1"));
        assert_eq!(c.display_label("S1"), "S1?");
        assert_eq!(put(&mut c, &[0.99, 0.01]), from("S1"));
        assert!(!c.is_provisional("S1"));
        assert_eq!(c.display_label("S1"), "S1");
    }

    #[test]
    fn confirmed_centroid_stays_confirmed_after_more_hits() {
        let mut c = labeler(0.9);
        put(&mut c, &[1.0, 0.0]);
        put(&mut c, &[0.99, 0.01]);
        put(&mut c, &[0.98, 0.02]);
        assert!(!c.is_provisional("S1"));
        assert_eq!(c.display_label("S1"), "S1");
    }

    #[test]
    fn display_label_does_not_affect_which_centroid_is_assigned() {
        let mut c = labeler(0.9);
        put(&mut c, &[1.0, 0.0]);
        let (centroids, counts) = (c.centroids.clone(), c.counts.clone());
        c.display_label("S1");
        c.is_provisional("S1");
        c.display_label("S1");
        assert_eq!(c.centroids, centroids);
        assert_eq!(c.counts, counts);
        assert_eq!(put(&mut c, &[0.0, 1.0]), from("S2"));
    }

    #[test]
    fn display_label_empty_string_passthrough() {
        let c = labeler(0.9);
        assert!(!c.is_provisional(""));
        assert_eq!(c.display_label(""), "");
    }

    #[test]
    fn display_label_out_of_range_label_passthrough() {
        let c = labeler(0.9);
        assert!(!c.is_provisional("S9"));
        assert_eq!(c.display_label("S9"), "S9");
        assert!(!c.is_provisional("S0"));
        assert!(!c.is_provisional("Sx"));
    }

    #[test]
    fn provisional_confirm_hits_constant_is_two() {
        assert_eq!(PROVISIONAL_CONFIRM_HITS, 2);
    }

    #[test]
    fn provisional_label_count_counts_only_still_provisional_centroids() {
        let mut c = labeler(0.9);
        put(&mut c, &[1.0, 0.0]);
        put(&mut c, &[0.99, 0.01]);
        put(&mut c, &[0.0, 1.0]);
        put(&mut c, &[-1.0, 0.0]);
        assert_eq!(c.provisional_label_count(), 2);
    }

    /// A labeler with the two given centroids, one match each, both confirmed.
    fn with_centroids(config: Config, vs: &[&[f32]]) -> Centroids {
        let mut c = Centroids::new(config);
        c.centroids = vs.iter().map(|v| unit(v)).collect();
        c.counts = vec![1; vs.len()];
        c.confirmed = vec![true; vs.len()];
        c.open_log = vec![Source::Unspecified; vs.len()];
        c
    }

    fn merging(threshold: f64) -> Config {
        Config {
            merge_enabled: true,
            merge_threshold: threshold,
            ..Config::default()
        }
    }

    #[test]
    fn provisional_label_count_skips_merged_away_centroids() {
        let mut c = with_centroids(merging(0.9), &[&[1.0, 0.0], &[0.99, 0.1411]]);
        c.merge_centroids(None);
        assert_eq!(c.provisional_label_count(), 0);
    }

    fn hysteresis(min_hits: u32) -> Config {
        Config {
            threshold: 0.9,
            hysteresis_enabled: true,
            hysteresis_min_hits: min_hits,
            ..Config::default()
        }
    }

    #[test]
    fn hysteresis_first_speaker_confirmed_immediately() {
        let mut c = Centroids::new(hysteresis(2));
        assert_eq!(put(&mut c, &[1.0, 0.0]), from("S1"));
        assert_eq!(c.confirmed, [true]);
    }

    #[test]
    fn hysteresis_provisional_speaker_falls_back_to_nearest_confirmed() {
        let mut c = Centroids::new(hysteresis(2));
        put(&mut c, &[1.0, 0.0]);
        assert_eq!(put(&mut c, &[0.0, 1.0]), from("S1"));
        assert_eq!(c.centroids.len(), 2);
        assert_eq!(c.confirmed, [true, false]);
    }

    #[test]
    fn hysteresis_confirms_after_min_hits_and_gets_own_label() {
        let mut c = Centroids::new(hysteresis(2));
        put(&mut c, &[1.0, 0.0]);
        put(&mut c, &[0.0, 1.0]);
        assert_eq!(put(&mut c, &[0.01, 0.9999]), from("S2"));
        assert_eq!(c.confirmed, [true, true]);
    }

    #[test]
    fn hysteresis_disabled_opens_new_speaker_immediately() {
        let mut c = labeler(0.9);
        put(&mut c, &[1.0, 0.0]);
        assert_eq!(put(&mut c, &[0.0, 1.0]), from("S2"));
    }

    #[test]
    fn merge_centroids_folds_similar_speakers() {
        let mut c = with_centroids(merging(0.9), &[&[1.0, 0.0], &[0.99, 0.1411]]);
        let merged = c.merge_centroids(None);
        let expected = BTreeMap::from([(String::from("S2"), String::from("S1"))]);
        assert_eq!(merged, expected);
        assert_eq!(c.alias, BTreeMap::from([(1, 0)]));
        assert_eq!(c.merge_history(), expected);
    }

    #[test]
    fn merge_centroids_below_threshold_does_nothing() {
        let mut c = with_centroids(merging(0.99), &[&[1.0, 0.0], &[0.0, 1.0]]);
        assert!(c.merge_centroids(None).is_empty());
        assert!(c.alias.is_empty());
        assert!(c.merge_history().is_empty());
    }

    #[test]
    fn maybe_merge_centroids_is_noop_when_disabled() {
        let config = Config {
            merge_enabled: false,
            merge_threshold: 0.9,
            ..Config::default()
        };
        let mut c = with_centroids(config, &[&[1.0, 0.0], &[0.99, 0.1411]]);
        assert!(c.maybe_merge_centroids().is_empty());
        assert!(c.alias.is_empty());
    }

    #[test]
    fn merged_centroid_absorbs_future_matches() {
        let config = Config {
            threshold: 0.5,
            ..merging(0.9)
        };
        let mut c = with_centroids(config, &[&[1.0, 0.0], &[0.99, 0.1411]]);
        c.merge_centroids(None);
        let label = put(&mut c, &[0.0, 1.0]);
        assert_ne!(label, from("S2"));
        assert_eq!(c.centroids.len(), 3);
        assert_eq!(label, from("S3"));
    }

    #[test]
    fn merge_history_flattens_across_calls() {
        let mut c = with_centroids(
            merging(0.9),
            &[&[1.0, 0.0], &[0.5, 0.8660], &[0.49, 0.8717]],
        );
        let first = c.merge_centroids(None);
        assert_eq!(
            first,
            BTreeMap::from([(String::from("S3"), String::from("S2"))])
        );
        c.centroids[1] = unit(&[0.95, 0.3122]);
        let second = c.merge_centroids(None);
        assert_eq!(
            second,
            BTreeMap::from([(String::from("S2"), String::from("S1"))])
        );
        let expected = BTreeMap::from([
            (String::from("S3"), String::from("S1")),
            (String::from("S2"), String::from("S1")),
        ]);
        assert_eq!(c.merge_history(), expected);
    }

    #[test]
    fn speaker_labeler_reset_clears_everything() {
        let mut c = with_centroids(merging(0.9), &[&[1.0, 0.0], &[0.99, 0.1411]]);
        c.merge_centroids(None);
        c.reset();
        assert!(c.centroids.is_empty() && c.counts.is_empty() && c.confirmed.is_empty());
        assert!(c.alias.is_empty() && c.merge_history.is_empty() && c.open_log.is_empty());
        assert_eq!(put(&mut c, &[1.0, 0.0]), from("S1"));
    }

    #[test]
    fn the_remap_threshold_follows_the_threshold_when_none_is_given() {
        let own = Centroids::new(Config {
            threshold: 0.6,
            remap_threshold: None,
            ..Config::default()
        });
        assert!((own.remap_threshold() - 0.6).abs() < 1e-12);
        assert!((Centroids::default().remap_threshold() - REMAP_THRESHOLD).abs() < 1e-12);
    }

    #[test]
    fn num_confirmed_speakers_counts_live_speakers_with_two_matches() {
        let mut c = labeler(0.9);
        put(&mut c, &[1.0, 0.0]);
        put(&mut c, &[0.99, 0.01]);
        put(&mut c, &[0.0, 1.0]);
        assert_eq!(c.num_confirmed_speakers(), 1);
    }

    #[test]
    fn a_zero_vector_opens_a_speaker_without_not_a_number() {
        let mut c = labeler(0.45);
        assert_eq!(c.label(&[0.0, 0.0], Source::Fast), "S1");
        assert_eq!(c.label(&[0.0, 0.0], Source::Fast), "S2");
    }
}
