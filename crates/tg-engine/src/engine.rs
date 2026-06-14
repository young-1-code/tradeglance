use std::{cmp::Ordering, collections::BinaryHeap, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use tg_contracts::{Event, Fill, OrderId, OrderIntent, Result, TgError};
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::{
    portfolio::{InMemoryCrossSection, InMemoryPortfolio},
    traits::{Clock, DataFeed, ExecutionHandler, OrderSink, Portfolio, Strategy, StrategyContext},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub events_processed: u64,
    pub bars_processed: u64,
    pub snapshots_processed: u64,
    pub fills_processed: u64,
    pub timers_processed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EventPriority {
    Fill = 0,
    Bar = 1,
    Snapshot = 2,
    Timer = 3,
}

#[derive(Debug)]
struct QueuedEvent {
    ts: DateTime<Utc>,
    priority: EventPriority,
    seq: u64,
    event: Event,
}

impl QueuedEvent {
    fn new(event: Event, seq: u64) -> Self {
        let ts = event_ts(&event);
        let priority = event_priority(&event);
        Self {
            ts,
            priority,
            seq,
            event,
        }
    }
}

impl Eq for QueuedEvent {}

impl PartialEq for QueuedEvent {
    fn eq(&self, other: &Self) -> bool {
        self.ts == other.ts && self.priority == other.priority && self.seq == other.seq
    }
}

impl Ord for QueuedEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .ts
            .cmp(&self.ts)
            .then_with(|| other.priority.cmp(&self.priority))
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for QueuedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct Engine {
    feed: Box<dyn DataFeed>,
    executor: Arc<dyn ExecutionHandler>,
    clock: Arc<dyn Clock>,
    strategies: Vec<Box<dyn Strategy>>,
    timer_queue: BinaryHeap<QueuedEvent>,
    pending_events: BinaryHeap<QueuedEvent>,
    fill_rx: broadcast::Receiver<Fill>,
    portfolio: Arc<dyn Portfolio>,
    cross_section: Arc<InMemoryCrossSection>,
    broker: Arc<dyn OrderSink>,
    seq: u64,
    current_time: DateTime<Utc>,
    summary: RunSummary,
}

impl Engine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        feed: Box<dyn DataFeed>,
        executor: Arc<dyn ExecutionHandler>,
        clock: Arc<dyn Clock>,
        strategies: Vec<Box<dyn Strategy>>,
        portfolio: Arc<dyn Portfolio>,
        cross_section: Arc<InMemoryCrossSection>,
    ) -> Self {
        let fill_rx = executor.fill_channel();
        let broker = Arc::new(ExecutionOrderSink {
            executor: Arc::clone(&executor),
        });
        let current_time = clock.now();
        Self {
            feed,
            executor,
            clock,
            strategies,
            timer_queue: BinaryHeap::new(),
            pending_events: BinaryHeap::new(),
            fill_rx,
            portfolio,
            cross_section,
            broker,
            seq: 0,
            current_time,
            summary: RunSummary {
                events_processed: 0,
                bars_processed: 0,
                snapshots_processed: 0,
                fills_processed: 0,
                timers_processed: 0,
            },
        }
    }

    pub fn builder() -> EngineBuilder {
        EngineBuilder::default()
    }

    pub fn schedule_timer(&mut self, at: DateTime<Utc>) {
        self.push_timer(at);
    }

    pub fn executor(&self) -> Arc<dyn ExecutionHandler> {
        Arc::clone(&self.executor)
    }

    pub async fn run(&mut self) -> Result<RunSummary> {
        debug!("starting engine");
        self.drain_fills();
        self.call_on_init().await?;

        while let Some(event) = self.pick_next_event().await? {
            self.dispatch(event).await?;
            self.drain_fills();
            self.drain_due_timers();
        }

        self.call_on_shutdown().await?;
        debug!("engine stopped");
        Ok(self.summary.clone())
    }

    async fn call_on_init(&mut self) -> Result<()> {
        for strategy in &mut self.strategies {
            let context_clock = ContextClock {
                now: self.current_time,
                delegate: Arc::clone(&self.clock),
            };
            let portfolio = Arc::clone(&self.portfolio);
            let cross_section = Arc::clone(&self.cross_section);
            let broker = Arc::clone(&self.broker);
            let mut ctx = StrategyContext {
                now: self.current_time,
                clock: &context_clock,
                portfolio: portfolio.as_ref(),
                cross_section: cross_section.as_ref(),
                broker: broker.as_ref(),
            };
            strategy.on_init(&mut ctx).await?;
        }
        Ok(())
    }

    async fn call_on_shutdown(&mut self) -> Result<()> {
        for strategy in &mut self.strategies {
            let context_clock = ContextClock {
                now: self.current_time,
                delegate: Arc::clone(&self.clock),
            };
            let portfolio = Arc::clone(&self.portfolio);
            let cross_section = Arc::clone(&self.cross_section);
            let broker = Arc::clone(&self.broker);
            let mut ctx = StrategyContext {
                now: self.current_time,
                clock: &context_clock,
                portfolio: portfolio.as_ref(),
                cross_section: cross_section.as_ref(),
                broker: broker.as_ref(),
            };
            strategy.on_shutdown(&mut ctx).await?;
        }
        Ok(())
    }

    async fn dispatch(&mut self, event: Event) -> Result<()> {
        self.current_time = event_ts(&event);
        self.summary.events_processed += 1;

        match event {
            Event::Bar(bar) => {
                self.cross_section.update_bar(bar.clone())?;
                self.summary.bars_processed += 1;
                for strategy in &mut self.strategies {
                    let context_clock = ContextClock {
                        now: self.current_time,
                        delegate: Arc::clone(&self.clock),
                    };
                    let portfolio = Arc::clone(&self.portfolio);
                    let cross_section = Arc::clone(&self.cross_section);
                    let broker = Arc::clone(&self.broker);
                    let mut ctx = StrategyContext {
                        now: self.current_time,
                        clock: &context_clock,
                        portfolio: portfolio.as_ref(),
                        cross_section: cross_section.as_ref(),
                        broker: broker.as_ref(),
                    };
                    strategy.on_bar(&bar, &mut ctx).await?;
                }
            }
            Event::Snapshot(snapshot) => {
                self.cross_section.update_snapshot(snapshot.clone())?;
                self.summary.snapshots_processed += 1;
                for strategy in &mut self.strategies {
                    let context_clock = ContextClock {
                        now: self.current_time,
                        delegate: Arc::clone(&self.clock),
                    };
                    let portfolio = Arc::clone(&self.portfolio);
                    let cross_section = Arc::clone(&self.cross_section);
                    let broker = Arc::clone(&self.broker);
                    let mut ctx = StrategyContext {
                        now: self.current_time,
                        clock: &context_clock,
                        portfolio: portfolio.as_ref(),
                        cross_section: cross_section.as_ref(),
                        broker: broker.as_ref(),
                    };
                    strategy.on_snapshot(&snapshot, &mut ctx).await?;
                }
            }
            Event::Timer(at) => {
                self.summary.timers_processed += 1;
                for strategy in &mut self.strategies {
                    let context_clock = ContextClock {
                        now: self.current_time,
                        delegate: Arc::clone(&self.clock),
                    };
                    let portfolio = Arc::clone(&self.portfolio);
                    let cross_section = Arc::clone(&self.cross_section);
                    let broker = Arc::clone(&self.broker);
                    let mut ctx = StrategyContext {
                        now: self.current_time,
                        clock: &context_clock,
                        portfolio: portfolio.as_ref(),
                        cross_section: cross_section.as_ref(),
                        broker: broker.as_ref(),
                    };
                    strategy.on_timer(at, &mut ctx).await?;
                }
            }
            Event::Fill(fill) => {
                self.portfolio.apply_fill(&fill)?;
                self.summary.fills_processed += 1;
                for strategy in &mut self.strategies {
                    let context_clock = ContextClock {
                        now: self.current_time,
                        delegate: Arc::clone(&self.clock),
                    };
                    let portfolio = Arc::clone(&self.portfolio);
                    let cross_section = Arc::clone(&self.cross_section);
                    let broker = Arc::clone(&self.broker);
                    let mut ctx = StrategyContext {
                        now: self.current_time,
                        clock: &context_clock,
                        portfolio: portfolio.as_ref(),
                        cross_section: cross_section.as_ref(),
                        broker: broker.as_ref(),
                    };
                    strategy.on_fill(&fill, &mut ctx).await?;
                }
            }
        }

        Ok(())
    }

    async fn pick_next_event(&mut self) -> Result<Option<Event>> {
        self.load_next_timestamp_group().await?;
        self.drain_due_timers();
        Ok(self.pending_events.pop().map(|entry| entry.event))
    }

    async fn load_next_timestamp_group(&mut self) -> Result<()> {
        loop {
            let feed_ts = self.feed.peek_next_ts().await?;
            let pending_ts = self.pending_events.peek().map(|entry| entry.ts);
            let timer_ts = self.timer_queue.peek().map(|entry| entry.ts);

            let next_ts = [feed_ts, pending_ts, timer_ts].into_iter().flatten().min();
            let Some(next_ts) = next_ts else {
                return Ok(());
            };

            while self
                .timer_queue
                .peek()
                .is_some_and(|entry| entry.ts == next_ts)
            {
                if let Some(entry) = self.timer_queue.pop() {
                    self.pending_events.push(entry);
                }
            }

            if feed_ts == Some(next_ts) {
                while self.feed.peek_next_ts().await? == Some(next_ts) {
                    let Some(event) = self.feed.next_event().await? else {
                        break;
                    };
                    self.push_pending(event);
                }
                continue;
            }

            return Ok(());
        }
    }

    fn drain_fills(&mut self) {
        loop {
            match self.fill_rx.try_recv() {
                Ok(fill) => self.push_pending(Event::Fill(fill)),
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
                    warn!(skipped, "engine fill receiver lagged");
                    continue;
                }
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }
    }

    fn drain_due_timers(&mut self) {
        while self
            .timer_queue
            .peek()
            .is_some_and(|entry| entry.ts <= self.current_time)
        {
            if let Some(entry) = self.timer_queue.pop() {
                self.pending_events.push(entry);
            }
        }
    }

    fn push_timer(&mut self, at: DateTime<Utc>) {
        self.seq += 1;
        self.timer_queue
            .push(QueuedEvent::new(Event::Timer(at), self.seq));
    }

    fn push_pending(&mut self, event: Event) {
        self.seq += 1;
        self.pending_events.push(QueuedEvent::new(event, self.seq));
    }
}

#[derive(Default)]
pub struct EngineBuilder {
    feed: Option<Box<dyn DataFeed>>,
    executor: Option<Arc<dyn ExecutionHandler>>,
    clock: Option<Arc<dyn Clock>>,
    strategies: Vec<Box<dyn Strategy>>,
    portfolio: Option<Arc<dyn Portfolio>>,
    cross_section: Option<Arc<InMemoryCrossSection>>,
}

impl EngineBuilder {
    pub fn with_feed(mut self, feed: impl DataFeed + 'static) -> Self {
        self.feed = Some(Box::new(feed));
        self
    }

    pub fn with_boxed_feed(mut self, feed: Box<dyn DataFeed>) -> Self {
        self.feed = Some(feed);
        self
    }

    pub fn with_executor(mut self, executor: Arc<dyn ExecutionHandler>) -> Self {
        self.executor = Some(executor);
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    pub fn with_strategy(mut self, strategy: impl Strategy + 'static) -> Self {
        self.strategies.push(Box::new(strategy));
        self
    }

    pub fn with_strategies(mut self, strategies: Vec<Box<dyn Strategy>>) -> Self {
        self.strategies = strategies;
        self
    }

    pub fn with_portfolio(mut self, portfolio: Arc<dyn Portfolio>) -> Self {
        self.portfolio = Some(portfolio);
        self
    }

    pub fn with_cross_section(mut self, cross_section: Arc<InMemoryCrossSection>) -> Self {
        self.cross_section = Some(cross_section);
        self
    }

    pub fn build(self) -> Result<Engine> {
        let feed = self
            .feed
            .ok_or_else(|| TgError::Validation("engine feed is required".to_owned()))?;
        let executor = self
            .executor
            .ok_or_else(|| TgError::Validation("engine executor is required".to_owned()))?;
        let clock = self
            .clock
            .ok_or_else(|| TgError::Validation("engine clock is required".to_owned()))?;
        let portfolio = self
            .portfolio
            .unwrap_or_else(|| Arc::new(InMemoryPortfolio::default()));
        let cross_section = self
            .cross_section
            .unwrap_or_else(|| Arc::new(InMemoryCrossSection::new()));

        Ok(Engine::new(
            feed,
            executor,
            clock,
            self.strategies,
            portfolio,
            cross_section,
        ))
    }
}

struct ExecutionOrderSink {
    executor: Arc<dyn ExecutionHandler>,
}

#[async_trait]
impl OrderSink for ExecutionOrderSink {
    async fn submit(&self, intent: OrderIntent) -> std::result::Result<OrderId, TgError> {
        self.executor.submit(intent).await
    }

    async fn cancel(&self, order_id: &OrderId) -> std::result::Result<(), TgError> {
        self.executor.cancel(order_id).await
    }
}

struct ContextClock {
    now: DateTime<Utc>,
    delegate: Arc<dyn Clock>,
}

impl Clock for ContextClock {
    fn now(&self) -> DateTime<Utc> {
        self.now
    }

    fn trading_date(&self, ts: DateTime<Utc>) -> NaiveDate {
        self.delegate.trading_date(ts)
    }
}

fn event_ts(event: &Event) -> DateTime<Utc> {
    match event {
        Event::Bar(bar) => bar.ts,
        Event::Snapshot(snapshot) => snapshot.ts,
        Event::Timer(at) => *at,
        Event::Fill(fill) => fill.ts,
    }
}

fn event_priority(event: &Event) -> EventPriority {
    match event {
        Event::Fill(_) => EventPriority::Fill,
        Event::Bar(_) => EventPriority::Bar,
        Event::Snapshot(_) => EventPriority::Snapshot,
        Event::Timer(_) => EventPriority::Timer,
    }
}
