use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use tg_contracts::{
    Account, Bar, BarPeriod, Event, Exchange, Fill, OrderId, OrderIntent, OrderSide, OrderType,
    Position, Result, Snapshot, StrategyStyle, TgError, TimeInForce,
};
use tg_engine::{
    Clock, CrossSection, DataFeed, Engine, ExecutionHandler, InMemoryCrossSection,
    InMemoryPortfolio, Portfolio, Strategy, StrategyContext,
};
use tokio::sync::broadcast;

fn ts(offset_secs: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-15T02:00:00Z")
        .expect("valid timestamp")
        .with_timezone(&Utc)
        + chrono::Duration::seconds(offset_secs)
}

fn trading_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 6, 15).expect("valid date")
}

fn dec(value: i64, scale: u32) -> Decimal {
    Decimal::new(value, scale)
}

fn bar(symbol: &str, at: DateTime<Utc>) -> Bar {
    Bar {
        symbol: symbol.to_owned(),
        exchange: Exchange::Sh,
        period: BarPeriod::Min1,
        ts: at,
        trading_date: trading_date(),
        open: dec(1000, 2),
        high: dec(1100, 2),
        low: dec(900, 2),
        close: dec(1050, 2),
        volume: 10_000,
        amount: dec(105_000, 2),
    }
}

fn snapshot(symbol: &str, at: DateTime<Utc>) -> Snapshot {
    Snapshot {
        symbol: symbol.to_owned(),
        exchange: Exchange::Sh,
        ts: at,
        trading_date: trading_date(),
        last: dec(1050, 2),
        open: dec(1000, 2),
        high: dec(1100, 2),
        low: dec(900, 2),
        pre_close: dec(990, 2),
        volume: 10_000,
        amount: dec(105_000, 2),
        bid_price: [dec(1049, 2); 5],
        bid_volume: [100; 5],
        ask_price: [dec(1051, 2); 5],
        ask_volume: [100; 5],
    }
}

fn fill(symbol: &str, at: DateTime<Utc>, side: OrderSide, price: Decimal, quantity: i64) -> Fill {
    Fill {
        order_id: "order-1".to_owned(),
        fill_id: format!("fill-{symbol}-{quantity}"),
        symbol: symbol.to_owned(),
        exchange: Exchange::Sh,
        side,
        price,
        quantity,
        commission: dec(100, 2),
        tax: Decimal::ZERO,
        transfer_fee: Decimal::ZERO,
        ts: at,
        trading_date: trading_date(),
    }
}

#[derive(Clone)]
struct FixedClock {
    now: DateTime<Utc>,
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.now
    }

    fn trading_date(&self, _ts: DateTime<Utc>) -> NaiveDate {
        trading_date()
    }
}

struct MockDataFeed {
    events: VecDeque<Event>,
}

impl MockDataFeed {
    fn new(events: Vec<Event>) -> Self {
        Self {
            events: VecDeque::from(events),
        }
    }
}

#[async_trait]
impl DataFeed for MockDataFeed {
    async fn next_event(&mut self) -> Result<Option<Event>> {
        Ok(self.events.pop_front())
    }

    async fn peek_next_ts(&mut self) -> Result<Option<DateTime<Utc>>> {
        Ok(self.events.front().map(event_ts))
    }
}

struct MockExecutionHandler {
    tx: broadcast::Sender<Fill>,
    emitted_fill: Mutex<Option<Fill>>,
}

impl MockExecutionHandler {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            tx,
            emitted_fill: Mutex::new(None),
        }
    }

    fn with_fill(fill: Fill) -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            tx,
            emitted_fill: Mutex::new(Some(fill)),
        }
    }
}

#[async_trait]
impl ExecutionHandler for MockExecutionHandler {
    async fn submit(&self, _intent: OrderIntent) -> std::result::Result<OrderId, TgError> {
        let order_id = "order-1".to_owned();
        if let Some(fill) = self
            .emitted_fill
            .lock()
            .expect("fill mutex should not be poisoned")
            .take()
        {
            let _ = self.tx.send(fill);
        }
        Ok(order_id)
    }

    async fn cancel(&self, _order_id: &OrderId) -> std::result::Result<(), TgError> {
        Ok(())
    }

    async fn snapshot_positions(&self) -> std::result::Result<Vec<Position>, TgError> {
        Ok(Vec::new())
    }

    async fn snapshot_account(&self) -> std::result::Result<Account, TgError> {
        Ok(Account {
            cash: Decimal::ZERO,
            frozen_cash: Decimal::ZERO,
            total_value: Decimal::ZERO,
            positions: Default::default(),
        })
    }

    fn fill_channel(&self) -> broadcast::Receiver<Fill> {
        self.tx.subscribe()
    }
}

#[derive(Default)]
struct RecordingStrategy {
    seen: Arc<Mutex<Vec<String>>>,
    submit_on_bar: bool,
}

#[async_trait]
impl Strategy for RecordingStrategy {
    async fn on_init(&mut self, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn on_bar(&mut self, bar: &Bar, ctx: &mut StrategyContext<'_>) -> Result<()> {
        self.seen
            .lock()
            .expect("seen mutex should not be poisoned")
            .push(format!("bar:{}", bar.symbol));
        if self.submit_on_bar {
            ctx.broker
                .submit(OrderIntent {
                    client_order_id: "client-1".to_owned(),
                    symbol: bar.symbol.clone(),
                    exchange: bar.exchange,
                    side: OrderSide::Buy,
                    order_type: OrderType::Limit,
                    price: Some(bar.close),
                    quantity: 100,
                    time_in_force: TimeInForce::Day,
                    strategy_tag: StrategyStyle::Swing,
                })
                .await?;
        }
        Ok(())
    }

    async fn on_snapshot(&mut self, snap: &Snapshot, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        self.seen
            .lock()
            .expect("seen mutex should not be poisoned")
            .push(format!("snapshot:{}", snap.symbol));
        Ok(())
    }

    async fn on_timer(&mut self, _at: DateTime<Utc>, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        self.seen
            .lock()
            .expect("seen mutex should not be poisoned")
            .push("timer".to_owned());
        Ok(())
    }

    async fn on_fill(&mut self, fill: &Fill, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        self.seen
            .lock()
            .expect("seen mutex should not be poisoned")
            .push(format!("fill:{}", fill.symbol));
        Ok(())
    }

    async fn on_shutdown(&mut self, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        Ok(())
    }

    fn style(&self) -> StrategyStyle {
        StrategyStyle::Swing
    }
}

#[tokio::test]
async fn event_ordering() {
    let at = ts(0);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let strategy = RecordingStrategy {
        seen: Arc::clone(&seen),
        submit_on_bar: false,
    };
    let executor = Arc::new(MockExecutionHandler::new());
    let mut engine = Engine::builder()
        .with_feed(MockDataFeed::new(vec![
            Event::Bar(bar("600001", at)),
            Event::Snapshot(snapshot("600001", at)),
            Event::Timer(at),
            Event::Fill(fill("600001", at, OrderSide::Buy, dec(1000, 2), 100)),
            Event::Bar(bar("600002", at)),
        ]))
        .with_executor(executor)
        .with_clock(Arc::new(FixedClock { now: at }))
        .with_strategy(strategy)
        .with_portfolio(Arc::new(InMemoryPortfolio::with_cash(dec(100_000, 2))))
        .build()
        .expect("engine builds");

    let summary = engine.run().await.expect("engine runs");

    assert_eq!(summary.events_processed, 5);
    assert_eq!(
        *seen.lock().expect("seen mutex should not be poisoned"),
        vec![
            "fill:600001".to_owned(),
            "bar:600001".to_owned(),
            "bar:600002".to_owned(),
            "snapshot:600001".to_owned(),
            "timer".to_owned(),
        ]
    );
}

#[test]
fn portfolio_apply_fill() {
    let portfolio = InMemoryPortfolio::with_cash(dec(10_000_000, 2));
    let buy = fill("600001", ts(0), OrderSide::Buy, dec(1000, 2), 100);
    portfolio.apply_fill(&buy).expect("buy fill applies");

    let position = portfolio.position("600001").expect("position exists");
    assert_eq!(position.total_quantity, 100);
    assert_eq!(position.t1_locked_quantity, 100);
    assert_eq!(position.available_quantity, 0);
    assert_eq!(position.avg_cost, dec(1001, 2));
    assert_eq!(portfolio.account().cash, dec(9_899_900, 2));

    let sell = fill("600001", ts(60), OrderSide::Sell, dec(1200, 2), 40);
    portfolio.apply_fill(&sell).expect("sell fill applies");

    let position = portfolio.position("600001").expect("position remains");
    assert_eq!(position.total_quantity, 60);
    assert_eq!(position.t1_locked_quantity, 60);
    assert_eq!(position.avg_cost, dec(1001, 2));
    assert_eq!(portfolio.account().cash, dec(9_947_800, 2));
}

#[tokio::test]
async fn engine_end_to_end() {
    let at = ts(0);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let strategy = RecordingStrategy {
        seen: Arc::clone(&seen),
        submit_on_bar: true,
    };
    let portfolio = Arc::new(InMemoryPortfolio::with_cash(dec(100_000, 2)));
    let executor = Arc::new(MockExecutionHandler::with_fill(fill(
        "600001",
        at,
        OrderSide::Buy,
        dec(1050, 2),
        100,
    )));

    let mut engine = Engine::builder()
        .with_feed(MockDataFeed::new(vec![
            Event::Bar(bar("600001", at)),
            Event::Bar(bar("600001", ts(60))),
        ]))
        .with_executor(executor)
        .with_clock(Arc::new(FixedClock { now: at }))
        .with_strategy(strategy)
        .with_portfolio(portfolio.clone())
        .build()
        .expect("engine builds");

    let summary = engine.run().await.expect("engine runs");

    assert_eq!(summary.fills_processed, 1);
    assert!(seen
        .lock()
        .expect("seen mutex should not be poisoned")
        .contains(&"fill:600001".to_owned()));
    assert_eq!(
        portfolio
            .position("600001")
            .expect("portfolio reflects fill")
            .total_quantity,
        100
    );
}

#[tokio::test]
async fn cross_section_snapshot() {
    let at = ts(0);
    let cross_section = Arc::new(InMemoryCrossSection::new());
    let executor = Arc::new(MockExecutionHandler::new());
    let mut engine = Engine::builder()
        .with_feed(MockDataFeed::new(vec![
            Event::Bar(bar("600001", at)),
            Event::Snapshot(snapshot("600002", ts(60))),
        ]))
        .with_executor(executor)
        .with_clock(Arc::new(FixedClock { now: at }))
        .with_strategy(RecordingStrategy::default())
        .with_cross_section(Arc::clone(&cross_section))
        .build()
        .expect("engine builds");

    engine.run().await.expect("engine runs");

    assert_eq!(
        cross_section
            .latest_bar("600001")
            .expect("latest bar")
            .symbol,
        "600001"
    );
    assert_eq!(
        cross_section
            .latest_snapshot("600002")
            .expect("latest snapshot")
            .symbol,
        "600002"
    );
    assert_eq!(
        cross_section.universe(),
        vec!["600001".to_owned(), "600002".to_owned()]
    );
}

fn event_ts(event: &Event) -> DateTime<Utc> {
    match event {
        Event::Bar(bar) => bar.ts,
        Event::Snapshot(snapshot) => snapshot.ts,
        Event::Timer(at) => *at,
        Event::Fill(fill) => fill.ts,
    }
}
