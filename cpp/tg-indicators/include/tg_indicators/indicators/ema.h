#pragma once

#include "tg_indicators/indicators/indicator_base.h"

namespace tg_indicators {

// EMA(n): exponential moving average of close, seeded by SMA(n), alpha=smoothing/(n+1).
class EmaIndicator final : public IIndicator {
 public:
  SeriesMap compute(const std::vector<OHLCV>& bars, const Params& params) const override;
};

std::vector<double> compute_ema(const std::vector<double>& values, int period, double smoothing = 2.0);

}  // namespace tg_indicators

