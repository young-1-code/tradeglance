use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use futures::future::join_all;
use sqlx::Row;
use tg_contracts::proto::tg::v1::{SyncJob, SyncStatus};
use tg_contracts::{BarPeriod, InstrumentType, Result, TgError};
#[cfg(test)]
use tg_persistence::WatchlistEntry;
use tg_persistence::{
    BarRepo, CalendarRepo, FactorRepo, InstrumentRepo, PostgresStore, SnapshotRepo,
};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::config::{RetryConfig, WatchlistSymbol};
use crate::rate_limit::AsyncRateLimiter;
use crate::sidecar::SidecarClient;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchState {
    pub symbol: String,
    pub period: BarPeriod,
    pub last_fetched_ts: Option<DateTime<Utc>>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub status: String,
    pub last_error: Option<String>,
}

#[async_trait]
pub trait FetchStateRepo: Send + Sync {
    async fn upsert_fetch_state(&self, state: &FetchState) -> Result<()>;
    async fn list_fetch_states(&self) -> Result<Vec<FetchState>>;
}

#[async_trait]
impl FetchStateRepo for PostgresStore {
    async fn upsert_fetch_state(&self, state: &FetchState) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO fetch_state (
                symbol, period, last_fetched_ts, last_sync_at, status, last_error
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (symbol, period) DO UPDATE SET
                last_fetched_ts = EXCLUDED.last_fetched_ts,
                last_sync_at = EXCLUDED.last_sync_at,
                status = EXCLUDED.status,
                last_error = EXCLUDED.last_error
            "#,
        )
        .bind(&state.symbol)
        .bind(period_to_db(state.period))
        .bind(state.last_fetched_ts)
        .bind(state.last_sync_at)
        .bind(&state.status)
        .bind(&state.last_error)
        .execute(self.pool())
        .await
        .map_err(|err| TgError::Other(err.into()))?;
        Ok(())
    }

    async fn list_fetch_states(&self) -> Result<Vec<FetchState>> {
        let rows = sqlx::query(
            r#"
            SELECT symbol, period, last_fetched_ts, last_sync_at, status, last_error
            FROM fetch_state
            ORDER BY symbol, period
            "#,
        )
        .fetch_all(self.pool())
        .await
        .map_err(|err| TgError::Other(err.into()))?;

        rows.into_iter()
            .map(|row| {
                let period = row.try_get::<String, _>("period").map_err(other)?;
                Ok(FetchState {
                    symbol: row.try_get("symbol").map_err(other)?,
                    period: period_from_db(&period)?,
                    last_fetched_ts: row.try_get("last_fetched_ts").map_err(other)?,
                    last_sync_at: row.try_get("last_sync_at").map_err(other)?,
                    status: row.try_get("status").map_err(other)?,
                    last_error: row.try_get("last_error").map_err(other)?,
                })
            })
            .collect()
    }
}

#[derive(Clone)]
pub struct Repositories {
    pub bars: Arc<dyn BarRepo>,
    pub snapshots: Arc<dyn SnapshotRepo>,
    pub instruments: Arc<dyn InstrumentRepo>,
    pub calendar: Arc<dyn CalendarRepo>,
    pub factors: Arc<dyn FactorRepo>,
    pub fetch_state: Arc<dyn FetchStateRepo>,
}

impl Repositories {
    pub fn from_postgres_store(store: Arc<PostgresStore>) -> Self {
        Self {
            bars: store.clone(),
            snapshots: store.clone(),
            instruments: store.clone(),
            calendar: store.clone(),
            factors: store.clone(),
            fetch_state: store,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub history_start: NaiveDate,
    pub history_end: NaiveDate,
    pub poll_interval: Duration,
    pub retry: RetryConfig,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            history_start: NaiveDate::from_ymd_opt(2021, 1, 1).expect("valid default date"),
            history_end: Utc::now()
                .date_naive()
                .succ_opt()
                .expect("valid default date"),
            poll_interval: Duration::from_secs(5),
            retry: RetryConfig {
                max_attempts: 3,
                base_delay_ms: 200,
                max_delay_ms: 5_000,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct JobState {
    id: String,
    status: String,
    created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct SyncEngine {
    sidecar: Arc<dyn SidecarClient>,
    repos: Repositories,
    limiter: Arc<AsyncRateLimiter>,
    options: SyncOptions,
    jobs: Arc<RwLock<HashMap<String, JobState>>>,
}

impl SyncEngine {
    pub fn new(
        sidecar: Arc<dyn SidecarClient>,
        repos: Repositories,
        limiter: Arc<AsyncRateLimiter>,
        options: SyncOptions,
    ) -> Self {
        Self {
            sidecar,
            repos,
            limiter,
            options,
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn apply_watchlist_config(&self, symbols: &[WatchlistSymbol]) -> Result<()> {
        for entry in symbols {
            self.repos
                .instruments
                .add_to_watchlist(&entry.symbol, &entry.strategy_tags)
                .await?;
        }
        Ok(())
    }

    pub async fn trigger_full_sync(&self, symbols: Vec<String>) -> SyncJob {
        let job = self.create_job("running").await;
        let job_id = job.id.clone();
        let engine = self.clone();
        tokio::spawn(async move {
            let result = engine.full_sync(symbols).await;
            engine.finish_job(&job_id, result).await;
        });
        job
    }

    pub async fn trigger_incremental_sync(&self) -> SyncJob {
        let job = self.create_job("running").await;
        let job_id = job.id.clone();
        let engine = self.clone();
        tokio::spawn(async move {
            let result = engine.incremental_sync().await;
            engine.finish_job(&job_id, result).await;
        });
        job
    }

    pub async fn full_sync(&self, requested_symbols: Vec<String>) -> Result<()> {
        info!("starting full market-data sync");
        self.sync_instruments().await?;
        let symbols = self.resolve_symbols(requested_symbols).await?;
        let start = self
            .options
            .history_start
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| TgError::Validation("invalid history_start".to_owned()))?
            .and_utc();
        let end = self
            .options
            .history_end
            .and_hms_opt(23, 59, 59)
            .ok_or_else(|| TgError::Validation("invalid history_end".to_owned()))?
            .and_utc();

        let calendar = self
            .call_sidecar(|| {
                self.sidecar
                    .get_calendar(self.options.history_start, self.options.history_end)
            })
            .await?;
        self.repos.calendar.upsert_calendar(&calendar).await?;

        for symbol in symbols {
            let factors = self
                .call_sidecar(|| self.sidecar.get_adjust_factors(&symbol))
                .await?;
            self.repos
                .factors
                .upsert_adjustment_factors(&factors)
                .await?;

            for period in [BarPeriod::Daily, BarPeriod::Min1, BarPeriod::Min5] {
                self.mark_running(&symbol, period).await?;
                let result = self
                    .call_sidecar(|| self.sidecar.get_bars(&symbol, period, start, end))
                    .await;
                match result {
                    Ok(bars) => {
                        self.repos.bars.write_bars(&bars).await?;
                        let last = bars.iter().map(|bar| bar.ts).max();
                        self.mark_idle(&symbol, period, last).await?;
                    }
                    Err(err) => {
                        self.mark_failed(&symbol, period, &err.to_string()).await?;
                        return Err(err);
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn incremental_sync(&self) -> Result<()> {
        info!("starting incremental market-data sync");
        self.sync_instruments().await?;
        let symbols = self
            .repos
            .instruments
            .list_watchlist()
            .await?
            .into_iter()
            .map(|entry| entry.symbol)
            .collect::<Vec<_>>();
        let today = Utc::now().date_naive();
        let start = today
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| TgError::Validation("invalid incremental start".to_owned()))?
            .and_utc();
        let end = today
            .and_hms_opt(23, 59, 59)
            .ok_or_else(|| TgError::Validation("invalid incremental end".to_owned()))?
            .and_utc();

        let calendar = self
            .call_sidecar(|| self.sidecar.get_calendar(today, today))
            .await?;
        self.repos.calendar.upsert_calendar(&calendar).await?;

        for symbol in symbols {
            let factors = self
                .call_sidecar(|| self.sidecar.get_adjust_factors(&symbol))
                .await?;
            self.repos
                .factors
                .upsert_adjustment_factors(&factors)
                .await?;

            for period in [BarPeriod::Daily, BarPeriod::Min1, BarPeriod::Min5] {
                self.mark_running(&symbol, period).await?;
                let bars = self
                    .call_sidecar(|| self.sidecar.get_bars(&symbol, period, start, end))
                    .await?;
                self.repos.bars.write_bars(&bars).await?;
                self.mark_idle(&symbol, period, bars.iter().map(|bar| bar.ts).max())
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn realtime_poll_once(&self, symbols: &[String]) -> Result<usize> {
        if symbols.is_empty() {
            return Ok(0);
        }
        let snapshots = self
            .call_sidecar(|| self.sidecar.get_snapshot(symbols))
            .await?;
        let writes = snapshots
            .iter()
            .map(|snapshot| self.repos.snapshots.write_snapshot(snapshot));
        let results = join_all(writes).await;
        for result in results {
            result?;
        }
        Ok(snapshots.len())
    }

    pub async fn realtime_poll(self, cancellation: CancellationToken) -> Result<()> {
        loop {
            if cancellation.is_cancelled() {
                return Ok(());
            }
            let symbols = self
                .repos
                .instruments
                .list_watchlist()
                .await?
                .into_iter()
                .map(|entry| entry.symbol)
                .collect::<Vec<_>>();
            if let Err(err) = self.realtime_poll_once(&symbols).await {
                warn!(error = %err, "realtime poll failed");
            }
            tokio::select! {
                () = cancellation.cancelled() => return Ok(()),
                () = tokio::time::sleep(self.options.poll_interval) => {}
            }
        }
    }

    pub async fn sync_status_report(&self) -> Result<Vec<SyncStatus>> {
        self.repos
            .fetch_state
            .list_fetch_states()
            .await?
            .into_iter()
            .map(sync_status_from_state)
            .collect()
    }

    pub async fn update_watchlist(
        &self,
        add: Vec<String>,
        remove: Vec<String>,
    ) -> Result<Vec<String>> {
        for symbol in add {
            self.repos
                .instruments
                .add_to_watchlist(&symbol, &[])
                .await?;
        }
        for symbol in remove {
            self.repos
                .instruments
                .remove_from_watchlist(&symbol)
                .await?;
        }
        self.watchlist_symbols().await
    }

    pub async fn watchlist_symbols(&self) -> Result<Vec<String>> {
        Ok(self
            .repos
            .instruments
            .list_watchlist()
            .await?
            .into_iter()
            .map(|entry| entry.symbol)
            .collect())
    }

    async fn sync_instruments(&self) -> Result<()> {
        let stocks = self
            .call_sidecar(|| self.sidecar.get_instruments(InstrumentType::Stock))
            .await?;
        let etfs = self
            .call_sidecar(|| self.sidecar.get_instruments(InstrumentType::Etf))
            .await?;
        for instrument in stocks.into_iter().chain(etfs) {
            self.repos
                .instruments
                .upsert_instrument(&instrument)
                .await?;
        }
        Ok(())
    }

    async fn resolve_symbols(&self, requested_symbols: Vec<String>) -> Result<Vec<String>> {
        if !requested_symbols.is_empty() {
            return Ok(requested_symbols);
        }
        let watchlist = self.repos.instruments.list_watchlist().await?;
        Ok(watchlist.into_iter().map(|entry| entry.symbol).collect())
    }

    async fn call_sidecar<T, Fut, F>(&self, mut call: F) -> Result<T>
    where
        Fut: Future<Output = Result<T>>,
        F: FnMut() -> Fut,
    {
        let attempts = self.options.retry.max_attempts.max(1);
        let mut last_error = None;
        for attempt in 1..=attempts {
            self.limiter.acquire().await;
            match call().await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    last_error = Some(err);
                    if attempt < attempts {
                        tokio::time::sleep(backoff_delay(self.options.retry, attempt)).await;
                    }
                }
            }
        }
        Err(last_error.expect("retry loop has at least one attempt"))
    }

    async fn mark_running(&self, symbol: &str, period: BarPeriod) -> Result<()> {
        self.repos
            .fetch_state
            .upsert_fetch_state(&FetchState {
                symbol: symbol.to_owned(),
                period,
                last_fetched_ts: None,
                last_sync_at: Some(Utc::now()),
                status: "running".to_owned(),
                last_error: None,
            })
            .await
    }

    async fn mark_idle(
        &self,
        symbol: &str,
        period: BarPeriod,
        last_fetched_ts: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.repos
            .fetch_state
            .upsert_fetch_state(&FetchState {
                symbol: symbol.to_owned(),
                period,
                last_fetched_ts,
                last_sync_at: Some(Utc::now()),
                status: "idle".to_owned(),
                last_error: None,
            })
            .await
    }

    async fn mark_failed(&self, symbol: &str, period: BarPeriod, error: &str) -> Result<()> {
        self.repos
            .fetch_state
            .upsert_fetch_state(&FetchState {
                symbol: symbol.to_owned(),
                period,
                last_fetched_ts: None,
                last_sync_at: Some(Utc::now()),
                status: "failed".to_owned(),
                last_error: Some(error.to_owned()),
            })
            .await
    }

    async fn create_job(&self, status: &str) -> SyncJob {
        let now = Utc::now();
        let id = format!("sync-{}", now.timestamp_millis());
        let job = JobState {
            id: id.clone(),
            status: status.to_owned(),
            created_at: now,
        };
        self.jobs.write().await.insert(id.clone(), job.clone());
        sync_job_from_state(job)
    }

    async fn finish_job(&self, job_id: &str, result: Result<()>) {
        let status = match result {
            Ok(()) => "completed",
            Err(err) => {
                error!(job_id, error = %err, "sync job failed");
                "failed"
            }
        };
        if let Some(job) = self.jobs.write().await.get_mut(job_id) {
            job.status = status.to_owned();
        }
    }
}

pub fn period_to_db(period: BarPeriod) -> &'static str {
    match period {
        BarPeriod::Daily => "daily",
        BarPeriod::Min1 => "min1",
        BarPeriod::Min5 => "min5",
    }
}

pub fn period_from_db(value: &str) -> Result<BarPeriod> {
    match value {
        "daily" => Ok(BarPeriod::Daily),
        "min1" | "minute1" => Ok(BarPeriod::Min1),
        "min5" | "minute5" => Ok(BarPeriod::Min5),
        _ => Err(TgError::Validation(format!(
            "invalid fetch_state period {value}"
        ))),
    }
}

fn sync_job_from_state(job: JobState) -> SyncJob {
    SyncJob {
        id: job.id,
        status: job.status,
        created_at_epoch_millis: job.created_at.timestamp_millis(),
    }
}

fn sync_status_from_state(state: FetchState) -> Result<SyncStatus> {
    Ok(SyncStatus {
        symbol: state.symbol,
        period: proto_period(state.period),
        status: state.status,
        last_fetched_ts_epoch_millis: state.last_fetched_ts.map_or(0, |ts| ts.timestamp_millis()),
        last_sync_at_epoch_millis: state.last_sync_at.map_or(0, |ts| ts.timestamp_millis()),
        last_error: state.last_error.unwrap_or_default(),
    })
}

fn proto_period(period: BarPeriod) -> i32 {
    use tg_contracts::proto::tg::v1::BarPeriod as ProtoBarPeriod;
    match period {
        BarPeriod::Daily => ProtoBarPeriod::Daily as i32,
        BarPeriod::Min1 => ProtoBarPeriod::Min1 as i32,
        BarPeriod::Min5 => ProtoBarPeriod::Min5 as i32,
    }
}

fn backoff_delay(config: RetryConfig, attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(16);
    let exp = 1_u64 << shift;
    let base = config.base_delay_ms.saturating_mul(exp);
    let jitter = u64::from(attempt).saturating_mul(17);
    Duration::from_millis(base.saturating_add(jitter).min(config.max_delay_ms))
}

fn other<E>(err: E) -> TgError
where
    E: std::error::Error + Send + Sync + 'static,
{
    TgError::Other(err.into())
}

#[cfg(test)]
mod tests {
    use std::ops::Range;
    use std::sync::Mutex;

    use chrono::{DateTime, Utc};
    use tg_contracts::{
        Adjustment, AdjustmentFactor, Bar, BarQuery, Instrument, Snapshot, TradingCalendar,
    };

    use super::*;
    use crate::sidecar::MockSidecarClient;

    #[derive(Default)]
    struct InMemoryRepos {
        bars: Mutex<Vec<Bar>>,
        instruments: Mutex<Vec<Instrument>>,
        calendar: Mutex<Vec<TradingCalendar>>,
        factors: Mutex<Vec<AdjustmentFactor>>,
        snapshots: Mutex<Vec<Snapshot>>,
        watchlist: Mutex<Vec<WatchlistEntry>>,
        fetch_state: Mutex<Vec<FetchState>>,
    }

    #[async_trait]
    impl BarRepo for InMemoryRepos {
        async fn write_bars(&self, bars: &[Bar]) -> Result<()> {
            self.bars.lock().unwrap().extend_from_slice(bars);
            Ok(())
        }

        async fn query_bars(&self, _q: BarQuery) -> Result<Vec<Bar>> {
            Ok(self.bars.lock().unwrap().clone())
        }
    }

    #[async_trait]
    impl SnapshotRepo for InMemoryRepos {
        async fn write_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
            self.snapshots.lock().unwrap().push(snapshot.clone());
            Ok(())
        }

        async fn get_latest(&self, symbol: &str) -> Result<Option<Snapshot>> {
            Ok(self
                .snapshots
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|snapshot| snapshot.symbol == symbol)
                .cloned())
        }
    }

    #[async_trait]
    impl InstrumentRepo for InMemoryRepos {
        async fn upsert_instrument(&self, instrument: &Instrument) -> Result<()> {
            let mut instruments = self.instruments.lock().unwrap();
            if let Some(existing) = instruments
                .iter_mut()
                .find(|existing| existing.symbol == instrument.symbol)
            {
                *existing = instrument.clone();
            } else {
                instruments.push(instrument.clone());
            }
            Ok(())
        }

        async fn list_instruments(&self) -> Result<Vec<Instrument>> {
            Ok(self.instruments.lock().unwrap().clone())
        }

        async fn get_instrument(&self, symbol: &str) -> Result<Option<Instrument>> {
            Ok(self
                .instruments
                .lock()
                .unwrap()
                .iter()
                .find(|instrument| instrument.symbol == symbol)
                .cloned())
        }

        async fn add_to_watchlist(&self, symbol: &str, strategy_tags: &[String]) -> Result<()> {
            let mut watchlist = self.watchlist.lock().unwrap();
            if !watchlist.iter().any(|entry| entry.symbol == symbol) {
                watchlist.push(WatchlistEntry {
                    symbol: symbol.to_owned(),
                    strategy_tags: strategy_tags.to_vec(),
                    added_at: Utc::now(),
                });
            }
            Ok(())
        }

        async fn remove_from_watchlist(&self, symbol: &str) -> Result<()> {
            self.watchlist
                .lock()
                .unwrap()
                .retain(|entry| entry.symbol != symbol);
            Ok(())
        }

        async fn list_watchlist(&self) -> Result<Vec<WatchlistEntry>> {
            Ok(self.watchlist.lock().unwrap().clone())
        }
    }

    #[async_trait]
    impl CalendarRepo for InMemoryRepos {
        async fn upsert_calendar(&self, days: &[TradingCalendar]) -> Result<()> {
            self.calendar.lock().unwrap().extend_from_slice(days);
            Ok(())
        }

        async fn is_trading_day(&self, date: NaiveDate) -> Result<bool> {
            Ok(self
                .calendar
                .lock()
                .unwrap()
                .iter()
                .any(|day| day.date == date && day.is_trading_day))
        }

        async fn calendar_range(
            &self,
            start: NaiveDate,
            end: NaiveDate,
        ) -> Result<Vec<TradingCalendar>> {
            Ok(self
                .calendar
                .lock()
                .unwrap()
                .iter()
                .filter(|day| day.date >= start && day.date <= end)
                .cloned()
                .collect())
        }
    }

    #[async_trait]
    impl FactorRepo for InMemoryRepos {
        async fn upsert_adjustment_factors(&self, factors: &[AdjustmentFactor]) -> Result<()> {
            self.factors.lock().unwrap().extend_from_slice(factors);
            Ok(())
        }

        async fn query_adjustment_factors(
            &self,
            _symbol: &str,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<AdjustmentFactor>> {
            Ok(self.factors.lock().unwrap().clone())
        }
    }

    #[async_trait]
    impl FetchStateRepo for InMemoryRepos {
        async fn upsert_fetch_state(&self, state: &FetchState) -> Result<()> {
            let mut states = self.fetch_state.lock().unwrap();
            if let Some(existing) = states
                .iter_mut()
                .find(|existing| existing.symbol == state.symbol && existing.period == state.period)
            {
                *existing = state.clone();
            } else {
                states.push(state.clone());
            }
            Ok(())
        }

        async fn list_fetch_states(&self) -> Result<Vec<FetchState>> {
            Ok(self.fetch_state.lock().unwrap().clone())
        }
    }

    fn repos(memory: Arc<InMemoryRepos>) -> Repositories {
        Repositories {
            bars: memory.clone(),
            snapshots: memory.clone(),
            instruments: memory.clone(),
            calendar: memory.clone(),
            factors: memory.clone(),
            fetch_state: memory,
        }
    }

    #[tokio::test]
    async fn full_sync_with_mock_sidecar_writes_expected_data() {
        let memory = Arc::new(InMemoryRepos::default());
        memory
            .add_to_watchlist("600519", &[String::from("swing")])
            .await
            .unwrap();
        let engine = SyncEngine::new(
            Arc::new(MockSidecarClient::new()),
            repos(memory.clone()),
            Arc::new(AsyncRateLimiter::new(1_000.0, 100)),
            SyncOptions {
                history_start: NaiveDate::from_ymd_opt(2026, 6, 14).unwrap(),
                history_end: NaiveDate::from_ymd_opt(2026, 6, 16).unwrap(),
                ..SyncOptions::default()
            },
        );

        engine.full_sync(vec!["600519".to_owned()]).await.unwrap();

        assert_eq!(memory.instruments.lock().unwrap().len(), 2);
        assert_eq!(memory.bars.lock().unwrap().len(), 3);
        assert_eq!(memory.factors.lock().unwrap().len(), 1);
        assert_eq!(memory.fetch_state.lock().unwrap().len(), 3);
    }

    #[allow(dead_code)]
    fn _assert_query_shape(_range: Range<DateTime<Utc>>, _adjustment: Adjustment) {}
}
