use crate::factor::FactorDirection;

#[derive(Debug, Clone, PartialEq)]
pub struct CrossSectionValue {
    pub symbol: String,
    pub raw_value: f64,
    pub rank: Option<u32>,
    pub rank_score: f64,
    pub z_score: f64,
}

pub fn standardize_cross_section(
    values: &[(String, f64)],
    direction: FactorDirection,
) -> Vec<CrossSectionValue> {
    let adjusted = values
        .iter()
        .map(|(_, value)| match direction {
            FactorDirection::Positive => *value,
            FactorDirection::Negative => -*value,
        })
        .collect::<Vec<_>>();
    let ranks = ordinal_ranks(&adjusted);
    let finite_count = ranks.iter().filter(|rank| rank.is_some()).count();
    let denom = finite_count.saturating_sub(1) as f64;
    let mut rank_scores = ranks
        .iter()
        .map(|rank| match (rank, finite_count) {
            (Some(_), 1) => 0.5,
            (Some(rank), _) if denom > 0.0 => *rank as f64 / denom,
            _ => f64::NAN,
        })
        .collect::<Vec<_>>();

    let (mean, std) = finite_mean_std(&rank_scores);
    values
        .iter()
        .zip(ranks.iter())
        .zip(rank_scores.iter_mut())
        .map(|((value, rank), rank_score)| CrossSectionValue {
            symbol: value.0.clone(),
            raw_value: value.1,
            rank: *rank,
            rank_score: *rank_score,
            z_score: if rank_score.is_finite() && std > 0.0 {
                (*rank_score - mean) / std
            } else if rank_score.is_finite() {
                0.0
            } else {
                f64::NAN
            },
        })
        .collect()
}

pub fn ordinal_ranks(values: &[f64]) -> Vec<Option<u32>> {
    let mut indexed = values
        .iter()
        .enumerate()
        .filter(|(_, value)| value.is_finite())
        .map(|(index, value)| (index, *value))
        .collect::<Vec<_>>();
    indexed.sort_by(|left, right| {
        left.1
            .total_cmp(&right.1)
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut ranks = vec![None; values.len()];
    for (rank, (index, _)) in indexed.into_iter().enumerate() {
        ranks[index] = Some(rank as u32);
    }
    ranks
}

pub fn average_ranks(values: &[f64]) -> Vec<Option<f64>> {
    let mut indexed = values
        .iter()
        .enumerate()
        .filter(|(_, value)| value.is_finite())
        .map(|(index, value)| (index, *value))
        .collect::<Vec<_>>();
    indexed.sort_by(|left, right| left.1.total_cmp(&right.1));

    let mut ranks = vec![None; values.len()];
    let mut start = 0;
    while start < indexed.len() {
        let mut end = start + 1;
        while end < indexed.len() && indexed[end].1 == indexed[start].1 {
            end += 1;
        }
        let avg_rank = (start + end - 1) as f64 / 2.0;
        for (index, _) in &indexed[start..end] {
            ranks[*index] = Some(avg_rank);
        }
        start = end;
    }
    ranks
}

fn finite_mean_std(values: &[f64]) -> (f64, f64) {
    let finite = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if finite.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let mean = finite.iter().sum::<f64>() / finite.len() as f64;
    let var = finite
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / finite.len() as f64;
    (mean, var.sqrt())
}

#[cfg(test)]
mod tests {
    use super::{average_ranks, standardize_cross_section};
    use crate::factor::FactorDirection;

    #[test]
    fn rank_standardization_handles_direction_and_nan() {
        let values = vec![
            ("a".to_owned(), 10.0),
            ("b".to_owned(), 30.0),
            ("c".to_owned(), 20.0),
            ("d".to_owned(), f64::NAN),
        ];
        let out = standardize_cross_section(&values, FactorDirection::Positive);
        assert_eq!(out[0].rank, Some(0));
        assert_eq!(out[1].rank, Some(2));
        assert_eq!(out[2].rank, Some(1));
        assert_eq!(out[3].rank, None);
        assert_eq!(out[1].rank_score, 1.0);

        let reversed = standardize_cross_section(&values, FactorDirection::Negative);
        assert_eq!(reversed[0].rank, Some(2));
        assert_eq!(reversed[1].rank, Some(0));
    }

    #[test]
    fn average_rank_ties_for_spearman() {
        let ranks = average_ranks(&[2.0, 2.0, 5.0, 1.0]);
        assert_eq!(ranks, vec![Some(1.5), Some(1.5), Some(3.0), Some(0.0)]);
    }
}
