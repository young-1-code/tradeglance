#include "tg_indicators/indicators/sma.h"

namespace tg_indicators {

std::vector<double> compute_sma(const std::vector<double>& values, int period) {
  require_bars(values.size(), static_cast<size_t>(period), "SMA");
  std::vector<double> out(values.size(), nan_value());
  double sum = 0.0;
  for (size_t i = 0; i < values.size(); ++i) {
    sum += values[i];
    if (i >= static_cast<size_t>(period)) {
      sum -= values[i - static_cast<size_t>(period)];
    }
    if (i + 1 >= static_cast<size_t>(period)) {
      out[i] = sum / static_cast<double>(period);
    }
  }
  return out;
}

SeriesMap SmaIndicator::compute(const std::vector<OHLCV>& bars, const Params& params) const {
  const int period = period_param(params, "period", 20);
  return {{"sma", compute_sma(close_values(bars), period)}};
}

}  // namespace tg_indicators

