use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tg_contracts::{Fill, OrderSide};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EquityPoint {
    pub date: NaiveDate,
    pub total_value: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trade {
    pub symbol: String,
    pub quantity: i64,
    pub opened_at: DateTime<Utc>,
    pub closed_at: DateTime<Utc>,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    pub pnl: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkMetrics {
    pub total_return: f64,
    pub annualized_return: f64,
    pub alpha: f64,
    pub beta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestMetrics {
    pub total_return: f64,
    pub annualized_return: f64,
    pub sharpe: f64,
    pub max_drawdown: f64,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub trade_count: usize,
    pub commission_total: Decimal,
    pub tax_total: Decimal,
    pub transfer_fee_total: Decimal,
    pub benchmark: Option<BenchmarkMetrics>,
    pub equity_curve: Vec<EquityPoint>,
    pub trades: Vec<Trade>,
}

pub fn compute_metrics(
    equity_curve: &[EquityPoint],
    trades: &[Trade],
    fills: &[Fill],
    benchmark: Option<&[EquityPoint]>,
) -> BacktestMetrics {
    let returns = daily_returns(equity_curve);
    let strategy_total_return = total_return(equity_curve);
    let strategy_annualized_return = annualized_return(strategy_total_return, returns.len());
    let sharpe = sharpe(&returns);
    let max_drawdown = max_drawdown(equity_curve);
    let win_rate = win_rate(trades);
    let profit_factor = profit_factor(trades);
    let commission_total = fills.iter().map(|fill| fill.commission).sum();
    let tax_total = fills.iter().map(|fill| fill.tax).sum();
    let transfer_fee_total = fills.iter().map(|fill| fill.transfer_fee).sum();
    let benchmark = benchmark.map(|series| {
        let benchmark_returns = daily_returns(series);
        let benchmark_total_return_value = total_return(series);
        let benchmark_annualized =
            annualized_return(benchmark_total_return_value, benchmark_returns.len());
        BenchmarkMetrics {
            total_return: benchmark_total_return_value,
            annualized_return: benchmark_annualized,
            alpha: strategy_annualized_return - benchmark_annualized,
            beta: beta(&returns, &benchmark_returns),
        }
    });

    BacktestMetrics {
        total_return: strategy_total_return,
        annualized_return: strategy_annualized_return,
        sharpe,
        max_drawdown,
        win_rate,
        profit_factor,
        trade_count: trades.len(),
        commission_total,
        tax_total,
        transfer_fee_total,
        benchmark,
        equity_curve: equity_curve.to_vec(),
        trades: trades.to_vec(),
    }
}

pub fn closed_trades_from_fills(fills: &[Fill]) -> Vec<Trade> {
    #[derive(Debug, Clone)]
    struct OpenLot {
        quantity: i64,
        opened_at: DateTime<Utc>,
        price: Decimal,
        remaining_cost: Decimal,
    }

    let mut open_lots: HashMap<String, Vec<OpenLot>> = HashMap::new();
    let mut trades = Vec::new();

    for fill in fills {
        let notional = fill.price * Decimal::from(fill.quantity);
        let fees = fill.commission + fill.tax + fill.transfer_fee;
        match fill.side {
            OrderSide::Buy => {
                open_lots
                    .entry(fill.symbol.clone())
                    .or_default()
                    .push(OpenLot {
                        quantity: fill.quantity,
                        opened_at: fill.ts,
                        price: fill.price,
                        remaining_cost: notional + fees,
                    });
            }
            OrderSide::Sell => {
                let lots = open_lots.entry(fill.symbol.clone()).or_default();
                let mut sell_remaining = fill.quantity;
                let mut lot_index = 0;
                while sell_remaining > 0 && lot_index < lots.len() {
                    let matched_qty = sell_remaining.min(lots[lot_index].quantity);
                    let qty_decimal = Decimal::from(matched_qty);
                    let lot_qty_decimal = Decimal::from(lots[lot_index].quantity);
                    let entry_cost = lots[lot_index].remaining_cost * qty_decimal / lot_qty_decimal;
                    let exit_fees = fees * qty_decimal / Decimal::from(fill.quantity);
                    let exit_value = fill.price * qty_decimal - exit_fees;
                    trades.push(Trade {
                        symbol: fill.symbol.clone(),
                        quantity: matched_qty,
                        opened_at: lots[lot_index].opened_at,
                        closed_at: fill.ts,
                        entry_price: lots[lot_index].price,
                        exit_price: fill.price,
                        pnl: exit_value - entry_cost,
                    });

                    lots[lot_index].quantity -= matched_qty;
                    lots[lot_index].remaining_cost -= entry_cost;
                    sell_remaining -= matched_qty;
                    if lots[lot_index].quantity == 0 {
                        lot_index += 1;
                    }
                }
                if lot_index > 0 {
                    lots.drain(0..lot_index);
                }
            }
        }
    }

    trades
}

fn total_return(equity_curve: &[EquityPoint]) -> f64 {
    if equity_curve.len() < 2 {
        return 0.0;
    }
    let start = decimal_to_f64(equity_curve[0].total_value);
    let end = decimal_to_f64(equity_curve[equity_curve.len() - 1].total_value);
    if start == 0.0 {
        0.0
    } else {
        end / start - 1.0
    }
}

fn annualized_return(total_return: f64, return_count: usize) -> f64 {
    if return_count == 0 {
        return 0.0;
    }
    (1.0 + total_return).powf(252.0 / return_count as f64) - 1.0
}

fn daily_returns(equity_curve: &[EquityPoint]) -> Vec<f64> {
    equity_curve
        .windows(2)
        .filter_map(|window| {
            let previous = decimal_to_f64(window[0].total_value);
            let current = decimal_to_f64(window[1].total_value);
            (previous != 0.0).then_some(current / previous - 1.0)
        })
        .collect()
}

fn sharpe(returns: &[f64]) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns
        .iter()
        .map(|ret| {
            let diff = ret - mean;
            diff * diff
        })
        .sum::<f64>()
        / returns.len() as f64;
    let std_dev = variance.sqrt();
    if std_dev == 0.0 {
        0.0
    } else {
        mean / std_dev * 252.0_f64.sqrt()
    }
}

fn max_drawdown(equity_curve: &[EquityPoint]) -> f64 {
    let mut peak = 0.0;
    let mut max_drawdown = 0.0;
    for point in equity_curve {
        let value = decimal_to_f64(point.total_value);
        if value > peak {
            peak = value;
        }
        if peak > 0.0 {
            let drawdown = (peak - value) / peak;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }
    }
    max_drawdown
}

fn win_rate(trades: &[Trade]) -> f64 {
    if trades.is_empty() {
        return 0.0;
    }
    let winners = trades
        .iter()
        .filter(|trade| trade.pnl > Decimal::ZERO)
        .count();
    winners as f64 / trades.len() as f64
}

fn profit_factor(trades: &[Trade]) -> f64 {
    let gross_profit: f64 = trades
        .iter()
        .filter(|trade| trade.pnl > Decimal::ZERO)
        .map(|trade| decimal_to_f64(trade.pnl))
        .sum();
    let gross_loss: f64 = trades
        .iter()
        .filter(|trade| trade.pnl < Decimal::ZERO)
        .map(|trade| decimal_to_f64(-trade.pnl))
        .sum();
    if gross_loss == 0.0 {
        if gross_profit == 0.0 {
            0.0
        } else {
            f64::INFINITY
        }
    } else {
        gross_profit / gross_loss
    }
}

fn beta(strategy_returns: &[f64], benchmark_returns: &[f64]) -> f64 {
    let len = strategy_returns.len().min(benchmark_returns.len());
    if len == 0 {
        return 0.0;
    }
    let strategy = &strategy_returns[..len];
    let benchmark = &benchmark_returns[..len];
    let strategy_mean = strategy.iter().sum::<f64>() / len as f64;
    let benchmark_mean = benchmark.iter().sum::<f64>() / len as f64;
    let covariance = strategy
        .iter()
        .zip(benchmark)
        .map(|(left, right)| (left - strategy_mean) * (right - benchmark_mean))
        .sum::<f64>()
        / len as f64;
    let variance = benchmark
        .iter()
        .map(|ret| {
            let diff = ret - benchmark_mean;
            diff * diff
        })
        .sum::<f64>()
        / len as f64;
    if variance == 0.0 {
        0.0
    } else {
        covariance / variance
    }
}

fn decimal_to_f64(value: Decimal) -> f64 {
    value.to_f64().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use tg_contracts::Exchange;

    use super::*;

    fn dec(value: i64, scale: u32) -> Decimal {
        Decimal::new(value, scale)
    }

    fn point(day: u32, value: i64) -> EquityPoint {
        EquityPoint {
            date: NaiveDate::from_ymd_opt(2026, 6, day).unwrap(),
            total_value: dec(value, 0),
        }
    }

    #[test]
    fn metrics_match_hand_computed_equity_and_trade_fixture() {
        let curve = vec![point(1, 100), point(2, 105), point(3, 99), point(4, 110)];
        let trades = vec![
            Trade {
                symbol: "600001".to_owned(),
                quantity: 100,
                opened_at: Utc::now(),
                closed_at: Utc::now(),
                entry_price: dec(1000, 2),
                exit_price: dec(1100, 2),
                pnl: dec(10, 0),
            },
            Trade {
                symbol: "600002".to_owned(),
                quantity: 100,
                opened_at: Utc::now(),
                closed_at: Utc::now(),
                entry_price: dec(1000, 2),
                exit_price: dec(950, 2),
                pnl: dec(-5, 0),
            },
        ];

        let metrics = compute_metrics(&curve, &trades, &[], None);

        assert!((metrics.total_return - 0.10).abs() < 1e-12);
        assert!((metrics.max_drawdown - (6.0 / 105.0)).abs() < 1e-12);
        assert_eq!(metrics.win_rate, 0.5);
        assert_eq!(metrics.profit_factor, 2.0);
        assert_eq!(metrics.trade_count, 2);
        assert!(metrics.sharpe.is_finite());
    }

    #[test]
    fn closed_trades_use_fifo_lots_and_costs() {
        let ts = Utc::now();
        let fills = vec![
            Fill {
                order_id: "o1".to_owned(),
                fill_id: "f1".to_owned(),
                symbol: "600001".to_owned(),
                exchange: Exchange::Sh,
                side: OrderSide::Buy,
                price: dec(1000, 2),
                quantity: 100,
                commission: dec(1, 0),
                tax: Decimal::ZERO,
                transfer_fee: Decimal::ZERO,
                ts,
                trading_date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            },
            Fill {
                order_id: "o2".to_owned(),
                fill_id: "f2".to_owned(),
                symbol: "600001".to_owned(),
                exchange: Exchange::Sh,
                side: OrderSide::Sell,
                price: dec(1100, 2),
                quantity: 100,
                commission: dec(1, 0),
                tax: Decimal::ZERO,
                transfer_fee: Decimal::ZERO,
                ts,
                trading_date: NaiveDate::from_ymd_opt(2026, 6, 2).unwrap(),
            },
        ];

        let trades = closed_trades_from_fills(&fills);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].pnl, dec(98, 0));
    }
}
