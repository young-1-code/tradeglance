#pragma once

#include "tg_indicators/indicators/indicator_base.h"

namespace tg_indicators {

// OBV: cumulative signed volume; add on higher close, subtract on lower close, start at 0.
class ObvIndicator final : public IIndicator {
 public:
  SeriesMap compute(const std::vector<OHLCV>& bars, const Params& params) const override;
};

}  // namespace tg_indicators

