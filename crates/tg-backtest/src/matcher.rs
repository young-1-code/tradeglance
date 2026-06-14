use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tg_contracts::{
    limit_up_pct, Account, Bar, Board, Exchange, Fill, Order, OrderId, OrderIntent, OrderSide,
    OrderStatus, OrderType, Position, TgError, COMMISSION_MAX_PCT, LOT_SIZE, STAMP_DUTY_PCT,
    TRANSFER_FEE_PCT,
};
use tg_engine::{ExecutionHandler, Portfolio};
use tokio::sync::broadcast;

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatcherConfig {
    pub initial_cash: Decimal,
    pub commission_rate: Decimal,
    pub min_commission: Decimal,
    pub slippage_bps: Decimal,
    pub default_board: Option<Board>,
}

impl Default for MatcherConfig {
    fn default() -> Self {
        Self {
            initial_cash: Decimal::ZERO,
            commission_rate: COMMISSION_MAX_PCT,
            min_commission: Decimal::ZERO,
            slippage_bps: Decimal::ZERO,
            default_board: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub commission: Decimal,
    pub tax: Decimal,
    pub transfer_fee: Decimal,
}

#[derive(Debug, Clone)]
pub struct PendingOrder {
    pub order: Order,
    pub submitted_bar: Bar,
}

#[derive(Debug)]
pub struct BacktestLedger {
    account: RwLock<Account>,
    open_orders: RwLock<HashMap<OrderId, Order>>,
    fills: RwLock<Vec<Fill>>,
    applied_fills: RwLock<HashSet<String>>,
    equity_by_date: RwLock<BTreeMap<NaiveDate, Decimal>>,
    last_trading_date: RwLock<Option<NaiveDate>>,
}

impl BacktestLedger {
    pub fn with_cash(cash: Decimal) -> Self {
        Self {
            account: RwLock::new(Account {
                cash,
                frozen_cash: Decimal::ZERO,
                total_value: cash,
                positions: HashMap::new(),
            }),
            open_orders: RwLock::new(HashMap::new()),
            fills: RwLock::new(Vec::new()),
            applied_fills: RwLock::new(HashSet::new()),
            equity_by_date: RwLock::new(BTreeMap::new()),
            last_trading_date: RwLock::new(None),
        }
    }

    pub fn fills(&self) -> Vec<Fill> {
        self.read_fills()
            .expect("ledger fills lock should not be poisoned")
            .clone()
    }

    pub fn equity_curve(&self) -> Vec<(NaiveDate, Decimal)> {
        self.read_equity()
            .expect("ledger equity lock should not be poisoned")
            .iter()
            .map(|(date, value)| (*date, *value))
            .collect()
    }

    pub fn mark_to_market(&self, bar: &Bar) -> Result<(), TgError> {
        self.unlock_for_date(bar.trading_date)?;
        let total_value = {
            let mut account = self.write_account()?;
            if let Some(position) = account.positions.get_mut(&bar.symbol) {
                position.last_price = bar.close;
                position.market_value = bar.close * Decimal::from(position.total_quantity);
                position.unrealized_pnl =
                    (bar.close - position.avg_cost) * Decimal::from(position.total_quantity);
            }
            recalculate_total_value(&mut account)
        };
        self.write_equity()?.insert(bar.trading_date, total_value);
        Ok(())
    }

    pub fn unlock_for_date(&self, date: NaiveDate) -> Result<(), TgError> {
        let mut last_date = self
            .last_trading_date
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("ledger date lock poisoned")))?;
        if last_date.is_some_and(|existing| existing == date) {
            return Ok(());
        }

        let mut account = self.write_account()?;
        for position in account.positions.values_mut() {
            position.t1_locked_quantity = 0;
            position.available_quantity = position.total_quantity;
        }
        *last_date = Some(date);
        Ok(())
    }

    pub fn upsert_open_order(&self, order: Order) -> Result<(), TgError> {
        self.write_orders()?.insert(order.id.clone(), order);
        Ok(())
    }

    pub fn remove_open_order(&self, order_id: &str) -> Result<Option<Order>, TgError> {
        Ok(self.write_orders()?.remove(order_id))
    }

    fn read_account(&self) -> Result<RwLockReadGuard<'_, Account>, TgError> {
        self.account
            .read()
            .map_err(|_| TgError::Other(anyhow::anyhow!("ledger account lock poisoned")))
    }

    fn write_account(&self) -> Result<RwLockWriteGuard<'_, Account>, TgError> {
        self.account
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("ledger account lock poisoned")))
    }

    fn read_orders(&self) -> Result<RwLockReadGuard<'_, HashMap<OrderId, Order>>, TgError> {
        self.open_orders
            .read()
            .map_err(|_| TgError::Other(anyhow::anyhow!("ledger orders lock poisoned")))
    }

    fn write_orders(&self) -> Result<RwLockWriteGuard<'_, HashMap<OrderId, Order>>, TgError> {
        self.open_orders
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("ledger orders lock poisoned")))
    }

    fn read_fills(&self) -> Result<RwLockReadGuard<'_, Vec<Fill>>, TgError> {
        self.fills
            .read()
            .map_err(|_| TgError::Other(anyhow::anyhow!("ledger fills lock poisoned")))
    }

    fn write_fills(&self) -> Result<RwLockWriteGuard<'_, Vec<Fill>>, TgError> {
        self.fills
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("ledger fills lock poisoned")))
    }

    fn read_equity(&self) -> Result<RwLockReadGuard<'_, BTreeMap<NaiveDate, Decimal>>, TgError> {
        self.equity_by_date
            .read()
            .map_err(|_| TgError::Other(anyhow::anyhow!("ledger equity lock poisoned")))
    }

    fn write_equity(&self) -> Result<RwLockWriteGuard<'_, BTreeMap<NaiveDate, Decimal>>, TgError> {
        self.equity_by_date
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("ledger equity lock poisoned")))
    }
}

impl Portfolio for BacktestLedger {
    fn account(&self) -> Account {
        self.read_account()
            .expect("ledger account lock should not be poisoned")
            .clone()
    }

    fn position(&self, symbol: &str) -> Option<Position> {
        self.read_account()
            .expect("ledger account lock should not be poisoned")
            .positions
            .get(symbol)
            .cloned()
    }

    fn positions(&self) -> Vec<Position> {
        self.read_account()
            .expect("ledger account lock should not be poisoned")
            .positions
            .values()
            .cloned()
            .collect()
    }

    fn open_orders(&self) -> Vec<Order> {
        self.read_orders()
            .expect("ledger orders lock should not be poisoned")
            .values()
            .cloned()
            .collect()
    }

    fn apply_fill(&self, fill: &Fill) -> tg_contracts::Result<()> {
        {
            let mut applied = self
                .applied_fills
                .write()
                .map_err(|_| TgError::Other(anyhow::anyhow!("ledger fill id lock poisoned")))?;
            if !applied.insert(fill.fill_id.clone()) {
                return Ok(());
            }
        }

        let total_value = {
            let mut account = self.write_account()?;
            apply_fill_to_account(&mut account, fill);
            recalculate_total_value(&mut account)
        };
        self.write_fills()?.push(fill.clone());
        self.write_equity()?.insert(fill.trading_date, total_value);
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct CurrentBar {
    bar: Bar,
    pre_close: Decimal,
    board: Board,
}

#[derive(Debug)]
pub struct HistoricalMatcher {
    config: MatcherConfig,
    ledger: Arc<BacktestLedger>,
    current_bars: RwLock<HashMap<String, CurrentBar>>,
    previous_close: RwLock<HashMap<String, Decimal>>,
    pending: RwLock<HashMap<OrderId, PendingOrder>>,
    tx: broadcast::Sender<Fill>,
}

impl HistoricalMatcher {
    pub fn new(config: MatcherConfig) -> Self {
        Self::with_ledger(
            Arc::new(BacktestLedger::with_cash(config.initial_cash)),
            config,
        )
    }

    pub fn with_ledger(ledger: Arc<BacktestLedger>, config: MatcherConfig) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            config,
            ledger,
            current_bars: RwLock::new(HashMap::new()),
            previous_close: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashMap::new()),
            tx,
        }
    }

    pub fn ledger(&self) -> Arc<BacktestLedger> {
        Arc::clone(&self.ledger)
    }

    pub fn set_current_bar(&self, bar: Bar) -> Result<(), TgError> {
        self.ledger.unlock_for_date(bar.trading_date)?;
        let pre_close = self
            .previous_close
            .read()
            .map_err(|_| TgError::Other(anyhow::anyhow!("matcher previous close lock poisoned")))?
            .get(&bar.symbol)
            .copied()
            .unwrap_or(bar.open);
        let board = self
            .config
            .default_board
            .unwrap_or_else(|| infer_board(&bar.symbol, bar.exchange));
        self.current_bars
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("matcher current bar lock poisoned")))?
            .insert(
                bar.symbol.clone(),
                CurrentBar {
                    bar: bar.clone(),
                    pre_close,
                    board,
                },
            );
        self.previous_close
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("matcher previous close lock poisoned")))?
            .insert(bar.symbol.clone(), bar.close);
        Ok(())
    }

    pub fn compute_cost(
        &self,
        side: OrderSide,
        price: Decimal,
        qty: i64,
        exchange: Exchange,
    ) -> CostBreakdown {
        compute_cost(&self.config, side, price, qty, exchange)
    }

    fn current_bar(&self, symbol: &str) -> Result<CurrentBar, TgError> {
        self.current_bars
            .read()
            .map_err(|_| TgError::Other(anyhow::anyhow!("matcher current bar lock poisoned")))?
            .get(symbol)
            .cloned()
            .ok_or_else(|| TgError::InvalidOrder(format!("no current bar for {symbol}")))
    }

    fn validate_intent(&self, intent: &OrderIntent, current: &CurrentBar) -> Result<(), TgError> {
        if intent.quantity <= 0 || intent.quantity % LOT_SIZE != 0 {
            return Err(TgError::InvalidOrder(format!(
                "quantity must be a positive multiple of {LOT_SIZE}"
            )));
        }

        if matches!(intent.order_type, OrderType::Limit) {
            let price = intent
                .price
                .ok_or_else(|| TgError::InvalidOrder("limit order requires price".to_owned()))?;
            if !valid_tick(price) {
                return Err(TgError::InvalidOrder(
                    "limit price must use 0.01 tick".to_owned(),
                ));
            }
            let pct = limit_up_pct(current.board);
            let upper = current.pre_close * (Decimal::ONE + pct);
            let lower = current.pre_close * (Decimal::ONE - pct);
            if price >= upper || price <= lower {
                return Err(TgError::InvalidOrder(format!(
                    "limit price {price} outside conservative band ({lower}, {upper})"
                )));
            }
        }

        let pct = limit_up_pct(current.board);
        let upper = current.pre_close * (Decimal::ONE + pct);
        let lower = current.pre_close * (Decimal::ONE - pct);
        if matches!(intent.side, OrderSide::Buy) && current.bar.close >= upper {
            return Err(TgError::RiskRejected(
                "buy rejected because bar closed at limit up".to_owned(),
            ));
        }
        if matches!(intent.side, OrderSide::Sell) && current.bar.close <= lower {
            return Err(TgError::RiskRejected(
                "sell rejected because bar closed at limit down".to_owned(),
            ));
        }

        Ok(())
    }

    fn match_price(&self, intent: &OrderIntent, bar: &Bar) -> Result<Option<Decimal>, TgError> {
        match intent.order_type {
            OrderType::Market => Ok(Some(apply_slippage(
                bar.close,
                intent.side,
                self.config.slippage_bps,
            ))),
            OrderType::Limit => {
                let price = intent.price.ok_or_else(|| {
                    TgError::InvalidOrder("limit order requires price".to_owned())
                })?;
                if bar.low <= price && price <= bar.high {
                    Ok(Some(apply_slippage(
                        price,
                        intent.side,
                        self.config.slippage_bps,
                    )))
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn assert_account_can_fill(
        &self,
        intent: &OrderIntent,
        fill_price: Decimal,
        cost: &CostBreakdown,
    ) -> Result<(), TgError> {
        let account = self.ledger.account();
        match intent.side {
            OrderSide::Buy => {
                let notional = fill_price * Decimal::from(intent.quantity);
                let total_cost = notional + cost.commission + cost.tax + cost.transfer_fee;
                if account.cash < total_cost {
                    return Err(TgError::RiskRejected(format!(
                        "insufficient cash: need {total_cost}, have {}",
                        account.cash
                    )));
                }
            }
            OrderSide::Sell => {
                let available = account
                    .positions
                    .get(&intent.symbol)
                    .map_or(0, |position| position.available_quantity);
                if available < intent.quantity {
                    return Err(TgError::RiskRejected(format!(
                        "T+1 or position check rejected sell: available {available}, requested {}",
                        intent.quantity
                    )));
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl ExecutionHandler for HistoricalMatcher {
    async fn submit(&self, intent: OrderIntent) -> std::result::Result<OrderId, TgError> {
        let current = self.current_bar(&intent.symbol)?;
        self.validate_intent(&intent, &current)?;

        let order_id = new_ulid_like();
        let created_at = current.bar.ts;
        let fill_price = self.match_price(&intent, &current.bar)?;
        let Some(fill_price) = fill_price else {
            let order = order_from_intent(order_id.clone(), &intent, created_at, OrderStatus::New);
            self.ledger.upsert_open_order(order.clone())?;
            self.pending
                .write()
                .map_err(|_| TgError::Other(anyhow::anyhow!("matcher pending lock poisoned")))?
                .insert(
                    order_id.clone(),
                    PendingOrder {
                        order,
                        submitted_bar: current.bar,
                    },
                );
            return Ok(order_id);
        };

        let cost = self.compute_cost(intent.side, fill_price, intent.quantity, intent.exchange);
        self.assert_account_can_fill(&intent, fill_price, &cost)?;

        let fill = Fill {
            order_id: order_id.clone(),
            fill_id: new_ulid_like(),
            symbol: intent.symbol,
            exchange: intent.exchange,
            side: intent.side,
            price: fill_price,
            quantity: intent.quantity,
            commission: cost.commission,
            tax: cost.tax,
            transfer_fee: cost.transfer_fee,
            ts: current.bar.ts,
            trading_date: current.bar.trading_date,
        };
        self.ledger.apply_fill(&fill)?;
        let _ = self.tx.send(fill);
        Ok(order_id)
    }

    async fn cancel(&self, order_id: &OrderId) -> std::result::Result<(), TgError> {
        let pending = self
            .pending
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("matcher pending lock poisoned")))?
            .remove(order_id);
        let _ = self.ledger.remove_open_order(order_id)?;
        match pending {
            Some(_) => Ok(()),
            None => Err(TgError::InvalidOrder(format!(
                "order {order_id} is not pending"
            ))),
        }
    }

    async fn snapshot_positions(&self) -> std::result::Result<Vec<Position>, TgError> {
        Ok(self.ledger.positions())
    }

    async fn snapshot_account(&self) -> std::result::Result<Account, TgError> {
        Ok(self.ledger.account())
    }

    fn fill_channel(&self) -> broadcast::Receiver<Fill> {
        self.tx.subscribe()
    }
}

pub fn compute_cost(
    config: &MatcherConfig,
    side: OrderSide,
    price: Decimal,
    qty: i64,
    exchange: Exchange,
) -> CostBreakdown {
    let notional = price * Decimal::from(qty);
    let commission_rate = if config.commission_rate > COMMISSION_MAX_PCT {
        COMMISSION_MAX_PCT
    } else {
        config.commission_rate
    };
    let raw_commission = notional * commission_rate;
    let commission = if raw_commission < config.min_commission && notional > Decimal::ZERO {
        config.min_commission
    } else {
        raw_commission
    };
    let tax = if matches!(side, OrderSide::Sell) {
        notional * STAMP_DUTY_PCT
    } else {
        Decimal::ZERO
    };
    let transfer_fee = if matches!(exchange, Exchange::Sh) {
        notional * TRANSFER_FEE_PCT
    } else {
        Decimal::ZERO
    };
    CostBreakdown {
        commission,
        tax,
        transfer_fee,
    }
}

fn order_from_intent(
    order_id: OrderId,
    intent: &OrderIntent,
    created_at: DateTime<Utc>,
    status: OrderStatus,
) -> Order {
    Order {
        id: order_id,
        client_order_id: intent.client_order_id.clone(),
        symbol: intent.symbol.clone(),
        exchange: intent.exchange,
        side: intent.side,
        order_type: intent.order_type,
        price: intent.price,
        quantity: intent.quantity,
        time_in_force: intent.time_in_force,
        strategy_tag: intent.strategy_tag,
        created_at,
        status,
        filled_quantity: 0,
        avg_fill_price: Decimal::ZERO,
    }
}

fn apply_slippage(price: Decimal, side: OrderSide, bps: Decimal) -> Decimal {
    if bps == Decimal::ZERO {
        return price;
    }
    let rate = bps / Decimal::from(10_000);
    match side {
        OrderSide::Buy => price * (Decimal::ONE + rate),
        OrderSide::Sell => price * (Decimal::ONE - rate),
    }
}

fn valid_tick(price: Decimal) -> bool {
    price.round_dp(2) == price
}

fn infer_board(symbol: &str, exchange: Exchange) -> Board {
    if matches!(exchange, Exchange::Bj) {
        Board::Bj
    } else if symbol.starts_with("688") {
        Board::Star
    } else if symbol.starts_with("300") || symbol.starts_with("301") {
        Board::ChiNext
    } else {
        Board::MainBoard
    }
}

fn apply_fill_to_account(account: &mut Account, fill: &Fill) {
    let fees = fill.commission + fill.tax + fill.transfer_fee;
    let notional = fill.price * Decimal::from(fill.quantity);

    match fill.side {
        OrderSide::Buy => {
            account.cash -= notional + fees;
            let entry = account
                .positions
                .entry(fill.symbol.clone())
                .or_insert_with(|| Position {
                    symbol: fill.symbol.clone(),
                    exchange: fill.exchange,
                    total_quantity: 0,
                    t1_locked_quantity: 0,
                    available_quantity: 0,
                    avg_cost: Decimal::ZERO,
                    last_price: fill.price,
                    market_value: Decimal::ZERO,
                    unrealized_pnl: Decimal::ZERO,
                });
            let old_quantity = entry.total_quantity;
            let new_quantity = old_quantity + fill.quantity;
            let old_cost_basis = entry.avg_cost * Decimal::from(old_quantity);
            let new_cost_basis = old_cost_basis + notional + fees;
            entry.total_quantity = new_quantity;
            entry.t1_locked_quantity += fill.quantity;
            entry.avg_cost = if new_quantity == 0 {
                Decimal::ZERO
            } else {
                new_cost_basis / Decimal::from(new_quantity)
            };
            entry.last_price = fill.price;
            entry.market_value = fill.price * Decimal::from(entry.total_quantity);
            entry.unrealized_pnl =
                (fill.price - entry.avg_cost) * Decimal::from(entry.total_quantity);
        }
        OrderSide::Sell => {
            account.cash += notional - fees;
            let mut remove_position = false;
            if let Some(entry) = account.positions.get_mut(&fill.symbol) {
                entry.total_quantity -= fill.quantity;
                entry.available_quantity = entry.available_quantity.saturating_sub(fill.quantity);
                if entry.total_quantity <= 0 {
                    remove_position = true;
                } else {
                    entry.t1_locked_quantity = entry.t1_locked_quantity.min(entry.total_quantity);
                    entry.last_price = fill.price;
                    entry.market_value = fill.price * Decimal::from(entry.total_quantity);
                    entry.unrealized_pnl =
                        (fill.price - entry.avg_cost) * Decimal::from(entry.total_quantity);
                }
            }
            if remove_position {
                account.positions.remove(&fill.symbol);
            }
        }
    }
}

fn recalculate_total_value(account: &mut Account) -> Decimal {
    let positions_value = account
        .positions
        .values()
        .fold(Decimal::ZERO, |acc, position| acc + position.market_value);
    account.total_value = account.cash + positions_value;
    account.total_value
}

fn new_ulid_like() -> String {
    let millis = Utc::now().timestamp_millis().max(0) as u64;
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let process = u64::from(std::process::id());
    let value = ((millis as u128) << 80) | ((process as u128 & 0xffff) << 64) | counter as u128;
    encode_crockford_128(value)
}

fn encode_crockford_128(mut value: u128) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut out = [b'0'; 26];
    for idx in (0..26).rev() {
        out[idx] = ALPHABET[(value & 31) as usize];
        value >>= 5;
    }
    String::from_utf8(out.to_vec()).expect("Crockford alphabet is valid UTF-8")
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, NaiveDate, TimeZone};
    use tg_contracts::{BarPeriod, StrategyStyle, TimeInForce};

    use super::*;

    fn dec(value: i64, scale: u32) -> Decimal {
        Decimal::new(value, scale)
    }

    fn bar(close: Decimal, date: NaiveDate) -> Bar {
        Bar {
            symbol: "600001".to_owned(),
            exchange: Exchange::Sh,
            period: BarPeriod::Min1,
            ts: Utc
                .with_ymd_and_hms(date.year(), date.month(), date.day(), 7, 0, 0)
                .unwrap(),
            trading_date: date,
            open: dec(1000, 2),
            high: dec(1200, 2),
            low: dec(900, 2),
            close,
            volume: 10_000,
            amount: dec(100_000, 2),
        }
    }

    fn intent(side: OrderSide, price: Option<Decimal>) -> OrderIntent {
        OrderIntent {
            client_order_id: "client-1".to_owned(),
            symbol: "600001".to_owned(),
            exchange: Exchange::Sh,
            side,
            order_type: if price.is_some() {
                OrderType::Limit
            } else {
                OrderType::Market
            },
            price,
            quantity: 100,
            time_in_force: TimeInForce::Day,
            strategy_tag: StrategyStyle::Swing,
        }
    }

    #[tokio::test]
    async fn limit_order_fills_inside_bar_range() {
        let matcher = HistoricalMatcher::new(MatcherConfig {
            initial_cash: dec(100_000, 0),
            ..MatcherConfig::default()
        });
        let date = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        matcher.set_current_bar(bar(dec(1050, 2), date)).unwrap();

        let mut rx = matcher.fill_channel();
        let order_id = matcher
            .submit(intent(OrderSide::Buy, Some(dec(1050, 2))))
            .await
            .unwrap();
        let fill = rx.recv().await.unwrap();

        assert_eq!(fill.order_id, order_id);
        assert_eq!(fill.price, dec(1050, 2));
        assert_eq!(fill.quantity, 100);
    }

    #[tokio::test]
    async fn rejects_limit_order_at_or_over_limit_band() {
        let matcher = HistoricalMatcher::new(MatcherConfig {
            initial_cash: dec(100_000, 0),
            default_board: Some(Board::MainBoard),
            ..MatcherConfig::default()
        });
        let date = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        matcher.set_current_bar(bar(dec(1000, 2), date)).unwrap();

        let err = matcher
            .submit(intent(OrderSide::Buy, Some(dec(1100, 2))))
            .await
            .unwrap_err();
        assert!(matches!(err, TgError::InvalidOrder(_)));
    }

    #[tokio::test]
    async fn t1_blocks_same_day_sell_after_buy() {
        let matcher = HistoricalMatcher::new(MatcherConfig {
            initial_cash: dec(100_000, 0),
            ..MatcherConfig::default()
        });
        let date = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        matcher.set_current_bar(bar(dec(1050, 2), date)).unwrap();
        matcher
            .submit(intent(OrderSide::Buy, Some(dec(1050, 2))))
            .await
            .unwrap();

        let err = matcher
            .submit(intent(OrderSide::Sell, Some(dec(1060, 2))))
            .await
            .unwrap_err();
        assert!(matches!(err, TgError::RiskRejected(_)));
    }

    #[test]
    fn cost_math_includes_commission_stamp_duty_and_sh_transfer_fee() {
        let config = MatcherConfig::default();
        let cost = compute_cost(&config, OrderSide::Sell, dec(1000, 2), 200, Exchange::Sh);
        assert_eq!(cost.commission, dec(60, 2));
        assert_eq!(cost.tax, dec(100, 2));
        assert_eq!(cost.transfer_fee, dec(2, 2));
    }

    #[test]
    fn duplicate_fill_application_is_ignored() {
        let ledger = BacktestLedger::with_cash(dec(100_000, 0));
        let date = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        let fill = Fill {
            order_id: "order".to_owned(),
            fill_id: "fill".to_owned(),
            symbol: "600001".to_owned(),
            exchange: Exchange::Sh,
            side: OrderSide::Buy,
            price: dec(1000, 2),
            quantity: 100,
            commission: Decimal::ZERO,
            tax: Decimal::ZERO,
            transfer_fee: Decimal::ZERO,
            ts: Utc.with_ymd_and_hms(2026, 6, 15, 7, 0, 0).unwrap(),
            trading_date: date,
        };
        ledger.apply_fill(&fill).unwrap();
        ledger.apply_fill(&fill).unwrap();
        assert_eq!(ledger.position("600001").unwrap().total_quantity, 100);
    }
}
