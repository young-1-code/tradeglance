#pragma once

#include <memory>
#include <string>

#include "tg_indicators/indicators/indicator_base.h"

namespace tg_indicators {

std::string normalize_indicator_name(const std::string& name);
std::unique_ptr<IIndicator> create_indicator(const std::string& name);

}  // namespace tg_indicators

