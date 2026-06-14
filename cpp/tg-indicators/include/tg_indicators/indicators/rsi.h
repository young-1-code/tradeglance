#pragma once

#include "tg_indicators/indicators/indicator_base.h"

namespace tg_indicators {

// RSI(n): Wilder-smoothed relative strength index over close changes.
class RsiIndicator final : public IIndicator {
 public:
  SeriesMap compute(const std::vector<OHLCV>& bars, const Params& params) const override;
};

}  // namespace tg_indicators

