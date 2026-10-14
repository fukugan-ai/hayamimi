//! Agglomerative clustering of embeddings: the algorithm of sherpa-onnx's
//! `FastClustering` (cosine dissimilarity, complete linkage, a cut at a
//! distance or into a number of clusters), written for the CPU.
//!
//! The speaker diarizer of sherpa-onnx runs its own copy inside one call and
//! has no entry point for it alone. This module is what the stage
//! measurement times for "the clustering". It is the nearest-neighbour-chain
//! algorithm, so a set of `n` points costs `O(n^2)`.

use std::num::NonZeroUsize;

/// How the distance between two clusters follows from the distances of their members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Linkage {
    /// The largest member distance.
    Complete,
    /// The mean member distance.
    Average,
}

/// One merge of the dendrogram: a point of each cluster that joins, and the distance at which they join.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Merge {
    pub a: usize,
    pub b: usize,
    pub height: f64,
}

/// The position of the pair `i < j` in a condensed distance matrix of `n` points.
fn pair_index(n: usize, i: usize, j: usize) -> usize {
    n * i - i * (i + 1) / 2 + (j - i - 1)
}

fn distance(condensed: &[f64], n: usize, i: usize, j: usize) -> f64 {
    let (low, high) = if i < j { (i, j) } else { (j, i) };
    condensed[pair_index(n, low, high)]
}

/// The nearest live cluster to `a`. A tie goes to `previous` (the cluster
/// below `a` in the chain), then to the lowest index.
fn nearest(
    condensed: &[f64],
    n: usize,
    alive: &[bool],
    a: usize,
    previous: Option<usize>,
) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for j in (0..n).filter(|j| alive[*j] && *j != a) {
        let d = distance(condensed, n, a, j);
        if best.is_none_or(|(_, found)| d < found) {
            best = Some((j, d));
        }
    }
    let (found, d) = best?;
    match previous {
        Some(p) if distance(condensed, n, a, p) <= d => Some(p),
        _ => Some(found),
    }
}

/// The distance from the cluster formed of `a` and `b` to a third cluster, from
/// the distances `da` and `db` of the two and their sizes.
fn combined(method: Linkage, da: f64, sa: usize, db: f64, sb: usize) -> f64 {
    match method {
        Linkage::Complete => da.max(db),
        Linkage::Average => (sa as f64 * da + sb as f64 * db) / (sa + sb) as f64,
    }
}

/// Joins clusters `a` and `b` into the lower of the two slots and updates the
/// distances to every other live cluster.
fn merge_slots(
    condensed: &mut [f64],
    n: usize,
    alive: &mut [bool],
    size: &mut [usize],
    method: Linkage,
    (a, b): (usize, usize),
) {
    let (keep, dead) = if a < b { (a, b) } else { (b, a) };
    for k in (0..n).filter(|k| alive[*k] && *k != keep && *k != dead) {
        let da = distance(condensed, n, keep, k);
        let db = distance(condensed, n, dead, k);
        let updated = combined(method, da, size[keep], db, size[dead]);
        let (low, high) = if keep < k { (keep, k) } else { (k, keep) };
        condensed[pair_index(n, low, high)] = updated;
    }
    size[keep] += size[dead];
    alive[dead] = false;
}

/// The merges that turn `n` points into one cluster. `condensed` holds the
/// `n (n - 1) / 2` distances of the pairs `(0, 1), (0, 2) .. (n-2, n-1)` and is
/// overwritten. The merges come in the order the algorithm finds them, which is
/// not the order of their heights.
pub fn linkage(mut condensed: Vec<f64>, n: usize, method: Linkage) -> Vec<Merge> {
    let mut merges = Vec::with_capacity(n.saturating_sub(1));
    let mut alive = vec![true; n];
    let mut size = vec![1_usize; n];
    let mut chain: Vec<usize> = Vec::new();
    while merges.len() + 1 < n {
        if chain.is_empty() {
            chain.extend(alive.iter().position(|live| *live));
        }
        while let Some(&a) = chain.last() {
            let previous = chain.len().checked_sub(2).map(|i| chain[i]);
            let Some(b) = nearest(&condensed, n, &alive, a, previous) else {
                return merges;
            };
            if Some(b) == previous {
                break;
            }
            chain.push(b);
        }
        let (Some(b), Some(a)) = (chain.pop(), chain.pop()) else {
            return merges;
        };
        let height = distance(&condensed, n, a, b);
        merges.push(Merge { a, b, height });
        merge_slots(&mut condensed, n, &mut alive, &mut size, method, (a, b));
    }
    merges
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// Cluster numbers `0..` in the order of the first point of each cluster.
fn labels_of(parent: &mut [usize]) -> Vec<usize> {
    let mut numbers: Vec<Option<usize>> = vec![None; parent.len()];
    let mut next = 0;
    let mut labels = Vec::with_capacity(parent.len());
    for i in 0..parent.len() {
        let root = find(parent, i);
        let number = *numbers[root].get_or_insert_with(|| {
            next += 1;
            next - 1
        });
        labels.push(number);
    }
    labels
}

fn apply(parent: &mut [usize], merge: &Merge) {
    let (ra, rb) = (find(parent, merge.a), find(parent, merge.b));
    if ra != rb {
        parent[rb] = ra;
    }
}

/// The clusters that remain when every merge at or below `threshold` is made.
pub fn cut_at_distance(n: usize, merges: &[Merge], threshold: f64) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..n).collect();
    for merge in merges.iter().filter(|m| m.height <= threshold) {
        apply(&mut parent, merge);
    }
    labels_of(&mut parent)
}

/// The clusters that remain when the lowest merges are made until `k` are left.
pub fn cut_into(n: usize, merges: &[Merge], k: usize) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..n).collect();
    let mut ordered: Vec<&Merge> = merges.iter().collect();
    ordered.sort_by(|x, y| x.height.total_cmp(&y.height));
    let keep = k.clamp(1, n.max(1));
    for merge in ordered.into_iter().take(n.saturating_sub(keep)) {
        apply(&mut parent, merge);
    }
    labels_of(&mut parent)
}

/// `1 - cos` of every pair of rows, clamped at 0, condensed. `rows` holds
/// `n` vectors of `dims` values, back to back.
pub fn cosine_dissimilarities(rows: &[f32], dims: NonZeroUsize) -> Vec<f64> {
    let dims = dims.get();
    let n = rows.len() / dims;
    let norms: Vec<f64> = rows
        .chunks_exact(dims)
        .map(|row| {
            row.iter()
                .map(|x| f64::from(*x).powi(2))
                .sum::<f64>()
                .sqrt()
        })
        .collect();
    let mut out = Vec::with_capacity(n * n.saturating_sub(1) / 2);
    for i in 0..n {
        let row_i = &rows[i * dims..(i + 1) * dims];
        for j in i + 1..n {
            let row_j = &rows[j * dims..(j + 1) * dims];
            let dot: f64 = row_i
                .iter()
                .zip(row_j)
                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                .sum();
            let scale = norms[i] * norms[j];
            let cosine = if scale > 0.0 { dot / scale } else { 0.0 };
            out.push((1.0 - cosine).max(0.0));
        }
    }
    out
}

/// What the speaker diarizer's clustering does with the embeddings of one
/// group: cosine dissimilarity, complete linkage, then a cut into
/// `num_clusters` clusters when that is given, else at `threshold`.
pub fn fast_clustering(
    rows: &[f32],
    dims: NonZeroUsize,
    threshold: f64,
    num_clusters: Option<usize>,
) -> Vec<usize> {
    let n = rows.len() / dims.get();
    if n <= 1 {
        return vec![0; n];
    }
    let merges = linkage(cosine_dissimilarities(rows, dims), n, Linkage::Complete);
    match num_clusters {
        Some(k) => cut_into(n, &merges, k),
        None => cut_at_distance(n, &merges, threshold),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: NonZeroUsize = match NonZeroUsize::new(2) {
        Some(two) => two,
        None => panic!("2 is not zero"),
    };

    /// Two tight groups on a line: {0, 1, 2} near 0 and {3, 4} near 10.
    fn line() -> (Vec<f64>, usize) {
        let points = [0.0_f64, 0.5, 1.0, 10.0, 10.5];
        let mut condensed = Vec::new();
        for i in 0..points.len() {
            for j in i + 1..points.len() {
                condensed.push((points[i] - points[j]).abs());
            }
        }
        (condensed, points.len())
    }

    #[test]
    fn a_pair_has_one_position_in_the_condensed_matrix() {
        assert_eq!(pair_index(4, 0, 1), 0);
        assert_eq!(pair_index(4, 0, 3), 2);
        assert_eq!(pair_index(4, 1, 2), 3);
        assert_eq!(pair_index(4, 2, 3), 5);
    }

    #[test]
    fn n_points_make_n_minus_one_merges() {
        let (condensed, n) = line();
        assert_eq!(linkage(condensed, n, Linkage::Complete).len(), n - 1);
    }

    #[test]
    fn no_points_and_one_point_make_no_merge() {
        assert!(linkage(Vec::new(), 0, Linkage::Complete).is_empty());
        assert!(linkage(Vec::new(), 1, Linkage::Complete).is_empty());
    }

    #[test]
    fn a_cut_between_the_groups_gives_two_clusters() {
        let (condensed, n) = line();
        let merges = linkage(condensed, n, Linkage::Complete);
        assert_eq!(cut_at_distance(n, &merges, 2.0), [0, 0, 0, 1, 1]);
    }

    #[test]
    fn a_cut_above_the_last_merge_gives_one_cluster_and_below_the_first_gives_n() {
        let (condensed, n) = line();
        let merges = linkage(condensed, n, Linkage::Complete);
        assert_eq!(cut_at_distance(n, &merges, 100.0), [0, 0, 0, 0, 0]);
        assert_eq!(cut_at_distance(n, &merges, 0.1), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn a_cut_into_k_gives_k_clusters() {
        let (condensed, n) = line();
        let merges = linkage(condensed, n, Linkage::Complete);
        assert_eq!(cut_into(n, &merges, 2), [0, 0, 0, 1, 1]);
        assert_eq!(cut_into(n, &merges, 1), [0, 0, 0, 0, 0]);
        assert_eq!(cut_into(n, &merges, 5), [0, 1, 2, 3, 4]);
        assert_eq!(cut_into(n, &merges, 9), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn complete_linkage_height_is_the_largest_member_distance() {
        let (condensed, n) = line();
        let merges = linkage(condensed, n, Linkage::Complete);
        let top = merges.iter().map(|m| m.height).fold(0.0, f64::max);
        assert!((top - 10.5).abs() < 1e-12, "{top}");
    }

    #[test]
    fn average_linkage_height_is_the_mean_member_distance() {
        let (condensed, n) = line();
        let merges = linkage(condensed, n, Linkage::Average);
        let top = merges.iter().map(|m| m.height).fold(0.0, f64::max);
        // The mean of 10, 9.5, 9, 10.5, 10, 9.5 over {0, .5, 1} x {10, 10.5}.
        assert!((top - 9.75).abs() < 1e-12, "{top}");
    }

    #[test]
    fn the_cluster_numbers_follow_the_first_point_of_each_cluster() {
        let (condensed, n) = line();
        let merges = linkage(condensed, n, Linkage::Complete);
        let labels = cut_at_distance(n, &merges, 2.0);
        assert_eq!(labels.first(), Some(&0));
    }

    #[test]
    fn fast_clustering_separates_two_directions() {
        let rows = [
            1.0_f32, 0.0, //
            0.99, 0.1, //
            0.0, 1.0, //
            0.1, 0.99,
        ];
        assert_eq!(fast_clustering(&rows, TWO, 0.5, None), [0, 0, 1, 1]);
        assert_eq!(fast_clustering(&rows, TWO, 0.5, Some(1)), [0, 0, 0, 0]);
    }

    #[test]
    fn fast_clustering_of_nothing_and_of_one_row() {
        assert!(fast_clustering(&[], TWO, 0.5, None).is_empty());
        assert_eq!(fast_clustering(&[1.0, 0.0], TWO, 0.5, None), [0]);
    }

    #[test]
    fn a_zero_vector_is_far_from_everything_rather_than_not_a_number() {
        let rows = [0.0_f32, 0.0, 1.0, 0.0];
        assert_eq!(cosine_dissimilarities(&rows, TWO), [1.0]);
    }

    #[test]
    fn dissimilarity_of_equal_and_opposite_rows() {
        let rows = [1.0_f32, 0.0, 1.0, 0.0, -1.0, 0.0];
        let d = cosine_dissimilarities(&rows, TWO);
        assert!(d[0].abs() < 1e-12);
        assert!((d[1] - 2.0).abs() < 1e-6);
        assert!((d[2] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn many_points_in_three_clouds_are_found() {
        let mut rows = Vec::new();
        for cloud in 0..3_usize {
            for i in 0..40_usize {
                let angle = cloud as f32 * 2.0 + i as f32 * 0.002;
                rows.extend([angle.cos(), angle.sin()]);
            }
        }
        let labels = fast_clustering(&rows, TWO, 0.3, None);
        assert_eq!(labels.len(), 120);
        assert_eq!(labels.iter().max(), Some(&2));
        assert!(labels[..40].iter().all(|l| *l == labels[0]));
        assert!(labels[80..].iter().all(|l| *l == labels[80]));
    }
}
