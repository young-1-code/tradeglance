# TradeGlance collector-python

Run the FastAPI sidecar from this directory:

```bash
uvicorn app:app --port 8000
```

The Rust `tg-market-data` service calls these endpoints:

- `GET /health`
- `GET /instruments?type=stock|etf`
- `GET /calendar?start=YYYY-MM-DD&end=YYYY-MM-DD`
- `GET /bars?symbol=600519&period=daily&start=...&end=...`
- `GET /snapshot?symbols=600519,159915`
- `GET /adjust_factors?symbol=600519`

If `akshare` is not installed, the app still imports and `/health` responds, while data endpoints return HTTP 503.
