pub mod limitup;
pub mod swing;
pub mod t0;

pub use limitup::{LimitUpConfig, LimitUpStrategy};
pub use swing::{SwingConfig, SwingStrategy};
pub use t0::{T0Config, T0Strategy};

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SIGNAL_ID: AtomicU64 = AtomicU64::new(1);

fn next_signal_id(prefix: &str) -> String {
    let id = NEXT_SIGNAL_ID.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{id:020}")
}
