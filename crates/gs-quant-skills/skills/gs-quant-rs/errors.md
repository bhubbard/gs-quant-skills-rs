# Errors & Diagnostics (`gs-quant-core`)

`gs_quant_core` replaces Python runtime exceptions and `ErrorValue` wrappers with strongly typed `Result<T, QuantError>` returns.

## `QuantError` Variants

| Variant | Common Cause | Resolution |
|---|---|---|
| `QuantError::MissingMarketData` | Valuation date or series name absent from `MarketHistory` | Load CSV or insert quotes for that date; verify `MissingDataStrategy::FillForward` |
| `QuantError::InvalidTenor` | Malformed tenor string | Use `<positive-int>(b\|w\|m\|y)` such as `"1b"`, `"3m"`, `"1y"` |
| `QuantError::InvalidCsv` | Missing column name or non-`YYYY-MM-DD` / non-numeric cell | Pass `--date-col` / `--price-col` matching CSV headers |
| `QuantError::EmptySeries` | Backtest schedule or volatility window lacks enough points | Expand date range or ensure `len >= window + 1` |
| `QuantError::ZeroRiskSensitivity` | `ScalingType::RiskMeasure` or `Hedge` targeted an instrument with zero unit risk | Check option moneyness/expiry or switch hedging instrument |
| `QuantError::InvalidScenario` | `RollFwd` target date precedes snapshot date | Ensure `target_date >= snapshot.date` |

## Scenario Matched-Points Pitfall

`MarketDataShockBasedScenario::apply` returns `ScenarioResult { snapshot, matched_points }`. If `matched_points == 0`, your `MarketDataPattern` did not match any key in `snapshot.quotes` (for example `"EUR/USD"` instead of `"EURUSD"`). Check `matched_points > 0` before interpreting shocked valuations.
