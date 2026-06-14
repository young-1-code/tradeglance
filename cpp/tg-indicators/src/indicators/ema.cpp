#include "tg_indicators/indicators/ema.h"

#include <numeric>

namespace tg_indicators {

std::vector<double> compute_ema(const std::vector<double>& values, int period, double smoothing) {
  require_bars(values.size(), static_cast<size_t>(period), "EMA");
  if (!std::isfinite(smoothing) || smoothing <= 0.0) {
    throw std::invalid_argument("parameter smoothing must be positive");
  }

  std::vector<double> out(values.size(), nan_value());
  const size_t p = static_cast<size_t>(period);
  const double seed = std::accumulate(values.begin(), values.begin() + static_cast<long>(p), 0.0) /
                      static_cast<double>(period);
  out[p - 1] = seed;
  const double alpha = smoothing / (static_cast<double>(period) + 1.0);
  for (size_t i = p; i < values.size(); ++i) {
    out[i] = alpha * values[i] + (1.0 - alpha) * out[i - 1];
  }
  return out;
}

SeriesMap EmaIndicator::compute(const std::vector<OHLCV>& bars, const Params& params) const {
  const int period = period_param(params, "period", 12);
  const double smoothing = param_or(params, "smoothing", 2.0);
  return {{"ema", compute_ema(close_values(bars), period, smoothing)}};
}

}  // namespace tg_indicators

