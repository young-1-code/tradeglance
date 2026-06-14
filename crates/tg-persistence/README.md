# tg-persistence

Shared storage and data-access crate for TradeGlance Phase 0.

PostgreSQL stores metadata, calendars, adjustment factors, fetch state, and `latest_snapshots`.
Parquet stores immutable time-series history under the spec layout:

- `data/bars/{daily,minute1,minute5}/symbol=<SYM>/year=<YYYY>/part.parquet`
- `data/snapshots/symbol=<SYM>/date=<YYYYMMDD>/part.parquet`

Parquet writes use a temp file in the destination directory followed by `rename`, under the
Phase 0 single-writer assumption. Readers open files read-only. DuckDB is intentionally deferred;
the current read path uses `parquet` + `arrow` directly.

## PostgreSQL integration tests

Default tests do not require external services. To run PostgreSQL-backed tests:

```bash
docker run --rm -e POSTGRES_PASSWORD=postgres -p 5432:5432 postgres:16
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres
cargo test -p tg-persistence -F pg_integration
```
