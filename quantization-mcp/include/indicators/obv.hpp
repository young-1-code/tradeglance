#pragma once

#include "indicator_base.hpp"

namespace quantization::indicators {

class OBV : public IIndicator {
public:
    OBV() = default;
    ~OBV() override = default;

    quantization::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    std::string name() const override { return "OBV"; }
    size_t min_data_points() const override { return 2; }
};

} // namespace quantization::indicators
