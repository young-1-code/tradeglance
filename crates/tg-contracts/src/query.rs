use std::ops::Range;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Adjustment, BarPeriod};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarQuery {
    pub symbol: String,
    pub period: BarPeriod,
    pub range: Range<DateTime<Utc>>,
    pub adjustment: Adjustment,
}
