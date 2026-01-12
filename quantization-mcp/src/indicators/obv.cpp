#include "indicators/obv.hpp"

namespace quantization::indicators {

quantization::expected<IndicatorResult, IndicatorError>
OBV::calculate(const std::vector<OHLCV>& data) {
    if (data.size() < 2) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    IndicatorResult result;
    result.indicator_name = name();
    result.values.reserve(data.size());
    result.timestamps.reserve(data.size());

    // 初始OBV值为0
    double obv = 0.0;
    result.values.push_back(obv);
    result.timestamps.push_back(data[0].timestamp);

    // 计算后续OBV值
    for (size_t i = 1; i < data.size(); ++i) {
        if (data[i].close > data[i - 1].close) {
            obv += data[i].volume;
        } else if (data[i].close < data[i - 1].close) {
            obv -= data[i].volume;
        }
        // 如果收盘价相等，OBV保持不变

        result.values.push_back(obv);
        result.timestamps.push_back(data[i].timestamp);
    }

    return result;
}

} // namespace quantization::indicators
