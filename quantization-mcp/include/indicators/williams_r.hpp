#pragma once

#include "indicator_base.hpp"

namespace quantization::indicators {

class WilliamsR : public IIndicator {
public:
    explicit WilliamsR(size_t period = 14);
    ~WilliamsR() override = default;

    quantization::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    std::string name() const override { return "WilliamsR"; }
    size_t min_data_points() const override { return period_; }

private:
    size_t period_;
};

} // namespace quantization::indicators
