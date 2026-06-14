#pragma once

#include "tg_indicators/indicators/indicator_base.h"

namespace tg_indicators {

// ATR(n): Wilder-smoothed true range using high/low and previous close.
class AtrIndicator final : public IIndicator {
 public:
  SeriesMap compute(const std::vector<OHLCV>& bars, const Params& params) const override;
};

std::vector<double> true_ranges(const std::vector<OHLCV>& bars);

}  // namespace tg_indicators

