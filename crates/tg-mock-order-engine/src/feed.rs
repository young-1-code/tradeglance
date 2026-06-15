use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tg_contracts::{Event, Result, Snapshot};
use tg_engine::DataFeed;
use tg_persistence::SnapshotRepo;

pub struct RealtimeDataFeed {
    repo: Arc<dyn SnapshotRepo>,
    symbols: Vec<String>,
    poll_interval: Duration,
    latest_ts: HashMap<String, DateTime<Utc>>,
    buffered: Option<Event>,
}

impl RealtimeDataFeed {
    pub fn new(repo: Arc<dyn SnapshotRepo>, symbols: Vec<String>, poll_interval: Duration) -> Self {
        Self {
            repo,
            symbols,
            poll_interval,
            latest_ts: HashMap::new(),
            buffered: None,
        }
    }

    async fn fill_buffer(&mut self) -> Result<()> {
        while self.buffered.is_none() {
            for symbol in &self.symbols {
                let Some(snapshot) = self.repo.get_latest(symbol).await? else {
                    continue;
                };
                if self.is_newer(&snapshot) {
                    self.latest_ts.insert(symbol.clone(), snapshot.ts);
                    self.buffered = Some(Event::Snapshot(snapshot));
                    return Ok(());
                }
            }
            tokio::time::sleep(self.poll_interval).await;
        }
        Ok(())
    }

    fn is_newer(&self, snapshot: &Snapshot) -> bool {
        self.latest_ts
            .get(&snapshot.symbol)
            .map_or(true, |seen| snapshot.ts > *seen)
    }
}

#[async_trait]
impl DataFeed for RealtimeDataFeed {
    async fn next_event(&mut self) -> Result<Option<Event>> {
        self.fill_buffer().await?;
        Ok(self.buffered.take())
    }

    async fn peek_next_ts(&mut self) -> Result<Option<DateTime<Utc>>> {
        self.fill_buffer().await?;
        Ok(self.buffered.as_ref().map(|event| match event {
            Event::Bar(bar) => bar.ts,
            Event::Snapshot(snapshot) => snapshot.ts,
            Event::Timer(ts) => *ts,
            Event::Fill(fill) => fill.ts,
        }))
    }
}
