from __future__ import annotations

from datetime import date, datetime, time, timezone
from decimal import Decimal, InvalidOperation
from typing import Any

import pandas as pd
from fastapi import FastAPI, HTTPException, Query

app = FastAPI(title="TradeGlance Market Data Collector")


@app.get("/health")
def health() -> dict[str, str]:
    return {"status": "ok"}


@app.get("/instruments")
def instruments(type: str = Query(pattern="^(stock|etf)$")) -> list[dict[str, Any]]:
    return fetch_instruments(type)


@app.get("/calendar")
def calendar(start: str, end: str) -> list[dict[str, Any]]:
    return fetch_calendar(start, end)


@app.get("/bars")
def bars(symbol: str, period: str, start: str, end: str) -> list[dict[str, Any]]:
    return fetch_bars(symbol, period, start, end)


@app.get("/snapshot")
def snapshot(symbols: str) -> list[dict[str, Any]]:
    parsed = [symbol.strip() for symbol in symbols.split(",") if symbol.strip()]
    return fetch_snapshot(parsed)


@app.get("/adjust_factors")
def adjust_factors(symbol: str) -> list[dict[str, Any]]:
    return fetch_adjust_factors(symbol)


def fetch_instruments(instrument_type: str) -> list[dict[str, Any]]:
    ak = _akshare()
    if instrument_type == "stock":
        frame = ak.stock_info_a_code_name()
    else:
        frame = ak.fund_etf_spot_em()
    rows: list[dict[str, Any]] = []
    for _, row in frame.iterrows():
        symbol = _pick(row, "code", "代码", "基金代码")
        if not symbol:
            continue
        symbol = str(symbol).zfill(6)
        name = str(_pick(row, "name", "名称", "基金简称") or symbol)
        rows.append(
            {
                "symbol": symbol,
                "exchange": _exchange(symbol),
                "instrument_type": instrument_type,
                "name": name,
                "list_date": _date_string(_pick(row, "上市时间", "list_date") or "1970-01-01"),
                "delist_date": None,
                "is_st": "ST" in name.upper(),
                "board": _board(symbol),
            }
        )
    return rows


def fetch_calendar(start: str, end: str) -> list[dict[str, Any]]:
    ak = _akshare()
    start_date = date.fromisoformat(start)
    end_date = date.fromisoformat(end)
    frame = ak.tool_trade_date_hist_sina()
    trading = {
        date.fromisoformat(_date_string(_pick(row, "trade_date", "交易日")))
        for _, row in frame.iterrows()
    }
    days = pd.date_range(start_date, end_date, freq="D")
    return [
        {"date": item.date().isoformat(), "is_trading_day": item.date() in trading}
        for item in days
    ]


def fetch_bars(symbol: str, period: str, start: str, end: str) -> list[dict[str, Any]]:
    ak = _akshare()
    start_dt = datetime.fromisoformat(start.replace("Z", "+00:00"))
    end_dt = datetime.fromisoformat(end.replace("Z", "+00:00"))
    if period == "daily":
        frame = ak.stock_zh_a_hist(
            symbol=symbol,
            period="daily",
            start_date=start_dt.date().strftime("%Y%m%d"),
            end_date=end_dt.date().strftime("%Y%m%d"),
            adjust="",
        )
    else:
        ak_period = "1" if period in {"min1", "1m"} else "5"
        frame = ak.stock_zh_a_hist_min_em(
            symbol=symbol,
            period=ak_period,
            start_date=start_dt.strftime("%Y-%m-%d %H:%M:%S"),
            end_date=end_dt.strftime("%Y-%m-%d %H:%M:%S"),
            adjust="",
        )
    rows: list[dict[str, Any]] = []
    for _, row in frame.iterrows():
        trading_date = _date_string(_pick(row, "日期", "时间", "date"))
        ts = _bar_ts_utc(_pick(row, "日期", "时间", "date"), period)
        rows.append(
            {
                "symbol": symbol,
                "exchange": _exchange(symbol),
                "period": "min1" if period in {"min1", "1m"} else "min5" if period in {"min5", "5m"} else "daily",
                "ts": ts,
                "trading_date": trading_date,
                "open": _money(_pick(row, "开盘", "open")),
                "high": _money(_pick(row, "最高", "high")),
                "low": _money(_pick(row, "最低", "low")),
                "close": _money(_pick(row, "收盘", "close")),
                "volume": int(Decimal(str(_pick(row, "成交量", "volume") or 0))),
                "amount": _money(_pick(row, "成交额", "amount") or 0),
            }
        )
    return rows


def fetch_snapshot(symbols: list[str]) -> list[dict[str, Any]]:
    ak = _akshare()
    frame = ak.stock_zh_a_spot_em()
    requested = set(symbols)
    rows: list[dict[str, Any]] = []
    for _, row in frame.iterrows():
        symbol = str(_pick(row, "代码", "code") or "").zfill(6)
        if symbol not in requested:
            continue
        last = _money(_pick(row, "最新价", "last") or 0)
        bid_price, bid_volume, ask_price, ask_volume = _quote_depth(ak, symbol)
        rows.append(
            {
                "symbol": symbol,
                "exchange": _exchange(symbol),
                "ts": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
                "trading_date": datetime.now(timezone.utc).date().isoformat(),
                "last": last,
                "open": _money(_pick(row, "今开", "open") or last),
                "high": _money(_pick(row, "最高", "high") or last),
                "low": _money(_pick(row, "最低", "low") or last),
                "pre_close": _money(_pick(row, "昨收", "pre_close") or last),
                "volume": int(Decimal(str(_pick(row, "成交量", "volume") or 0))),
                "amount": _money(_pick(row, "成交额", "amount") or 0),
                "bid_price": bid_price,
                "bid_volume": bid_volume,
                "ask_price": ask_price,
                "ask_volume": ask_volume,
            }
        )
    return rows


def fetch_adjust_factors(symbol: str) -> list[dict[str, Any]]:
    ak = _akshare()
    if hasattr(ak, "stock_zh_a_daily"):
        frame = ak.stock_zh_a_daily(symbol=_ak_symbol(symbol), adjust="qfq-factor")
    else:
        frame = pd.DataFrame()
    rows: list[dict[str, Any]] = []
    for _, row in frame.iterrows():
        ex_date = _date_string(_pick(row, "date", "日期"))
        factor = _money(_pick(row, "qfq_factor", "factor", "复权因子") or 1)
        rows.append({"symbol": symbol, "ex_date": ex_date, "factor": factor})
    return rows


def _akshare() -> Any:
    try:
        import akshare as ak  # type: ignore
    except ImportError as exc:
        raise HTTPException(status_code=503, detail="akshare is not installed") from exc
    return ak


def _pick(row: Any, *names: str) -> Any:
    for name in names:
        if name in row and pd.notna(row[name]):
            return row[name]
    return None


def _money(value: Any) -> str:
    try:
        return format(Decimal(str(value)), "f")
    except (InvalidOperation, ValueError):
        return "0"


def _date_string(value: Any) -> str:
    text = str(value)
    if " " in text:
        text = text.split(" ", 1)[0]
    return date.fromisoformat(text.replace("/", "-")).isoformat()


def _bar_ts_utc(value: Any, period: str) -> str:
    text = str(value).replace("/", "-")
    if len(text) == 10:
        local_dt = datetime.combine(date.fromisoformat(text), time(15, 0))
    else:
        local_dt = datetime.fromisoformat(text)
    cst = timezone.utc if local_dt.tzinfo else timezone.utc
    if cst is timezone.utc and local_dt.tzinfo is None:
        local_dt = local_dt.replace(tzinfo=timezone.utc)
    if period == "daily":
        local_dt = datetime.combine(local_dt.date(), time(15, 0), timezone.utc)
    return local_dt.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def _quote_depth(ak: Any, symbol: str) -> tuple[list[str], list[int], list[str], list[int]]:
    zeros_price = ["0", "0", "0", "0", "0"]
    zeros_volume = [0, 0, 0, 0, 0]
    if not hasattr(ak, "stock_bid_ask_em"):
        return zeros_price, zeros_volume, zeros_price, zeros_volume
    try:
        frame = ak.stock_bid_ask_em(symbol=symbol)
    except Exception:
        return zeros_price, zeros_volume, zeros_price, zeros_volume
    values = {str(row["item"]): row["value"] for _, row in frame.iterrows() if "item" in row}
    bid_price = [_money(values.get(f"buy_{i}_price", 0)) for i in range(1, 6)]
    bid_volume = [int(Decimal(str(values.get(f"buy_{i}_vol", 0)))) for i in range(1, 6)]
    ask_price = [_money(values.get(f"sell_{i}_price", 0)) for i in range(1, 6)]
    ask_volume = [int(Decimal(str(values.get(f"sell_{i}_vol", 0)))) for i in range(1, 6)]
    return bid_price, bid_volume, ask_price, ask_volume


def _exchange(symbol: str) -> str:
    if symbol.startswith("6"):
        return "sh"
    if symbol.startswith(("8", "4")):
        return "bj"
    return "sz"


def _board(symbol: str) -> str:
    if symbol.startswith("688"):
        return "star"
    if symbol.startswith("300"):
        return "chi_next"
    if symbol.startswith(("8", "4")):
        return "bj"
    return "main_board"


def _ak_symbol(symbol: str) -> str:
    return f"{_exchange(symbol)}{symbol}"
