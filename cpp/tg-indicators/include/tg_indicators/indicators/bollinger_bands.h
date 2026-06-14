#pragma once

#include "tg_indicators/indicators/indicator_base.h"

namespace tg_indicators {

// Bollinger Bands: mid=SMA(n), population stddev, upper/lower=mid +/- k*stddev.
class BollingerBandsIndicator final : public IIndicator {
 public:
  SeriesMap compute(const std::vector<OHLCV>& bars, const Params& params) const override;
};

}  // namespace tg_indicators

