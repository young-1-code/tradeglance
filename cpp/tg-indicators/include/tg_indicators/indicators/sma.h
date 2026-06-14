#pragma once

#include "tg_indicators/indicators/indicator_base.h"

namespace tg_indicators {

// SMA(n): arithmetic mean of close over the last n bars. Warm-up slots are NaN.
class SmaIndicator final : public IIndicator {
 public:
  SeriesMap compute(const std::vector<OHLCV>& bars, const Params& params) const override;
};

std::vector<double> compute_sma(const std::vector<double>& values, int period);

}  // namespace tg_indicators

