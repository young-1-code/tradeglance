#include "tg_indicators/indicators/atr.h"

#include <algorithm>
#include <numeric>

namespace tg_indicators {

std::vector<double> true_ranges(const std::vector<OHLCV>& bars) {
  std::vector<double> tr(bars.size(), 0.0);
  if (bars.empty()) {
    return tr;
  }
  tr[0] = bars[0].high - bars[0].low;
  for (size_t i = 1; i < bars.size(); ++i) {
    const double high_low = bars[i].high - bars[i].low;
    const double high_prev_close = std::abs(bars[i].high - bars[i - 1].close);
    const double low_prev_close = std::abs(bars[i].low - bars[i - 1].close);
    tr[i] = std::max({high_low, high_prev_close, low_prev_close});
  }
  return tr;
}

SeriesMap AtrIndicator::compute(const std::vector<OHLCV>& bars, const Params& params) const {
  const int period = period_param(params, "period", 14);
  require_bars(bars.size(), static_cast<size_t>(period), "ATR");
  const std::vector<double> tr = true_ranges(bars);
  std::vector<double> atr(bars.size(), nan_value());
  const size_t p = static_cast<size_t>(period);
  double seed = std::accumulate(tr.begin(), tr.begin() + static_cast<long>(p), 0.0);
  atr[p - 1] = seed / static_cast<double>(period);
  for (size_t i = p; i < bars.size(); ++i) {
    atr[i] = ((atr[i - 1] * static_cast<double>(period - 1)) + tr[i]) / static_cast<double>(period);
  }
  return {{"atr", atr}};
}

}  // namespace tg_indicators

