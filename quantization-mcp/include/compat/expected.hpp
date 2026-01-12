#pragma once

#include <variant>
#include <stdexcept>
#include <type_traits>

namespace quantization {

// 简单的 expected 实现，兼容 C++17
template<typename T, typename E>
class expected {
public:
    expected(const T& value) : data_(value) {}
    expected(T&& value) : data_(std::move(value)) {}

    // 从错误类型构造
    expected(const E& error) : data_(error) {}
    expected(E&& error) : data_(std::move(error)) {}

    bool has_value() const {
        return std::holds_alternative<T>(data_);
    }

    explicit operator bool() const {
        return has_value();
    }

    const T& value() const {
        if (!has_value()) {
            throw std::runtime_error("bad expected access");
        }
        return std::get<T>(data_);
    }

    T& value() {
        if (!has_value()) {
            throw std::runtime_error("bad expected access");
        }
        return std::get<T>(data_);
    }

    const T& operator*() const {
        return value();
    }

    T& operator*() {
        return value();
    }

    const T* operator->() const {
        return &value();
    }

    T* operator->() {
        return &value();
    }

    const E& error() const {
        if (has_value()) {
            throw std::runtime_error("no error in expected");
        }
        return std::get<E>(data_);
    }

    E& error() {
        if (has_value()) {
            throw std::runtime_error("no error in expected");
        }
        return std::get<E>(data_);
    }

private:
    std::variant<T, E> data_;
};

// 辅助函数，直接返回错误类型
template<typename E>
E unexpected(E error) {
    return error;
}

} // namespace quantization
