#include "tg_indicators/indicators/rsi.h"

namespace tg_indicators {

SeriesMap RsiIndicator::compute(const std::vector<OHLCV>& bars, const Params& params) const {
  const int period = period_param(params, "period", 14);
  require_bars(bars.size(), static_cast<size_t>(period + 1), "RSI");

  std::vector<double> rsi(bars.size(), nan_value());
  double avg_gain = 0.0;
  double avg_loss = 0.0;
  for (size_t i = 1; i <= static_cast<size_t>(period); ++i) {
    const double change = bars[i].close - bars[i - 1].close;
    if (change >= 0.0) {
      avg_gain += change;
    } else {
      avg_loss -= change;
    }
  }
  avg_gain /= static_cast<double>(period);
  avg_loss /= static_cast<double>(period);

  auto to_rsi = [](double gain, double loss) {
    if (loss == 0.0) {
      return 100.0;
    }
    const double rs = gain / loss;
    return 100.0 - (100.0 / (1.0 + rs));
  };

  rsi[static_cast<size_t>(period)] = to_rsi(avg_gain, avg_loss);
  for (size_t i = static_cast<size_t>(period) + 1; i < bars.size(); ++i) {
    const double change = bars[i].close - bars[i - 1].close;
    const double gain = change > 0.0 ? change : 0.0;
    const double loss = change < 0.0 ? -change : 0.0;
    avg_gain = ((avg_gain * static_cast<double>(period - 1)) + gain) / static_cast<double>(period);
    avg_loss = ((avg_loss * static_cast<double>(period - 1)) + loss) / static_cast<double>(period);
    rsi[i] = to_rsi(avg_gain, avg_loss);
  }
  return {{"rsi", rsi}};
}

}  // namespace tg_indicators

