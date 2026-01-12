#pragma once

#include "indicator_base.hpp"

namespace quantization::indicators {

class CCI : public IIndicator {
public:
    explicit CCI(size_t period = 20);
    ~CCI() override = default;

    quantization::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    std::string name() const override { return "CCI"; }
    size_t min_data_points() const override { return period_; }

private:
    size_t period_;
    static constexpr double constant_ = 0.015;
};

} // namespace quantization::indicators
