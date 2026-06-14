#pragma once

#include "tg_indicators/indicators/indicator_base.h"

namespace tg_indicators {

// MACD: dif=EMA(fast)-EMA(slow), dea=EMA(signal of dif), hist=2*(dif-dea).
class MacdIndicator final : public IIndicator {
 public:
  SeriesMap compute(const std::vector<OHLCV>& bars, const Params& params) const override;
};

}  // namespace tg_indicators

