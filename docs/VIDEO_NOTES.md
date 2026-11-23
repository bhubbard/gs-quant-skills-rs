# Video Notes: "A $58 Billion Bank Open-Sourced an AI Skill for Backtesting. I Tested It"

Source: <https://www.youtube.com/watch?v=cLUa-qOQS5Q>  
Channel: **Unbiased Trading** (Published 2026-11-22)

This document captures every technical takeaway, gotcha, and empirical observation from the video and maps each one directly to its Pure Rust implementation in `gs-quant-skills-rs`.

---

## 1. The Core Access Catch (Why an Offline Rust Engine Exists)

- **Observation**: Installing `gs_quant/skills` teaches an AI coding assistant Goldman Sachs' `gs_quant` workflows (`list`, `install --global` / `--project`), **but external users cannot call Goldman's Marquee pricing backend** (`GsSession.use()` / `IRSwap.price()`) without institutional credentials.
- **Video workaround**: Brought in external price data (e.g. Massive / CSV feeds) and let the AI apply Goldman's backtest architecture against local prices.
- **Rust implementation**: `gs-quant-core` implements `Pricer` / `LocalPricer` (Black-Scholes-Merton, Garman-Kohlhagen, FX forwards, spot equities, par-curve swaps) and `MarketHistory` / `DataSeries::load_close_csv` directly in Rust—requiring **zero Python** and **zero Goldman login**.

---

## 2. When Do You Even Need a Backtest Engine?

- **Observation**: If you buy and hold a fixed instrument or static portfolio over a date range with no triggers, rebalancing, or hedging, **you do not need a stateful backtest engine**—just price the resolved position over the historical dates.
- **Stateful trigger**: Once trades depend on path state (e.g. buying more when price drops, capping open positions, hedging when risk breaches a band), the engine must walk date by date and carry positions forward.
- **Rust implementation**:
  - Static buy-and-hold: `HistoricalPricer::run(&portfolio, &dates, &history, &pricer)`
  - Path-dependent strategies: `GenericEngine::run(&strategy, &config, &history, &pricer)`

---

## 3. Triggers, Risk Bands, and Open-Position Caps

- **Trigger + Action Separation**: `Trigger` defines *when* to act (calendar `PeriodicTrigger`, explicit `Dates`, market level `MktTrigger`, risk threshold `StrategyRiskTrigger`, `MeanReversion`, `OpenPositionCount`); `Action` defines *what* to do (`AddTrade`, `AddScaledTrade`, `Hedge`, `ExitTrade`, `ExitAllPositions`).
- **Dual-Sided Risk Band (±50,000 FX Delta)**: Monitor portfolio `FxDelta`; stay idle inside `[-50_000, +50_000]`; fire hedge action when breaching above `+50,000` or below `-50_000`.
  - **Rust**: `TriggerCondition::Aggregate { mode: AggregateMode::AnyOf, conditions: [StrategyRisk { Above, 50_000.0 }, StrategyRisk { Below, -50_000.0 }] }` paired with `Action::Hedge`.
- **Open-Position Cap**: Prevent runaway stacking of orders when already at max capacity.
  - **Rust**: `TriggerCondition::OpenPositionCount { limit, direction: TriggerDirection::Below }` combined with entry condition via `AggregateMode::AllOf`.

---

## 4. Risk-Scaled Position Sizing

- **Observation**: Putting the same dollar notional into two options with different deltas produces unequal currency exposure. Goldman's `ScalingActionType.risk_measure` divides the target risk level by the unit risk of the candidate trade at entry so every trade starts with the exact same target sensitivity.
- **Rust**: `Action::AddScaledTrade { scaling_type: ScalingType::RiskMeasure(measure), scaling_level, .. }` computes `scale = scaling_level / unit_risk` (returning `QuantError::ZeroRiskSensitivity` if unit sensitivity is zero).

---

## 5. Transaction Costs (Entry vs. Exit & Combined Floors)

- **Observation**: Real costs can be flat fees, proportional to notional/quantity, proportional to a risk measure (e.g. 1% of option premium `|Price|`), or combined (`max(min_fee, bps_fee)`). Crucially, **exit costs can differ from entry costs** (`transaction_cost` vs `transaction_cost_exit`), so the engine must carry exit cost metadata onto the open position until it closes.
- **Rust**: Every entry action carries separate `entry_cost: TransactionCostModel` and `exit_cost: TransactionCostModel` (`Zero`, `Constant`, `Scaled`, `Aggregate { agg_type: Sum | Max | Min, .. }`).

---

## 6. Scenarios & Shock Testing

- **Observation**: Shock spot prices, FX rates, or implied volatilities and reprice the entire open portfolio under the exact same scenario. Also watch out for silent mismatches where a pattern matches zero market data points.
- **Rust**: `MarketDataShockBasedScenario::apply(&snapshot)` supports `Absolute`, `Proportional`, and `Override` shocks on `MarketDataPattern` and returns `ScenarioResult { snapshot, matched_points }` so callers can assert `matched_points > 0`. `RollFwd` advances valuation time with `ForwardRealisation::SpotMovesToForward` or `KeepSpotFixed`.

---

## 7. Realized Volatility Denominator Convention (`N - 1` vs `N`)

- **Observation**: Volatility can be measured around the sample mean (`N - 1` denominator) or around zero (`N` denominator, RMS convention used in variance/correlation swaps and zero-drift models).
- **Rust**: `realized_volatility(&series, window, returns_type, ann_factor, VolatilityMean::SampleMean | VolatilityMean::ZeroMean)` implements both conventions explicitly via enum (no ambiguous booleans).

---

## 8. Results, 19 Summary Stats & P&L Attribution

- **19 Summary Statistics**: Beyond win rate, inspect total P&L, transaction costs, trade count, peak P&L, annualised return/volatility, Sharpe, Sortino, max drawdown, max drawdown duration, Calmar, current drawdown, average/std daily P&L, best/worst day, % positive days (remember: a positive day can contain losing trades), skewness, and excess kurtosis.
  - **Rust**: `SummaryStats` computes all 3 date/duration metadata fields + all 19 Goldman statistics, plus closed-trade `win_rate_pct` and `avg_holding_days`.
- **Three-Bucket Cash Accounting**: `Total = Price (live MTM) + Cumulative Cash (closed trades) + Transaction Costs (<= 0)`.
- **P&L Explain & The Unexplained Residual**: For options, decomposing daily P&L into Delta, Gamma, and Vega (`fx_pnl_definition()`) leaves out higher-order cross terms and **theta / time decay**, producing a non-zero unexplained residual (`total_change - explained_sum`) that must be tracked explicitly.
  - **Rust**: `PnlExplainRow` tracks each named attribute plus `unexplained` and `total_change`.

---

## 9. Empirical Lesson on Mean Reversion

- **Video Test**: Tested Goldman's default `MeanReversionTrigger` rule (`30`-day rolling mean/std, `z_score_bound = 2.0`, exit on crossing back through the rolling mean) on EUR and on the 500 most active US stocks over 5 years.
- **Result**: ~70% win rate on EUR and ~65% win rate on US stocks, **yet only ~1.55% annualized return** (heavily underperforming SPY) even before commissions.
- **Why**: Because the exit condition waits indefinitely for price to revert to the rolling mean, losing trades stay open the longest and suffer asymmetric adverse tail moves ("the trades left open the longest normally lose the most on average").
- **Rust verification**: `gs-quant-cli backtest mean-reversion` and `examples/eurusd_mean_reversion.rs` output both the 19 Goldman summary stats AND closed-trade win rate + average holding duration (and winning vs losing average holding days) so this exact dynamic is immediately visible.
