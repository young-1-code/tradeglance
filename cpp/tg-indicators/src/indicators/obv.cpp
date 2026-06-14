#include "tg_indicators/indicators/obv.h"

namespace tg_indicators {

SeriesMap ObvIndicator::compute(const std::vector<OHLCV>& bars, const Params&) const {
  require_bars(bars.size(), 1, "OBV");
  std::vector<double> obv(bars.size(), 0.0);
  for (size_t i = 1; i < bars.size(); ++i) {
    obv[i] = obv[i - 1];
    if (bars[i].close > bars[i - 1].close) {
      obv[i] += static_cast<double>(bars[i].volume);
    } else if (bars[i].close < bars[i - 1].close) {
      obv[i] -= static_cast<double>(bars[i].volume);
    }
  }
  return {{"obv", obv}};
}

}  // namespace tg_indicators

