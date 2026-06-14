#include "tg_indicators/indicators/bollinger_bands.h"

#include <numeric>

#include "tg_indicators/indicators/sma.h"

namespace tg_indicators {

SeriesMap BollingerBandsIndicator::compute(const std::vector<OHLCV>& bars, const Params& params) const {
  const int period = period_param(params, "period", 20);
  const double k = param_or(params, "std_dev", 2.0);
  if (!std::isfinite(k) || k < 0.0) {
    throw std::invalid_argument("parameter std_dev must be non-negative");
  }
  const std::vector<double> close = close_values(bars);
  std::vector<double> mid = compute_sma(close, period);
  std::vector<double> upper(bars.size(), nan_value());
  std::vector<double> lower(bars.size(), nan_value());
  const size_t p = static_cast<size_t>(period);
  for (size_t i = p - 1; i < bars.size(); ++i) {
    double variance = 0.0;
    for (size_t j = i + 1 - p; j <= i; ++j) {
      const double diff = close[j] - mid[i];
      variance += diff * diff;
    }
    const double stddev = std::sqrt(variance / static_cast<double>(period));
    upper[i] = mid[i] + k * stddev;
    lower[i] = mid[i] - k * stddev;
  }
  return {{"upper", upper}, {"mid", mid}, {"lower", lower}};
}

}  // namespace tg_indicators

