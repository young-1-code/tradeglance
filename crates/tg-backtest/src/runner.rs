use std::ops::Range;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tg_contracts::{Adjustment, BarPeriod, Event, Result, TgError};
use tg_engine::{Clock, DataFeed, Engine, ExecutionHandler, Portfolio, RunSummary, Strategy};
use tg_persistence::{BacktestRunRecord, BacktestRunRepo, BarRepo};

use crate::matcher::{BacktestLedger, HistoricalMatcher, MatcherConfig};
use crate::perf::{closed_trades_from_fills, compute_metrics, BacktestMetrics, EquityPoint};
use crate::replay::BacktestReplay;

#[derive(Debug)]
pub struct HistoricalClock {
    now: RwLock<DateTime<Utc>>,
}

impl HistoricalClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: RwLock::new(now),
        }
    }

    pub fn set_now(&self, now: DateTime<Utc>) -> Result<()> {
        *self
            .now
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("historical clock lock poisoned")))? = now;
        Ok(())
    }
}

impl Clock for HistoricalClock {
    fn now(&self) -> DateTime<Utc> {
        *self
            .now
            .read()
            .expect("historical clock lock should not be poisoned")
    }

    fn trading_date(&self, ts: DateTime<Utc>) -> NaiveDate {
        (ts + chrono::Duration::hours(8)).date_naive()
    }
}

pub struct InstrumentedFeed {
    inner: Box<dyn DataFeed>,
    matcher: Arc<HistoricalMatcher>,
    ledger: Arc<BacktestLedger>,
    clock: Arc<HistoricalClock>,
}

impl InstrumentedFeed {
    pub fn new(
        inner: Box<dyn DataFeed>,
        matcher: Arc<HistoricalMatcher>,
        ledger: Arc<BacktestLedger>,
        clock: Arc<HistoricalClock>,
    ) -> Self {
        Self {
            inner,
            matcher,
            ledger,
            clock,
        }
    }
}

#[async_trait]
impl DataFeed for InstrumentedFeed {
    async fn next_event(&mut self) -> Result<Option<Event>> {
        let event = self.inner.next_event().await?;
        if let Some(Event::Bar(bar)) = &event {
            self.clock.set_now(bar.ts)?;
            self.matcher.set_current_bar(bar.clone())?;
            self.ledger.mark_to_market(bar)?;
        }
        Ok(event)
    }

    async fn peek_next_ts(&mut self) -> Result<Option<DateTime<Utc>>> {
        self.inner.peek_next_ts().await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub id: String,
    pub strategy: String,
    pub symbols: Vec<String>,
    pub period: BarPeriod,
    pub range: Range<DateTime<Utc>>,
    pub adjustment: Adjustment,
    pub matcher: MatcherConfig,
    pub config_json: Value,
}

#[derive(Debug, Clone)]
pub struct BacktestRunOutput {
    pub id: String,
    pub summary: RunSummary,
    pub metrics: BacktestMetrics,
}

pub struct BacktestRunner {
    bar_repo: Arc<dyn BarRepo>,
    run_repo: Option<Arc<dyn BacktestRunRepo>>,
}

impl BacktestRunner {
    pub fn new(bar_repo: Arc<dyn BarRepo>) -> Self {
        Self {
            bar_repo,
            run_repo: None,
        }
    }

    pub fn with_run_repo(mut self, run_repo: Arc<dyn BacktestRunRepo>) -> Self {
        self.run_repo = Some(run_repo);
        self
    }

    pub async fn run(
        &self,
        config: BacktestConfig,
        strategies: Vec<Box<dyn Strategy>>,
    ) -> Result<BacktestRunOutput> {
        self.save_status(&config, "running", None).await?;
        let result = self.execute_run(config.clone(), strategies).await;
        match &result {
            Ok(output) => {
                self.save_status(&config, "done", Some(metrics_json(&output.metrics)?))
                    .await?;
            }
            Err(_) => {
                self.save_status(&config, "failed", None).await?;
            }
        }
        result
    }

    async fn execute_run(
        &self,
        config: BacktestConfig,
        strategies: Vec<Box<dyn Strategy>>,
    ) -> Result<BacktestRunOutput> {
        let replay = BacktestReplay::from_repo(
            self.bar_repo.as_ref(),
            &config.symbols,
            config.period,
            config.range.clone(),
            config.adjustment,
        )
        .await?;
        let initial_now = config.range.start;
        let clock = Arc::new(HistoricalClock::new(initial_now));
        let ledger = Arc::new(BacktestLedger::with_cash(config.matcher.initial_cash));
        let matcher = Arc::new(HistoricalMatcher::with_ledger(
            Arc::clone(&ledger),
            config.matcher.clone(),
        ));
        let feed = InstrumentedFeed::new(
            Box::new(replay),
            Arc::clone(&matcher),
            Arc::clone(&ledger),
            Arc::clone(&clock),
        );
        let executor: Arc<dyn ExecutionHandler> = matcher;
        let portfolio: Arc<dyn Portfolio> = ledger.clone();

        let mut engine = Engine::builder()
            .with_feed(feed)
            .with_executor(executor)
            .with_clock(clock)
            .with_strategies(strategies)
            .with_portfolio(portfolio)
            .build()?;
        let summary = engine.run().await?;

        let fills = ledger.fills();
        let trades = closed_trades_from_fills(&fills);
        let equity_curve = ledger
            .equity_curve()
            .into_iter()
            .map(|(date, total_value)| EquityPoint { date, total_value })
            .collect::<Vec<_>>();
        let metrics = compute_metrics(&equity_curve, &trades, &fills, None);

        Ok(BacktestRunOutput {
            id: config.id,
            summary,
            metrics,
        })
    }

    async fn save_status(
        &self,
        config: &BacktestConfig,
        status: &str,
        metrics: Option<Value>,
    ) -> Result<()> {
        let Some(repo) = &self.run_repo else {
            return Ok(());
        };
        repo.save_run(BacktestRunRecord {
            id: config.id.clone(),
            strategy: config.strategy.clone(),
            symbols: config.symbols.clone(),
            config: config.config_json.clone(),
            status: status.to_owned(),
            metrics,
            created_at: Utc::now(),
            finished_at: (status == "done").then(Utc::now),
        })
        .await
    }
}

fn metrics_json(metrics: &BacktestMetrics) -> Result<Value> {
    serde_json::to_value(metrics).map_err(|error| TgError::Other(error.into()))
}
