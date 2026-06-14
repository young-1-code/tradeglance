#include "tg_indicators/indicators/macd.h"

#include "tg_indicators/indicators/ema.h"

namespace tg_indicators {

SeriesMap MacdIndicator::compute(const std::vector<OHLCV>& bars, const Params& params) const {
  const int fast = period_param(params, "fast", 12);
  const int slow = period_param(params, "slow", 26);
  const int signal = period_param(params, "signal", 9);
  if (fast >= slow) {
    throw std::invalid_argument("MACD requires fast < slow");
  }
  require_bars(bars.size(), static_cast<size_t>(slow + signal - 1), "MACD");

  const std::vector<double> close = close_values(bars);
  const std::vector<double> fast_ema = compute_ema(close, fast);
  const std::vector<double> slow_ema = compute_ema(close, slow);
  std::vector<double> dif(bars.size(), nan_value());
  for (size_t i = 0; i < bars.size(); ++i) {
    if (!std::isnan(fast_ema[i]) && !std::isnan(slow_ema[i])) {
      dif[i] = fast_ema[i] - slow_ema[i];
    }
  }

  std::vector<double> dea(bars.size(), nan_value());
  const size_t start = static_cast<size_t>(slow - 1);
  double seed_sum = 0.0;
  for (size_t i = start; i < start + static_cast<size_t>(signal); ++i) {
    seed_sum += dif[i];
  }
  const size_t seed_idx = start + static_cast<size_t>(signal) - 1;
  dea[seed_idx] = seed_sum / static_cast<double>(signal);
  const double alpha = 2.0 / (static_cast<double>(signal) + 1.0);
  for (size_t i = seed_idx + 1; i < bars.size(); ++i) {
    dea[i] = alpha * dif[i] + (1.0 - alpha) * dea[i - 1];
  }

  std::vector<double> hist(bars.size(), nan_value());
  for (size_t i = 0; i < bars.size(); ++i) {
    if (!std::isnan(dif[i]) && !std::isnan(dea[i])) {
      hist[i] = 2.0 * (dif[i] - dea[i]);
    }
  }
  return {{"dif", dif}, {"dea", dea}, {"hist", hist}};
}

}  // namespace tg_indicators

