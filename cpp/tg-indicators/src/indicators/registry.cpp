#include "tg_indicators/indicators/registry.h"

#include <algorithm>
#include <cctype>

#include "tg_indicators/indicators/adx.h"
#include "tg_indicators/indicators/atr.h"
#include "tg_indicators/indicators/bollinger_bands.h"
#include "tg_indicators/indicators/cci.h"
#include "tg_indicators/indicators/ema.h"
#include "tg_indicators/indicators/macd.h"
#include "tg_indicators/indicators/obv.h"
#include "tg_indicators/indicators/rsi.h"
#include "tg_indicators/indicators/sma.h"
#include "tg_indicators/indicators/stochastic.h"
#include "tg_indicators/indicators/williams_r.h"

namespace tg_indicators {

std::string normalize_indicator_name(const std::string& name) {
  std::string normalized;
  normalized.reserve(name.size());
  for (unsigned char c : name) {
    if (c != '-' && c != '_' && c != ' ') {
      normalized.push_back(static_cast<char>(std::toupper(c)));
    }
  }
  return normalized;
}

std::unique_ptr<IIndicator> create_indicator(const std::string& name) {
  const std::string key = normalize_indicator_name(name);
  if (key == "SMA") {
    return std::make_unique<SmaIndicator>();
  }
  if (key == "EMA") {
    return std::make_unique<EmaIndicator>();
  }
  if (key == "MACD") {
    return std::make_unique<MacdIndicator>();
  }
  if (key == "RSI") {
    return std::make_unique<RsiIndicator>();
  }
  if (key == "BOLL" || key == "BOLLINGER" || key == "BOLLINGERBANDS") {
    return std::make_unique<BollingerBandsIndicator>();
  }
  if (key == "ATR") {
    return std::make_unique<AtrIndicator>();
  }
  if (key == "ADX") {
    return std::make_unique<AdxIndicator>();
  }
  if (key == "CCI") {
    return std::make_unique<CciIndicator>();
  }
  if (key == "KDJ" || key == "STOCHASTIC") {
    return std::make_unique<StochasticIndicator>();
  }
  if (key == "WILLR" || key == "WILLIAMSR" || key == "WILLIAMS%R") {
    return std::make_unique<WilliamsRIndicator>();
  }
  if (key == "OBV") {
    return std::make_unique<ObvIndicator>();
  }
  return nullptr;
}

}  // namespace tg_indicators
