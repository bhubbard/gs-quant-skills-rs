# Backtest Results, 19 Summary Stats & P&L Explain (`gs-quant-core`)

## `result_summary` Accounting

Each `ResultSummaryRow` holds:
- `price`: Mark-to-market of open live positions.
- `cumulative_cash`: Initial cash minus entry MTM outflows plus exit MTM proceeds.
- `transaction_costs`: Cumulative entry + exit fees (always `<= 0.0`).
- `total`: `price + cumulative_cash + transaction_costs`.
- `risks`: Any additional requested `RiskMeasure` series.

---

## `SummaryStats` (All 19 Goldman Statistics + Diagnostics)

Calling `backtest.summary_stats()?` computes:
- **Date metadata (3)**: `start_date`, `end_date`, `duration_days`.
- **19 Goldman statistics**:
  1. `total_pnl`
  2. `total_transaction_costs`
  3. `total_trades`
  4. `peak_pnl`
  5. `annualised_return`
  6. `annualised_volatility`
  7. `sharpe_ratio`
  8. `sortino_ratio`
  9. `max_drawdown`
  10. `max_drawdown_duration_days`
  11. `calmar_ratio`
  12. `current_drawdown`
  13. `average_daily_pnl`
  14. `daily_pnl_std_dev`
  15. `best_day`
  16. `worst_day`
  17. `pct_positive_days` (note: a positive day can still contain losing closed trades)
  18. `skewness`
  19. `kurtosis` (excess kurtosis)
- **Closed-trade diagnostics**: `closed_trades`, `win_rate_pct`, `avg_holding_days`, `avg_winning_hold_days`, `avg_losing_hold_days`.

---

## P&L Explain (`fx_pnl_definition()`)

When `BacktestConfig::pnl_explain` is set (for example `Some(fx_pnl_definition())`), each `PnlExplainRow` records cumulative attributed P&L (`PNL_delta`, `PNL_gamma`, `VegaPnL`) plus `unexplained = total_pnl_change - sum(attributes)`.

Remember that first-order delta, second-order gamma, and vega **do not sum to 100% of option P&L**: time decay (`theta`) and higher-order cross terms live in `unexplained`.
