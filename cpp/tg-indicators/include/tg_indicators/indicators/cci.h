#pragma once

#include "tg_indicators/indicators/indicator_base.h"

namespace tg_indicators {

// CCI(n): (typical_price - SMA(tp)) / (constant * mean_absolute_deviation).
class CciIndicator final : public IIndicator {
 public:
  SeriesMap compute(const std::vector<OHLCV>& bars, const Params& params) const override;
};

}  // namespace tg_indicators

