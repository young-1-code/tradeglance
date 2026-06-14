#pragma once

#include <cmath>
#include <limits>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <vector>

#include "tg_indicators/bar_codec.h"

namespace tg_indicators {

using Params = std::unordered_map<std::string, double>;
using SeriesMap = std::unordered_map<std::string, std::vector<double>>;

inline double nan_value() {
  return std::numeric_limits<double>::quiet_NaN();
}

inline double param_or(const Params& params, const std::string& key, double fallback) {
  const auto it = params.find(key);
  return it == params.end() ? fallback : it->second;
}

inline int period_param(const Params& params, const std::string& key, int fallback) {
  const double raw = param_or(params, key, static_cast<double>(fallback));
  const int period = static_cast<int>(raw);
  if (!std::isfinite(raw) || raw < 1.0 || static_cast<double>(period) != raw) {
    throw std::invalid_argument("parameter " + key + " must be a positive integer");
  }
  return period;
}

inline void require_bars(size_t actual, size_t required, const std::string& indicator) {
  if (actual < required) {
    throw std::invalid_argument(indicator + " requires at least " + std::to_string(required) +
                                " bars, got " + std::to_string(actual));
  }
}

inline std::vector<double> close_values(const std::vector<OHLCV>& bars) {
  std::vector<double> values;
  values.reserve(bars.size());
  for (const auto& bar : bars) {
    values.push_back(bar.close);
  }
  return values;
}

class IIndicator {
 public:
  virtual ~IIndicator() = default;
  virtual SeriesMap compute(const std::vector<OHLCV>& bars, const Params& params) const = 0;
};

}  // namespace tg_indicators

