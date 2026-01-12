#pragma once

#include "indicator_base.hpp"

namespace quantization::indicators {

class SMA : public IIndicator {
public:
    explicit SMA(size_t period);
    ~SMA() override = default;

    quantization::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    std::string name() const override { return "SMA"; }
    size_t min_data_points() const override { return period_; }

private:
    size_t period_;
};

} // namespace quantization::indicators
