#include "tg_indicators/indicators/williams_r.h"

#include <algorithm>

namespace tg_indicators {

SeriesMap WilliamsRIndicator::compute(const std::vector<OHLCV>& bars, const Params& params) const {
  const int period = period_param(params, "period", 14);
  require_bars(bars.size(), static_cast<size_t>(period), "WILLR");

  std::vector<double> out(bars.size(), nan_value());
  const size_t p = static_cast<size_t>(period);
  for (size_t i = p - 1; i < bars.size(); ++i) {
    double highest_high = bars[i + 1 - p].high;
    double lowest_low = bars[i + 1 - p].low;
    for (size_t idx = i + 1 - p; idx <= i; ++idx) {
      highest_high = std::max(highest_high, bars[idx].high);
      lowest_low = std::min(lowest_low, bars[idx].low);
    }
    const double range = highest_high - lowest_low;
    out[i] = range == 0.0 ? 0.0 : -100.0 * (highest_high - bars[i].close) / range;
  }
  return {{"willr", out}};
}

}  // namespace tg_indicators

