#pragma once

#include "tg_indicators/indicators/indicator_base.h"

namespace tg_indicators {

// ADX(n): Wilder +DI/-DI, DX, then Wilder-smoothed ADX trend strength.
class AdxIndicator final : public IIndicator {
 public:
  SeriesMap compute(const std::vector<OHLCV>& bars, const Params& params) const override;
};

}  // namespace tg_indicators

