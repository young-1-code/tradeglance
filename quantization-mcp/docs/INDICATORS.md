# 技术指标详细说明

本文档详细介绍所有支持的技术指标的计算方法、参数和使用场景。

## 目录

1. [移动平均类](#移动平均类)
2. [动量指标](#动量指标)
3. [波动率指标](#波动率指标)
4. [趋势指标](#趋势指标)
5. [成交量指标](#成交量指标)

---

## 移动平均类

### SMA (Simple Moving Average) - 简单移动平均

**计算公式:**
```
SMA = (P1 + P2 + ... + Pn) / n
```

**参数:**
- `period`: 周期长度（默认值：5, 10, 20, 50, 200）

**使用场景:**
- 识别趋势方向
- 支撑/阻力位
- 金叉/死叉信号

**示例:**
```json
{
  "method": "calculate_indicator",
  "params": {
    "indicator": "sma_20",
    "symbol": "AAPL",
    "interval": "1d",
    "count": 100
  }
}
```

**解读:**
- 价格在SMA上方：上升趋势
- 价格在SMA下方：下降趋势
- 短期SMA上穿长期SMA：金叉（买入信号）
- 短期SMA下穿长期SMA：死叉（卖出信号）

---

### EMA (Exponential Moving Average) - 指数移动平均

**计算公式:**
```
EMA(t) = Price(t) × k + EMA(t-1) × (1 - k)
其中 k = 2 / (period + 1)
```

**参数:**
- `period`: 周期长度（默认值：5, 10, 12, 20, 26, 50）

**特点:**
- 对近期价格赋予更高权重
- 比SMA更敏感，反应更快

**使用场景:**
- 短期交易信号
- MACD指标的基础
- 动态支撑/阻力

---

## 动量指标

### RSI (Relative Strength Index) - 相对强弱指标

**计算公式:**
```
RS = 平均涨幅 / 平均跌幅
RSI = 100 - (100 / (1 + RS))
```

**参数:**
- `period`: 周期长度（默认值：14）

**取值范围:** 0-100

**使用场景:**
- 超买/超卖判断
- 背离信号
- 趋势确认

**解读:**
- RSI > 70：超买区域，可能回调
- RSI < 30：超卖区域，可能反弹
- RSI = 50：中性区域

**示例:**
```json
{
  "method": "calculate_indicator",
  "params": {
    "indicator": "rsi_14",
    "symbol": "AAPL",
    "interval": "1d",
    "count": 100
  }
}
```

---

### MACD (Moving Average Convergence Divergence)

**计算公式:**
```
MACD线 = EMA(12) - EMA(26)
信号线 = EMA(MACD, 9)
柱状图 = MACD线 - 信号线
```

**参数:**
- `fast_period`: 快速EMA周期（默认：12）
- `slow_period`: 慢速EMA周期（默认：26）
- `signal_period`: 信号线周期（默认：9）

**使用场景:**
- 趋势跟踪
- 动量变化
- 买卖信号

**解读:**
- MACD线上穿信号线：买入信号
- MACD线下穿信号线：卖出信号
- 柱状图扩大：趋势加强
- 柱状图缩小：趋势减弱

---

## 波动率指标

### Bollinger Bands - 布林带

**计算公式:**
```
中轨 = SMA(n)
上轨 = 中轨 + k × σ
下轨 = 中轨 - k × σ
```

**参数:**
- `period`: 周期长度（默认：20）
- `std_dev`: 标准差倍数（默认：2.0）

**使用场景:**
- 波动率测量
- 超买/超卖判断
- 突破信号

**解读:**
- 价格触及上轨：可能超买
- 价格触及下轨：可能超卖
- 带宽收窄：波动率降低，可能突破
- 带宽扩大：波动率增加

**示例:**
```json
{
  "method": "calculate_indicator",
  "params": {
    "indicator": "bb_20",
    "symbol": "AAPL",
    "interval": "1d",
    "count": 100
  }
}
```

---

### ATR (Average True Range) - 平均真实波幅

**计算公式:**
```
TR = max(H - L, |H - C_prev|, |L - C_prev|)
ATR = EMA(TR, period)
```

**参数:**
- `period`: 周期长度（默认：14）

**使用场景:**
- 波动率测量
- 止损位设置
- 仓位管理

**解读:**
- ATR值越大：波动越大，风险越高
- ATR值越小：波动越小，市场平静

---

## 趋势指标

### ADX (Average Directional Index) - 平均趋向指标

**计算公式:**
```
+DI = EMA(+DM, period) / ATR × 100
-DI = EMA(-DM, period) / ATR × 100
DX = |+DI - -DI| / (+DI + -DI) × 100
ADX = EMA(DX, period)
```

**参数:**
- `period`: 周期长度（默认：14）

**取值范围:** 0-100

**使用场景:**
- 趋势强度判断
- 趋势/震荡市场识别

**解读:**
- ADX > 25：强趋势
- ADX < 20：弱趋势或震荡
- +DI > -DI：上升趋势
- -DI > +DI：下降趋势

---

### CCI (Commodity Channel Index) - 顺势指标

**计算公式:**
```
TP = (H + L + C) / 3
CCI = (TP - SMA(TP)) / (0.015 × 平均偏差)
```

**参数:**
- `period`: 周期长度（默认：20）

**取值范围:** 通常在 -100 到 +100 之间

**使用场景:**
- 超买/超卖判断
- 趋势反转信号

**解读:**
- CCI > +100：超买
- CCI < -100：超卖
- CCI穿越0线：趋势变化

---

## 其他指标

### Stochastic Oscillator - 随机指标

**计算公式:**
```
%K = (C - L_n) / (H_n - L_n) × 100
%D = SMA(%K, d_period)
```

**参数:**
- `k_period`: %K周期（默认：14）
- `d_period`: %D周期（默认：3）

**取值范围:** 0-100

**解读:**
- %K > 80：超买
- %K < 20：超卖
- %K上穿%D：买入信号
- %K下穿%D：卖出信号

---

### Williams %R - 威廉指标

**计算公式:**
```
%R = (H_n - C) / (H_n - L_n) × -100
```

**参数:**
- `period`: 周期长度（默认：14）

**取值范围:** -100 到 0

**解读:**
- %R > -20：超买
- %R < -80：超卖

---

### OBV (On-Balance Volume) - 能量潮

**计算公式:**
```
如果 C > C_prev: OBV = OBV_prev + V
如果 C < C_prev: OBV = OBV_prev - V
如果 C = C_prev: OBV = OBV_prev
```

**使用场景:**
- 成交量趋势确认
- 价量背离识别

**解读:**
- OBV上升 + 价格上升：确认上涨趋势
- OBV下降 + 价格下降：确认下跌趋势
- OBV与价格背离：可能反转信号

---

## 组合使用建议

### 趋势跟踪策略
```
- SMA(50) + SMA(200)：识别长期趋势
- MACD：确认趋势动量
- ADX：判断趋势强度
```

### 超买超卖策略
```
- RSI：主要信号
- Stochastic：确认信号
- Bollinger Bands：价格位置
```

### 突破策略
```
- Bollinger Bands：识别突破
- ATR：设置止损
- OBV：成交量确认
```

---

## 注意事项

1. **参数调整**: 不同市场和时间周期可能需要不同参数
2. **组合使用**: 单一指标容易产生假信号，建议组合使用
3. **市场环境**: 趋势市场和震荡市场适用的指标不同
4. **回测验证**: 使用前应进行充分的历史数据回测
5. **风险管理**: 指标只是辅助工具，不能替代风险管理

---

## 参考资料

- Technical Analysis of the Financial Markets - John J. Murphy
- New Concepts in Technical Trading Systems - J. Welles Wilder
- Trading Systems and Methods - Perry J. Kaufman
