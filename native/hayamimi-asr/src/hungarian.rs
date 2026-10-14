//! The best one-to-one assignment of rows to columns (the Hungarian method).

/// The pairs `(row, column)` that make the total cost of a `rows` by `cols`
/// matrix, stored row by row, the smallest. There is one pair for each row of
/// the shorter side, sorted by row. A matrix with a value that is not finite
/// has no assignment and gives none.
pub fn minimize(cost: &[f64], rows: usize, cols: usize) -> Vec<(usize, usize)> {
    if rows == 0 || cols == 0 || cost.len() != rows * cols || cost.iter().any(|c| !c.is_finite()) {
        return Vec::new();
    }
    if rows <= cols {
        return solve(cost, rows, cols);
    }
    let transposed: Vec<f64> = (0..cols)
        .flat_map(|c| (0..rows).map(move |r| (r, c)))
        .map(|(r, c)| cost[r * cols + c])
        .collect();
    let mut pairs: Vec<(usize, usize)> = solve(&transposed, cols, rows)
        .into_iter()
        .map(|(c, r)| (r, c))
        .collect();
    pairs.sort_unstable();
    pairs
}

/// The method for `rows <= cols`, with potentials on both sides.
fn solve(cost: &[f64], rows: usize, cols: usize) -> Vec<(usize, usize)> {
    let mut u = vec![0.0_f64; rows + 1];
    let mut v = vec![0.0_f64; cols + 1];
    // `owner[j]` is the row (1-based, 0 for none) that holds column `j`.
    let mut owner = vec![0_usize; cols + 1];
    let mut way = vec![0_usize; cols + 1];
    for row in 1..=rows {
        owner[0] = row;
        let mut j0 = 0;
        let mut minv = vec![f64::INFINITY; cols + 1];
        let mut used = vec![false; cols + 1];
        loop {
            used[j0] = true;
            let i0 = owner[j0];
            let mut delta = f64::INFINITY;
            let mut j1 = 0;
            for j in (1..=cols).filter(|j| !used[*j]) {
                let reduced = cost[(i0 - 1) * cols + (j - 1)] - u[i0] - v[j];
                if reduced < minv[j] {
                    minv[j] = reduced;
                    way[j] = j0;
                }
                if minv[j] < delta {
                    delta = minv[j];
                    j1 = j;
                }
            }
            for j in 0..=cols {
                if used[j] {
                    u[owner[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if owner[j0] == 0 {
                break;
            }
        }
        loop {
            let previous = way[j0];
            owner[j0] = owner[previous];
            j0 = previous;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut pairs: Vec<(usize, usize)> = (1..=cols)
        .filter(|j| owner[*j] != 0)
        .map(|j| (owner[j] - 1, j - 1))
        .collect();
    pairs.sort_unstable();
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(cost: &[f64], cols: usize, pairs: &[(usize, usize)]) -> f64 {
        pairs.iter().map(|(r, c)| cost[r * cols + c]).sum()
    }

    #[test]
    fn a_three_by_three_matrix_has_its_known_optimum() {
        let cost = [4.0, 1.0, 3.0, 2.0, 0.0, 5.0, 3.0, 2.0, 2.0];
        let pairs = minimize(&cost, 3, 3);
        assert_eq!(pairs, [(0, 1), (1, 0), (2, 2)]);
        assert!((total(&cost, 3, &pairs) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn more_columns_than_rows_leaves_a_column_out() {
        let cost = [9.0, 1.0, 8.0, 7.0, 6.0, 0.5];
        let pairs = minimize(&cost, 2, 3);
        assert_eq!(pairs, [(0, 1), (1, 2)]);
    }

    #[test]
    fn more_rows_than_columns_leaves_a_row_out() {
        let cost = [9.0, 1.0, 2.0, 7.0, 0.5, 6.0];
        let pairs = minimize(&cost, 3, 2);
        assert_eq!(pairs, [(0, 1), (2, 0)]);
    }

    #[test]
    fn a_single_cell_is_the_assignment() {
        assert_eq!(minimize(&[3.0], 1, 1), [(0, 0)]);
    }

    #[test]
    fn an_empty_matrix_and_a_wrongly_sized_one_have_no_assignment() {
        assert!(minimize(&[], 0, 0).is_empty());
        assert!(minimize(&[], 0, 3).is_empty());
        assert!(minimize(&[1.0, 2.0, 3.0], 2, 2).is_empty());
    }

    #[test]
    fn a_value_that_is_not_finite_has_no_assignment() {
        assert!(minimize(&[1.0, f64::NAN, 2.0, 3.0], 2, 2).is_empty());
        assert!(minimize(&[1.0, f64::INFINITY, 2.0, 3.0], 2, 2).is_empty());
    }

    #[test]
    fn the_optimum_beats_the_greedy_choice() {
        // Greedy takes (0, 0) = 1 first and is forced to (1, 1) = 10; the optimum is 2 + 2.
        let cost = [1.0, 2.0, 2.0, 10.0];
        let pairs = minimize(&cost, 2, 2);
        assert_eq!(pairs, [(0, 1), (1, 0)]);
    }
}
