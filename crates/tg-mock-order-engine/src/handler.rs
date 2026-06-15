use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use tg_contracts::{
    Account, Board, Fill, Order, OrderId, OrderIntent, OrderSide, OrderStatus, OrderType, Position,
    Snapshot, TgError,
};
use tg_engine::ExecutionHandler;
use tokio::sync::broadcast;

use crate::account::VirtualAccount;
use crate::cost::{calculate_cost, CostConfig};
use crate::matcher::{MatchConfig, MatchEngine};
use crate::risk::{RiskConfig, RiskEngine};
use crate::rules::{is_t0_candidate, InstrumentRuleMeta, RuleEngine};

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn new_id(prefix: &str) -> String {
    let seq = ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("{prefix}_{}_{}", Utc::now().timestamp_millis(), seq)
}

#[derive(Debug, Clone)]
pub struct MockExecutionConfig {
    pub initial_cash: Decimal,
    pub default_board: Board,
    pub cost: CostConfig,
    pub match_config: MatchConfig,
    pub risk: RiskConfig,
}

impl Default for MockExecutionConfig {
    fn default() -> Self {
        Self {
            initial_cash: Decimal::new(1_000_000, 0),
            default_board: Board::MainBoard,
            cost: CostConfig::default(),
            match_config: MatchConfig::default(),
            risk: RiskConfig::default(),
        }
    }
}

#[derive(Debug, Clone)]
struct Reservation {
    cash: Decimal,
    sell_quantity: i64,
    original_quantity: i64,
}

#[derive(Debug)]
struct State {
    account: VirtualAccount,
    orders: HashMap<OrderId, Order>,
    latest: HashMap<String, Snapshot>,
    meta: HashMap<String, InstrumentRuleMeta>,
    reservations: HashMap<OrderId, Reservation>,
    hard_exit_triggered: HashSet<String>,
    today: NaiveDate,
}

pub struct MockExecutionHandler {
    state: Mutex<State>,
    rules: RuleEngine,
    matcher: MatchEngine,
    risk: RiskEngine,
    fills: broadcast::Sender<Fill>,
    order_repo: Option<Arc<dyn tg_persistence::OrderRepo>>,
    fill_repo: Option<Arc<dyn tg_persistence::FillRepo>>,
    account_repo: Option<Arc<dyn tg_persistence::AccountStateRepo>>,
}

impl MockExecutionHandler {
    pub fn new(config: MockExecutionConfig) -> Self {
        let today = Utc::now().date_naive();
        let rules = RuleEngine {
            cost: config.cost,
            blacklist: config.risk.blacklist.clone(),
            ..RuleEngine::default()
        };
        let matcher = MatchEngine::new(config.match_config, config.cost);
        let risk = RiskEngine::new(config.risk);
        let (fills, _) = broadcast::channel(1024);
        Self {
            state: Mutex::new(State {
                account: VirtualAccount::new(config.initial_cash),
                orders: HashMap::new(),
                latest: HashMap::new(),
                meta: HashMap::new(),
                reservations: HashMap::new(),
                hard_exit_triggered: HashSet::new(),
                today,
            }),
            rules,
            matcher,
            risk,
            fills,
            order_repo: None,
            fill_repo: None,
            account_repo: None,
        }
    }

    pub fn with_repos(
        mut self,
        order_repo: Arc<dyn tg_persistence::OrderRepo>,
        fill_repo: Arc<dyn tg_persistence::FillRepo>,
        account_repo: Arc<dyn tg_persistence::AccountStateRepo>,
    ) -> Self {
        self.order_repo = Some(order_repo);
        self.fill_repo = Some(fill_repo);
        self.account_repo = Some(account_repo);
        self
    }

    pub fn set_instrument_meta(&self, meta: InstrumentRuleMeta) -> Result<(), TgError> {
        self.lock_state()?.meta.insert(meta.symbol.clone(), meta);
        Ok(())
    }

    pub async fn on_snapshot(&self, snapshot: &Snapshot) -> Result<Vec<Fill>, TgError> {
        let (fills, hard_intents) = {
            let mut state = self.lock_state()?;
            state.today = snapshot.trading_date;
            state.account.update_market(&snapshot.symbol, snapshot.last);
            state
                .latest
                .insert(snapshot.symbol.clone(), snapshot.clone());
            let meta = get_or_default_meta(&mut state, snapshot);
            let mut fills = Vec::new();
            let order_ids: Vec<_> = state.orders.keys().cloned().collect();
            for order_id in order_ids {
                let Some(order) = state.orders.get(&order_id).cloned() else {
                    continue;
                };
                let Some(fill) = self.matcher.try_match(&order, snapshot, &meta) else {
                    continue;
                };
                apply_fill_to_state(&mut state, &fill, &meta, &self.fills)?;
                fills.push(fill);
            }

            let hard_intents: Vec<_> = state
                .account
                .positions(snapshot.trading_date)
                .into_iter()
                .filter(|position| position.symbol == snapshot.symbol)
                .filter_map(|position| {
                    if state.hard_exit_triggered.contains(&position.symbol) {
                        None
                    } else {
                        self.risk.hard_exit_intent(&position, snapshot)
                    }
                })
                .collect();
            for intent in &hard_intents {
                state.hard_exit_triggered.insert(intent.symbol.clone());
            }
            (fills, hard_intents)
        };

        for fill in &fills {
            let order = self.get_order(&fill.order_id)?;
            self.persist_order(&order).await?;
            self.persist_fill(fill).await?;
        }
        for intent in hard_intents {
            let _ = self.submit(intent).await?;
        }
        Ok(fills)
    }

    pub fn get_order(&self, order_id: &str) -> Result<Order, TgError> {
        self.lock_state()?
            .orders
            .get(order_id)
            .cloned()
            .ok_or_else(|| TgError::NotFound(format!("order not found: {order_id}")))
    }

    async fn persist_order(&self, order: &Order) -> Result<(), TgError> {
        if let Some(repo) = &self.order_repo {
            repo.save_order(order).await?;
        }
        Ok(())
    }

    async fn persist_fill(&self, fill: &Fill) -> Result<(), TgError> {
        if let Some(repo) = &self.fill_repo {
            repo.save_fill(fill).await?;
        }
        if let Some(repo) = &self.account_repo {
            let account = {
                let state = self.lock_state()?;
                state.account.account(fill.trading_date)
            };
            repo.save_account(&account, fill.trading_date).await?;
        }
        Ok(())
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, State>, TgError> {
        self.state
            .lock()
            .map_err(|_| TgError::Other(anyhow::anyhow!("mock execution state lock poisoned")))
    }
}

#[async_trait]
impl ExecutionHandler for MockExecutionHandler {
    async fn submit(&self, intent: OrderIntent) -> Result<OrderId, TgError> {
        let order = {
            let mut state = self.lock_state()?;
            let meta = state
                .meta
                .get(&intent.symbol)
                .cloned()
                .or_else(|| {
                    state
                        .latest
                        .get(&intent.symbol)
                        .map(default_meta_from_snapshot)
                })
                .ok_or_else(|| {
                    TgError::InvalidOrder(
                        "instrument metadata or latest snapshot required".to_owned(),
                    )
                })?;
            let latest = state.latest.get(&intent.symbol);
            let reservation_cash =
                self.rules
                    .validate_submit(&intent, &meta, &state.account, latest)?;

            let account = state.account.account(state.today);
            let current_position_value = account
                .positions
                .get(&intent.symbol)
                .map(|position| position.market_value)
                .unwrap_or_default();
            let total_position_value = account
                .positions
                .values()
                .fold(Decimal::ZERO, |acc, position| acc + position.market_value);
            let order_value = estimate_order_value(&intent, latest);
            self.risk
                .validate_soft(
                    &intent,
                    account.total_value,
                    current_position_value,
                    order_value,
                    total_position_value,
                )
                .map_err(TgError::RiskRejected)?;

            let order = Order {
                id: new_id("ord"),
                client_order_id: intent.client_order_id,
                symbol: intent.symbol,
                exchange: intent.exchange,
                side: intent.side,
                order_type: intent.order_type,
                price: intent.price,
                quantity: intent.quantity,
                time_in_force: intent.time_in_force,
                strategy_tag: intent.strategy_tag,
                created_at: Utc::now(),
                status: OrderStatus::New,
                filled_quantity: 0,
                avg_fill_price: Decimal::ZERO,
            };

            match order.side {
                OrderSide::Buy => state.account.freeze_cash(reservation_cash)?,
                OrderSide::Sell => {
                    let today = state.today;
                    state
                        .account
                        .reserve_sell(&order.symbol, order.quantity, today)?
                }
            }
            state.reservations.insert(
                order.id.clone(),
                Reservation {
                    cash: reservation_cash,
                    sell_quantity: if matches!(order.side, OrderSide::Sell) {
                        order.quantity
                    } else {
                        0
                    },
                    original_quantity: order.quantity,
                },
            );
            state.orders.insert(order.id.clone(), order.clone());
            order
        };

        self.persist_order(&order).await?;
        Ok(order.id)
    }

    async fn cancel(&self, order_id: &OrderId) -> Result<(), TgError> {
        let order = {
            let mut state = self.lock_state()?;
            let order = state
                .orders
                .get_mut(order_id)
                .ok_or_else(|| TgError::NotFound(format!("order not found: {order_id}")))?;
            if matches!(
                order.status,
                OrderStatus::Filled | OrderStatus::Cancelled | OrderStatus::Rejected
            ) {
                return Ok(());
            }
            order.status = OrderStatus::Cancelled;
            let order = order.clone();
            if let Some(reservation) = state.reservations.remove(order_id) {
                state.account.release_cash(reservation.cash);
                state
                    .account
                    .release_sell(&order.symbol, reservation.sell_quantity);
            }
            order
        };
        self.persist_order(&order).await
    }

    async fn snapshot_positions(&self) -> Result<Vec<Position>, TgError> {
        let state = self.lock_state()?;
        Ok(state.account.positions(state.today))
    }

    async fn snapshot_account(&self) -> Result<Account, TgError> {
        let state = self.lock_state()?;
        Ok(state.account.account(state.today))
    }

    fn fill_channel(&self) -> broadcast::Receiver<Fill> {
        self.fills.subscribe()
    }
}

fn apply_fill_to_state(
    state: &mut State,
    fill: &Fill,
    meta: &InstrumentRuleMeta,
    fills_tx: &broadcast::Sender<Fill>,
) -> Result<(), TgError> {
    let order = state
        .orders
        .get_mut(&fill.order_id)
        .ok_or_else(|| TgError::NotFound(format!("order not found: {}", fill.order_id)))?;
    let old_filled = order.filled_quantity;
    let new_filled = old_filled + fill.quantity;
    let old_value = order.avg_fill_price * Decimal::from(old_filled);
    let new_value = old_value + fill.price * Decimal::from(fill.quantity);
    order.filled_quantity = new_filled;
    order.avg_fill_price = new_value / Decimal::from(new_filled);
    order.status = if new_filled >= order.quantity {
        OrderStatus::Filled
    } else {
        OrderStatus::PartiallyFilled
    };
    let order_side = order.side;
    let order_symbol = order.symbol.clone();
    let order_strategy = order.strategy_tag;
    let order_quantity = order.quantity;
    let order_id = order.id.clone();
    let _ = order;

    if let Some(reservation) = state.reservations.get_mut(&order_id) {
        match order_side {
            OrderSide::Buy => {
                let release = reservation.cash * Decimal::from(fill.quantity)
                    / Decimal::from(reservation.original_quantity);
                reservation.cash = (reservation.cash - release).max(Decimal::ZERO);
                state.account.release_cash(release);
            }
            OrderSide::Sell => {
                reservation.sell_quantity = (reservation.sell_quantity - fill.quantity).max(0);
            }
        }
        if new_filled >= order_quantity {
            let remaining = state.reservations.remove(&order_id);
            if let Some(remaining) = remaining {
                state.account.release_cash(remaining.cash);
                state
                    .account
                    .release_sell(&order_symbol, remaining.sell_quantity);
            }
        }
    }

    let t0 = is_t0_candidate(meta, order_strategy);
    state.account.apply_fill(fill, t0)?;
    let _ = fills_tx.send(fill.clone());
    Ok(())
}

fn get_or_default_meta(state: &mut State, snapshot: &Snapshot) -> InstrumentRuleMeta {
    state
        .meta
        .entry(snapshot.symbol.clone())
        .or_insert_with(|| default_meta_from_snapshot(snapshot))
        .clone()
}

fn default_meta_from_snapshot(snapshot: &Snapshot) -> InstrumentRuleMeta {
    InstrumentRuleMeta::stock(snapshot.symbol.clone(), snapshot.exchange, Board::MainBoard)
}

fn estimate_order_value(intent: &OrderIntent, latest: Option<&Snapshot>) -> Decimal {
    let price = match (intent.order_type, intent.price, latest, intent.side) {
        (OrderType::Limit, Some(price), _, _) => price,
        (OrderType::Market, _, Some(snapshot), OrderSide::Buy) => snapshot.ask_price[0],
        (OrderType::Market, _, Some(snapshot), OrderSide::Sell) => snapshot.bid_price[0],
        _ => Decimal::ZERO,
    };
    let costs = calculate_cost(
        intent.side,
        intent.exchange,
        tg_contracts::InstrumentType::Stock,
        price,
        intent.quantity,
        CostConfig::default(),
    );
    price * Decimal::from(intent.quantity) + costs.total()
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone, Utc};
    use rust_decimal::Decimal;
    use tg_contracts::{Exchange, OrderIntent, OrderSide, OrderType, StrategyStyle, TimeInForce};
    use tg_engine::ExecutionHandler;

    use super::{MockExecutionConfig, MockExecutionHandler};

    fn snapshot() -> tg_contracts::Snapshot {
        tg_contracts::Snapshot {
            symbol: "600000".to_owned(),
            exchange: Exchange::Sh,
            ts: Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 0).unwrap(),
            trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
            last: Decimal::new(10, 0),
            open: Decimal::new(10, 0),
            high: Decimal::new(10, 0),
            low: Decimal::new(10, 0),
            pre_close: Decimal::new(10, 0),
            volume: 10_000,
            amount: Decimal::new(100_000, 0),
            bid_price: [Decimal::new(999, 2); 5],
            bid_volume: [1_000; 5],
            ask_price: [Decimal::new(1001, 2); 5],
            ask_volume: [1_000; 5],
        }
    }

    #[tokio::test]
    async fn submit_fill_channel_and_account_snapshots_work() {
        let handler = MockExecutionHandler::new(MockExecutionConfig::default());
        handler.on_snapshot(&snapshot()).await.unwrap();
        let mut rx = handler.fill_channel();
        let order_id = handler
            .submit(OrderIntent {
                client_order_id: "c1".to_owned(),
                symbol: "600000".to_owned(),
                exchange: Exchange::Sh,
                side: OrderSide::Buy,
                order_type: OrderType::Limit,
                price: Some(Decimal::new(1010, 2)),
                quantity: 100,
                time_in_force: TimeInForce::Day,
                strategy_tag: StrategyStyle::Swing,
            })
            .await
            .unwrap();
        handler.on_snapshot(&snapshot()).await.unwrap();
        let fill = rx.recv().await.unwrap();
        assert_eq!(fill.order_id, order_id);
        assert_eq!(
            handler.snapshot_positions().await.unwrap()[0].total_quantity,
            100
        );
        assert!(handler.snapshot_account().await.unwrap().cash < Decimal::new(1_000_000, 0));
    }
}
