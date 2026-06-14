use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use tg_contracts::{
    Account, Bar, Event, Fill, Order, OrderId, OrderIntent, Position, Result, Snapshot,
    StrategyStyle, TgError,
};

/// Strategy-facing context assembled by the engine for each callback.
pub struct StrategyContext<'a> {
    pub now: DateTime<Utc>,
    pub clock: &'a dyn Clock,
    pub portfolio: &'a dyn Portfolio,
    pub cross_section: &'a dyn CrossSection,
    pub broker: &'a dyn OrderSink,
}

#[async_trait]
pub trait Strategy: Send + Sync {
    async fn on_init(&mut self, ctx: &mut StrategyContext<'_>) -> Result<()>;

    async fn on_bar(&mut self, bar: &Bar, ctx: &mut StrategyContext<'_>) -> Result<()>;

    async fn on_snapshot(&mut self, snap: &Snapshot, ctx: &mut StrategyContext<'_>) -> Result<()>;

    async fn on_timer(&mut self, at: DateTime<Utc>, ctx: &mut StrategyContext<'_>) -> Result<()>;

    async fn on_fill(&mut self, fill: &Fill, ctx: &mut StrategyContext<'_>) -> Result<()>;

    async fn on_shutdown(&mut self, ctx: &mut StrategyContext<'_>) -> Result<()>;

    fn style(&self) -> StrategyStyle;
}

#[async_trait]
pub trait ExecutionHandler: Send + Sync {
    async fn submit(&self, intent: OrderIntent) -> std::result::Result<OrderId, TgError>;

    async fn cancel(&self, order_id: &OrderId) -> std::result::Result<(), TgError>;

    async fn snapshot_positions(&self) -> std::result::Result<Vec<Position>, TgError>;

    async fn snapshot_account(&self) -> std::result::Result<Account, TgError>;

    fn fill_channel(&self) -> tokio::sync::broadcast::Receiver<Fill>;
}

#[async_trait]
pub trait DataFeed: Send + Sync {
    async fn next_event(&mut self) -> Result<Option<Event>>;

    async fn peek_next_ts(&mut self) -> Result<Option<DateTime<Utc>>>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;

    fn trading_date(&self, ts: DateTime<Utc>) -> NaiveDate;
}

pub trait Portfolio: Send + Sync {
    fn account(&self) -> Account;

    fn position(&self, symbol: &str) -> Option<Position>;

    fn positions(&self) -> Vec<Position>;

    fn open_orders(&self) -> Vec<Order>;

    fn apply_fill(&self, fill: &Fill) -> Result<()>;
}

#[async_trait]
pub trait OrderSink: Send + Sync {
    async fn submit(&self, intent: OrderIntent) -> std::result::Result<OrderId, TgError>;

    async fn cancel(&self, order_id: &OrderId) -> std::result::Result<(), TgError>;
}

pub trait CrossSection: Send + Sync {
    fn latest_bar(&self, symbol: &str) -> Option<Bar>;

    fn latest_snapshot(&self, symbol: &str) -> Option<Snapshot>;

    fn universe(&self) -> Vec<String>;
}
