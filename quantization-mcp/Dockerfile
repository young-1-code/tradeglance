FROM ubuntu:22.04

# 安装依赖
RUN apt-get update && apt-get install -y \
    build-essential \
    cmake \
    git \
    libcurl4-openssl-dev \
    nlohmann-json3-dev \
    && rm -rf /var/lib/apt/lists/*

# 设置工作目录
WORKDIR /app

# 复制源代码
COPY . .

# 编译
RUN mkdir -p build && cd build && \
    cmake .. && \
    make -j$(nproc)

# 设置环境变量
ENV MARKET_DATA_API=http://localhost:8080/api

# 暴露端口（如果需要）
# EXPOSE 9999

# 运行服务器
CMD ["./build/quantization-mcp"]
