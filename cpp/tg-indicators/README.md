# tg-indicators

C++20 gRPC technical-indicator service for TradeGlance Phase 1.

## Build

```bash
cmake -S cpp/tg-indicators -B cpp/tg-indicators/build -DCMAKE_BUILD_TYPE=Release
cmake --build cpp/tg-indicators/build -j
```

The build generates C++ protobuf and gRPC stubs from
`crates/tg-contracts/proto/tg/v1/contracts.proto` into the CMake build
directory. It uses system `protobuf`, `grpc++`, and `gtest` libraries; it does
not download dependencies.

## Run

```bash
TG_INDICATORS_PORT=50053 ./cpp/tg-indicators/build/tg-indicators
```

If `TG_INDICATORS_PORT` is not set, the server listens on `0.0.0.0:50053`.

## Test

```bash
ctest --test-dir cpp/tg-indicators/build --output-on-failure
```

All output series are aligned to the input bar timestamps. Warm-up positions are
encoded as IEEE quiet NaN. Requests with too few bars for the requested period
return `INVALID_ARGUMENT`.

