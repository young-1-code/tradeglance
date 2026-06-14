#pragma once

#include "tg_indicators/indicators/indicator_base.h"

namespace tg_indicators {

// Williams %R(n): -100 * (highest_high - close) / (highest_high - lowest_low).
class WilliamsRIndicator final : public IIndicator {
 public:
  SeriesMap compute(const std::vector<OHLCV>& bars, const Params& params) const override;
};

}  // namespace tg_indicators

