import importlib.util
from pathlib import Path

import pytest

fastapi_available = importlib.util.find_spec("fastapi") is not None
pytestmark = pytest.mark.skipif(not fastapi_available, reason="fastapi is not installed")

if fastapi_available:
    from fastapi.testclient import TestClient
    import app


def test_health_shape():
    client = TestClient(app.app)
    response = client.get("/health")
    assert response.status_code == 200
    assert response.json() == {"status": "ok"}


def test_instruments_shape(monkeypatch):
    monkeypatch.setattr(
        app,
        "fetch_instruments",
        lambda instrument_type: [
            {
                "symbol": "600519",
                "exchange": "sh",
                "instrument_type": instrument_type,
                "name": "Maotai",
                "list_date": "2001-08-27",
                "delist_date": None,
                "is_st": False,
                "board": "main_board",
            }
        ],
    )
    client = TestClient(app.app)
    response = client.get("/instruments?type=stock")
    assert response.status_code == 200
    body = response.json()
    assert body[0]["symbol"] == "600519"
    assert body[0]["instrument_type"] == "stock"


def test_bars_shape(monkeypatch):
    monkeypatch.setattr(
        app,
        "fetch_bars",
        lambda symbol, period, start, end: [
            {
                "symbol": symbol,
                "exchange": "sh",
                "period": period,
                "ts": "2026-06-15T07:00:00Z",
                "trading_date": "2026-06-15",
                "open": "10.00",
                "high": "10.50",
                "low": "9.90",
                "close": "10.20",
                "volume": 10000,
                "amount": "102000.00",
            }
        ],
    )
    client = TestClient(app.app)
    response = client.get(
        "/bars?symbol=600519&period=daily&start=2026-06-15T00:00:00Z&end=2026-06-15T23:59:59Z"
    )
    assert response.status_code == 200
    assert response.json()[0]["close"] == "10.20"


def test_app_file_is_parseable():
    assert Path(app.__file__).name == "app.py"
