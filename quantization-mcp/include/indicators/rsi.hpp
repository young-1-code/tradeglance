#pragma once

#include "indicator_base.hpp"

namespace quantization::indicators {

class RSI : public IIndicator {
public:
    explicit RSI(size_t period = 14);
    ~RSI() override = default;

    quantization::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    std::string name() const override { return "RSI"; }
    size_t min_data_points() const override { return period_ + 1; }

private:
    size_t period_;
};

} // namespace quantization::indicators
