#include "tg_indicators/indicators/stochastic.h"

#include <algorithm>

namespace tg_indicators {

SeriesMap StochasticIndicator::compute(const std::vector<OHLCV>& bars, const Params& params) const {
  const int k_period = period_param(params, "k_period", 9);
  const int d_period = period_param(params, "d_period", 3);
  const double j_smooth = param_or(params, "j_smooth", 3.0);
  if (!std::isfinite(j_smooth) || j_smooth <= 0.0) {
    throw std::invalid_argument("parameter j_smooth must be positive");
  }
  require_bars(bars.size(), static_cast<size_t>(k_period), "KDJ");

  std::vector<double> k(bars.size(), nan_value());
  std::vector<double> d(bars.size(), nan_value());
  std::vector<double> j(bars.size(), nan_value());
  double prev_k = 50.0;
  double prev_d = 50.0;
  const size_t kp = static_cast<size_t>(k_period);
  const double k_alpha = 1.0 / static_cast<double>(d_period);
  for (size_t i = kp - 1; i < bars.size(); ++i) {
    double highest_high = bars[i + 1 - kp].high;
    double lowest_low = bars[i + 1 - kp].low;
    for (size_t idx = i + 1 - kp; idx <= i; ++idx) {
      highest_high = std::max(highest_high, bars[idx].high);
      lowest_low = std::min(lowest_low, bars[idx].low);
    }
    const double range = highest_high - lowest_low;
    const double rsv = range == 0.0 ? 50.0 : 100.0 * (bars[i].close - lowest_low) / range;
    prev_k = (1.0 - k_alpha) * prev_k + k_alpha * rsv;
    prev_d = (1.0 - k_alpha) * prev_d + k_alpha * prev_k;
    k[i] = prev_k;
    d[i] = prev_d;
    j[i] = j_smooth * k[i] - (j_smooth - 1.0) * d[i];
  }
  return {{"k", k}, {"d", d}, {"j", j}};
}

}  // namespace tg_indicators

