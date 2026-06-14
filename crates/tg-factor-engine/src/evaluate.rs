use std::collections::BTreeMap;

use chrono::NaiveDate;
use tg_contracts::FactorEvaluation;

use crate::cross_section::average_ranks;
use crate::error::{FactorError, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct FactorReturn {
    pub date: NaiveDate,
    pub symbol: String,
    pub factor_value: f64,
    pub forward_return: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationInput {
    pub factor: String,
    pub rows: Vec<FactorReturn>,
    pub decay_rows: Vec<Vec<FactorReturn>>,
    pub quantiles: usize,
}

pub fn spearman_rank_ic(pairs: &[(f64, f64)]) -> f64 {
    let filtered = pairs
        .iter()
        .copied()
        .filter(|(factor, ret)| factor.is_finite() && ret.is_finite())
        .collect::<Vec<_>>();
    if filtered.len() < 2 {
        return f64::NAN;
    }
    let factors = filtered
        .iter()
        .map(|(factor, _)| *factor)
        .collect::<Vec<_>>();
    let returns = filtered.iter().map(|(_, ret)| *ret).collect::<Vec<_>>();
    let factor_ranks = average_ranks(&factors);
    let return_ranks = average_ranks(&returns);
    let ranked_pairs = factor_ranks
        .iter()
        .zip(return_ranks.iter())
        .filter_map(|(factor, ret)| Some((factor.as_ref()?, ret.as_ref()?)))
        .map(|(factor, ret)| (*factor, *ret))
        .collect::<Vec<_>>();
    pearson(&ranked_pairs)
}

pub fn ic_series(rows: &[FactorReturn]) -> Vec<f64> {
    let mut by_date: BTreeMap<NaiveDate, Vec<(f64, f64)>> = BTreeMap::new();
    for row in rows {
        by_date
            .entry(row.date)
            .or_default()
            .push((row.factor_value, row.forward_return));
    }
    by_date
        .values()
        .map(|pairs| spearman_rank_ic(pairs))
        .filter(|value| value.is_finite())
        .collect()
}

pub fn ic_mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

pub fn ic_std(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let mean = ic_mean(values);
    let var = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / (values.len() - 1) as f64;
    var.sqrt()
}

pub fn information_ratio(values: &[f64]) -> f64 {
    let mean = ic_mean(values);
    let std = ic_std(values);
    if std > 0.0 {
        mean / std
    } else {
        f64::NAN
    }
}

pub fn decay(decay_rows: &[Vec<FactorReturn>]) -> Vec<f64> {
    decay_rows
        .iter()
        .map(|rows| ic_mean(&ic_series(rows)))
        .collect()
}

pub fn quantile_returns(rows: &[FactorReturn], quantiles: usize) -> Result<Vec<f64>> {
    if quantiles == 0 {
        return Err(FactorError::InvalidInput(
            "quantiles must be positive".to_owned(),
        ));
    }
    let mut sums = vec![0.0; quantiles];
    let mut counts = vec![0usize; quantiles];
    let mut by_date: BTreeMap<NaiveDate, Vec<&FactorReturn>> = BTreeMap::new();
    for row in rows {
        if row.factor_value.is_finite() && row.forward_return.is_finite() {
            by_date.entry(row.date).or_default().push(row);
        }
    }

    for mut date_rows in by_date.into_values() {
        date_rows.sort_by(|left, right| {
            left.factor_value
                .total_cmp(&right.factor_value)
                .then_with(|| left.symbol.cmp(&right.symbol))
        });
        let n = date_rows.len();
        if n == 0 {
            continue;
        }
        for (index, row) in date_rows.into_iter().enumerate() {
            let bucket = (index * quantiles / n).min(quantiles - 1);
            sums[bucket] += row.forward_return;
            counts[bucket] += 1;
        }
    }

    Ok(sums
        .into_iter()
        .zip(counts)
        .map(|(sum, count)| {
            if count > 0 {
                sum / count as f64
            } else {
                f64::NAN
            }
        })
        .collect())
}

pub fn evaluate(input: EvaluationInput) -> Result<FactorEvaluation> {
    let series = ic_series(&input.rows);
    let mean = ic_mean(&series);
    let std = ic_std(&series);
    let ir = if std > 0.0 { mean / std } else { f64::NAN };
    Ok(FactorEvaluation {
        factor: input.factor,
        ic_mean: mean,
        ic_std: std,
        ir,
        decay: decay(&input.decay_rows),
        quantile_returns: quantile_returns(&input.rows, input.quantiles)?,
    })
}

fn pearson(pairs: &[(f64, f64)]) -> f64 {
    if pairs.len() < 2 {
        return f64::NAN;
    }
    let x_mean = pairs.iter().map(|(x, _)| *x).sum::<f64>() / pairs.len() as f64;
    let y_mean = pairs.iter().map(|(_, y)| *y).sum::<f64>() / pairs.len() as f64;
    let mut numerator = 0.0;
    let mut x_var = 0.0;
    let mut y_var = 0.0;
    for (x, y) in pairs {
        let x_diff = x - x_mean;
        let y_diff = y - y_mean;
        numerator += x_diff * y_diff;
        x_var += x_diff.powi(2);
        y_var += y_diff.powi(2);
    }
    let denom = x_var.sqrt() * y_var.sqrt();
    if denom > 0.0 {
        numerator / denom
    } else {
        f64::NAN
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::{
        evaluate, ic_series, ic_std, information_ratio, quantile_returns, spearman_rank_ic,
        EvaluationInput, FactorReturn,
    };

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, day).unwrap()
    }

    #[test]
    fn spearman_ic_matches_hand_computed_inverse_ranks() {
        let ic = spearman_rank_ic(&[(1.0, 3.0), (2.0, 2.0), (3.0, 1.0)]);
        assert!((ic + 1.0).abs() < 1e-12);
    }

    #[test]
    fn ir_uses_sample_standard_deviation() {
        let values = [1.0, 2.0, 3.0];
        let std = ic_std(&values);
        assert!((std - 1.0).abs() < 1e-12);
        assert!((information_ratio(&values) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn series_decay_and_quantiles_are_grouped_by_date() {
        let rows = vec![
            FactorReturn {
                date: date(15),
                symbol: "a".to_owned(),
                factor_value: 1.0,
                forward_return: 0.01,
            },
            FactorReturn {
                date: date(15),
                symbol: "b".to_owned(),
                factor_value: 2.0,
                forward_return: 0.02,
            },
            FactorReturn {
                date: date(16),
                symbol: "a".to_owned(),
                factor_value: 1.0,
                forward_return: 0.03,
            },
            FactorReturn {
                date: date(16),
                symbol: "b".to_owned(),
                factor_value: 2.0,
                forward_return: 0.04,
            },
        ];
        let series = ic_series(&rows);
        assert_eq!(series.len(), 2);
        assert!(series.iter().all(|value| (value - 1.0).abs() < 1e-12));
        let q = quantile_returns(&rows, 2).unwrap();
        assert!((q[0] - 0.02).abs() < 1e-12);
        assert!((q[1] - 0.03).abs() < 1e-12);

        let eval = evaluate(EvaluationInput {
            factor: "f".to_owned(),
            rows: rows.clone(),
            decay_rows: vec![rows],
            quantiles: 2,
        })
        .unwrap();
        assert_eq!(eval.decay.len(), 1);
        assert!((eval.decay[0] - 1.0).abs() < 1e-12);
        assert!((eval.ic_mean - 1.0).abs() < 1e-12);
    }
}
