#pragma once

#include <cstdint>
#include <stdexcept>
#include <string>
#include <vector>

#include "tg/v1/contracts.pb.h"

namespace tg_indicators {

struct OHLCV {
  int64_t ts_millis{};
  double open{};
  double high{};
  double low{};
  double close{};
  int64_t volume{};
  double amount{};
};

inline double parse_decimal_string(const std::string& value, const char* field) {
  try {
    size_t parsed = 0;
    const double result = std::stod(value, &parsed);
    if (parsed != value.size()) {
      throw std::invalid_argument("trailing characters");
    }
    return result;
  } catch (const std::exception& e) {
    throw std::invalid_argument(std::string("invalid decimal field ") + field +
                                ": " + e.what());
  }
}

inline std::vector<OHLCV> decode_bars(const google::protobuf::RepeatedPtrField<tg::v1::Bar>& bars) {
  std::vector<OHLCV> decoded;
  decoded.reserve(static_cast<size_t>(bars.size()));
  for (const auto& bar : bars) {
    decoded.push_back(OHLCV{
        bar.ts_epoch_millis(),
        parse_decimal_string(bar.open(), "open"),
        parse_decimal_string(bar.high(), "high"),
        parse_decimal_string(bar.low(), "low"),
        parse_decimal_string(bar.close(), "close"),
        bar.volume(),
        parse_decimal_string(bar.amount(), "amount"),
    });
  }
  return decoded;
}

}  // namespace tg_indicators

