#include "tg_indicators/indicators/adx.h"

#include <algorithm>
#include <numeric>

#include "tg_indicators/indicators/atr.h"

namespace tg_indicators {

SeriesMap AdxIndicator::compute(const std::vector<OHLCV>& bars, const Params& params) const {
  const int period = period_param(params, "period", 14);
  require_bars(bars.size(), static_cast<size_t>(period * 2), "ADX");

  const size_t n = bars.size();
  const size_t p = static_cast<size_t>(period);
  std::vector<double> tr = true_ranges(bars);
  std::vector<double> plus_dm(n, 0.0);
  std::vector<double> minus_dm(n, 0.0);
  for (size_t i = 1; i < n; ++i) {
    const double up_move = bars[i].high - bars[i - 1].high;
    const double down_move = bars[i - 1].low - bars[i].low;
    plus_dm[i] = (up_move > down_move && up_move > 0.0) ? up_move : 0.0;
    minus_dm[i] = (down_move > up_move && down_move > 0.0) ? down_move : 0.0;
  }

  std::vector<double> plus_di(n, nan_value());
  std::vector<double> minus_di(n, nan_value());
  std::vector<double> dx(n, nan_value());

  double smooth_tr = std::accumulate(tr.begin() + 1, tr.begin() + static_cast<long>(p + 1), 0.0);
  double smooth_plus = std::accumulate(plus_dm.begin() + 1, plus_dm.begin() + static_cast<long>(p + 1), 0.0);
  double smooth_minus = std::accumulate(minus_dm.begin() + 1, minus_dm.begin() + static_cast<long>(p + 1), 0.0);

  for (size_t i = p; i < n; ++i) {
    if (i > p) {
      smooth_tr = smooth_tr - (smooth_tr / static_cast<double>(period)) + tr[i];
      smooth_plus = smooth_plus - (smooth_plus / static_cast<double>(period)) + plus_dm[i];
      smooth_minus = smooth_minus - (smooth_minus / static_cast<double>(period)) + minus_dm[i];
    }
    if (smooth_tr != 0.0) {
      plus_di[i] = 100.0 * smooth_plus / smooth_tr;
      minus_di[i] = 100.0 * smooth_minus / smooth_tr;
      const double denominator = plus_di[i] + minus_di[i];
      dx[i] = denominator == 0.0 ? 0.0 : 100.0 * std::abs(plus_di[i] - minus_di[i]) / denominator;
    }
  }

  std::vector<double> adx(n, nan_value());
  double seed = 0.0;
  for (size_t i = p; i < p * 2; ++i) {
    seed += dx[i];
  }
  adx[(p * 2) - 1] = seed / static_cast<double>(period);
  for (size_t i = p * 2; i < n; ++i) {
    adx[i] = ((adx[i - 1] * static_cast<double>(period - 1)) + dx[i]) / static_cast<double>(period);
  }

  return {{"adx", adx}, {"plus_di", plus_di}, {"minus_di", minus_di}};
}

}  // namespace tg_indicators

