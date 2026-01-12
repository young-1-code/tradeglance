#pragma once

#include "indicator_base.hpp"

namespace quantization::indicators {

class EMA : public IIndicator {
public:
    explicit EMA(size_t period);
    ~EMA() override = default;

    quantization::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    std::string name() const override { return "EMA"; }
    size_t min_data_points() const override { return period_; }

private:
    size_t period_;
    double multiplier_;
};

} // namespace quantization::indicators
