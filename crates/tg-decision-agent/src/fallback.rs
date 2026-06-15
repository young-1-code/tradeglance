use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::llm::LlmClient;

#[derive(Debug, Default)]
pub struct LlmAvailability {
    available: AtomicBool,
}

impl LlmAvailability {
    pub fn new(available: bool) -> Self {
        Self {
            available: AtomicBool::new(available),
        }
    }

    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::SeqCst)
    }

    pub fn mark_available(&self) {
        self.available.store(true, Ordering::SeqCst);
    }

    pub fn mark_unavailable(&self) {
        self.available.store(false, Ordering::SeqCst);
    }
}

pub fn spawn_probe_task<C>(
    client: Arc<C>,
    availability: Arc<LlmAvailability>,
    interval: Duration,
) -> tokio::task::JoinHandle<()>
where
    C: LlmClient + 'static,
{
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            match client.probe().await {
                Ok(()) => availability.mark_available(),
                Err(error) => {
                    tracing::warn!(%error, "LLM probe failed");
                    availability.mark_unavailable();
                }
            }
        }
    })
}
