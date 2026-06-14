#pragma once

#include "tg_indicators/indicators/indicator_base.h"

namespace tg_indicators {

// KDJ: RSV over k_period, K=2/3 prevK+1/3 RSV, D=2/3 prevD+1/3 K, J=3K-2D.
class StochasticIndicator final : public IIndicator {
 public:
  SeriesMap compute(const std::vector<OHLCV>& bars, const Params& params) const override;
};

}  // namespace tg_indicators

