#include "tg_indicators/indicators/cci.h"

#include <numeric>

namespace tg_indicators {

SeriesMap CciIndicator::compute(const std::vector<OHLCV>& bars, const Params& params) const {
  const int period = period_param(params, "period", 20);
  const double constant = param_or(params, "constant", 0.015);
  if (!std::isfinite(constant) || constant <= 0.0) {
    throw std::invalid_argument("parameter constant must be positive");
  }
  require_bars(bars.size(), static_cast<size_t>(period), "CCI");

  std::vector<double> tp;
  tp.reserve(bars.size());
  for (const auto& bar : bars) {
    tp.push_back((bar.high + bar.low + bar.close) / 3.0);
  }

  std::vector<double> cci(bars.size(), nan_value());
  const size_t p = static_cast<size_t>(period);
  for (size_t i = p - 1; i < bars.size(); ++i) {
    const auto first = tp.begin() + static_cast<long>(i + 1 - p);
    const auto last = tp.begin() + static_cast<long>(i + 1);
    const double mean = std::accumulate(first, last, 0.0) / static_cast<double>(period);
    double mad = 0.0;
    for (auto it = first; it != last; ++it) {
      mad += std::abs(*it - mean);
    }
    mad /= static_cast<double>(period);
    cci[i] = mad == 0.0 ? 0.0 : (tp[i] - mean) / (constant * mad);
  }
  return {{"cci", cci}};
}

}  // namespace tg_indicators

