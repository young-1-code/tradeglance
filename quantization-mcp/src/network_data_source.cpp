#include "network_data_source.hpp"
#include <curl/curl.h>
#include <nlohmann/json.hpp>
#include <sstream>
#include <iomanip>

using json = nlohmann::json;

namespace quantization {

// CURL写入回调函数
static size_t write_callback(void* contents, size_t size, size_t nmemb, std::string* userp) {
    userp->append(static_cast<char*>(contents), size * nmemb);
    return size * nmemb;
}

NetworkDataSource::NetworkDataSource(const std::string& api_endpoint)
    : api_endpoint_(api_endpoint) {
    curl_global_init(CURL_GLOBAL_DEFAULT);
}

quantization::expected<std::string, DataSourceError>
NetworkDataSource::http_get(const std::string& url) {
    CURL* curl = curl_easy_init();
    if (!curl) {
        return quantization::unexpected(DataSourceError::NetworkError);
    }

    std::string response_data;
    CURLcode res;

    curl_easy_setopt(curl, CURLOPT_URL, url.c_str());
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, write_callback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &response_data);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);
    curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 1L);

    res = curl_easy_perform(curl);

    if (res != CURLE_OK) {
        curl_easy_cleanup(curl);
        if (res == CURLE_OPERATION_TIMEDOUT) {
            return quantization::unexpected(DataSourceError::Timeout);
        }
        return quantization::unexpected(DataSourceError::NetworkError);
    }

    long http_code = 0;
    curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &http_code);
    curl_easy_cleanup(curl);

    if (http_code != 200) {
        return quantization::unexpected(DataSourceError::NetworkError);
    }

    return response_data;
}

quantization::expected<std::vector<OHLCV>, DataSourceError>
NetworkDataSource::parse_response(const std::string& json_data) {
    try {
        auto j = json::parse(json_data);
        std::vector<OHLCV> result;

        // 假设API返回格式为: {"data": [{"timestamp": ..., "open": ..., ...}]}
        if (!j.contains("data") || !j["data"].is_array()) {
            return quantization::unexpected(DataSourceError::ParseError);
        }

        for (const auto& item : j["data"]) {
            OHLCV ohlcv;

            // 解析时间戳（假设为Unix时间戳）
            if (item.contains("timestamp")) {
                auto timestamp = item["timestamp"].get<int64_t>();
                ohlcv.timestamp = std::chrono::system_clock::from_time_t(timestamp);
            }

            ohlcv.open = item.value("open", 0.0);
            ohlcv.high = item.value("high", 0.0);
            ohlcv.low = item.value("low", 0.0);
            ohlcv.close = item.value("close", 0.0);
            ohlcv.volume = item.value("volume", 0.0);

            result.push_back(ohlcv);
        }

        return result;
    } catch (const json::exception& e) {
        return quantization::unexpected(DataSourceError::ParseError);
    }
}

quantization::expected<std::vector<OHLCV>, DataSourceError>
NetworkDataSource::fetch_ohlcv(
    const std::string& symbol,
    const std::string& interval,
    std::chrono::system_clock::time_point start,
    std::chrono::system_clock::time_point end
) {
    // 构建URL
    auto start_time = std::chrono::system_clock::to_time_t(start);
    auto end_time = std::chrono::system_clock::to_time_t(end);

    std::ostringstream url;
    url << api_endpoint_ << "/ohlcv"
        << "?symbol=" << symbol
        << "&interval=" << interval
        << "&start=" << start_time
        << "&end=" << end_time;

    auto response = http_get(url.str());
    if (!response) {
        return quantization::unexpected(response.error());
    }

    return parse_response(*response);
}

quantization::expected<std::vector<OHLCV>, DataSourceError>
NetworkDataSource::fetch_latest(
    const std::string& symbol,
    const std::string& interval,
    size_t count
) {
    // 构建URL
    std::ostringstream url;
    url << api_endpoint_ << "/ohlcv/latest"
        << "?symbol=" << symbol
        << "&interval=" << interval
        << "&count=" << count;

    auto response = http_get(url.str());
    if (!response) {
        return quantization::unexpected(response.error());
    }

    return parse_response(*response);
}

} // namespace quantization
