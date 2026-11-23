# Backtesting Guide (`gs-quant-core`)

## Do You Need a Backtest Engine?

**If your strategy is a single instrument or static portfolio held over a date range with no triggers, rebalancing, hedging, or caps, do not use `GenericEngine`: use `HistoricalPricer::run(&portfolio, &dates, &history, &measure, &pricer)`.**

Reach for `GenericEngine` whenever decisions depend on state or market path:
- Entering or rolling trades on schedules or market signals
- Buying more when price drops or scaling into mean reversion
- Enforcing an open-position cap (`OpenPositionCount`)
- Triggering hedges when portfolio risk breaches `±X` (`StrategyRisk` inside `AggregateMode::AnyOf`)

---

## 1. Architecture Overview

| Concept | Role in `gs_quant_core` |
|---|---|
| `Strategy` | Optional `initial_portfolio` + ordered `Vec<Trigger>` |
| `Trigger` | Pairs a `TriggerCondition` (*when*) with `Vec<Action>` (*what*) |
| `GenericEngine` | Walks evaluation dates, closes due trades, fires triggers, tracks cash, costs, and P&L Explain |

---

## 2. Triggers

- **`TriggerCondition::Periodic`**: Schedule between `start_date` and `end_date` by `Tenor` (`1b`, `1w`, `1m`, `3m`, `6m`, `1y`), skipping weekends and `calendar` holidays.
- **`TriggerCondition::Dates`**: Explicit `Vec<NaiveDate>`.
- **`TriggerCondition::Market`**: Fires when a named `DataSeries` is `Above`, `Below`, or `Equal` to `level`.
- **`TriggerCondition::StrategyRisk`**: Monitors aggregate live portfolio `RiskMeasure` against `level`.
- **`TriggerCondition::MeanReversion`**: Tracks flat (`0`), long (`+1`), or short (`-1`) state using `rolling_mean_window`, `rolling_std_window`, and `z_score_bound`. Enters `-1` when `(P - mean)/std > z_score_bound`, `+1` when `< -z_score_bound`, and exits on crossing back through `rolling_mean`.
- **`TriggerCondition::OpenPositionCount`**: Checks `open_positions.len()` against `limit` (`TriggerDirection::Below` blocks new entries when full).
- **`TriggerCondition::Aggregate`**: Combines child conditions with `AggregateMode::AllOf` (AND) or `AggregateMode::AnyOf` (OR).
- **`TriggerCondition::Not`**: Inverts any child condition.

### Dual-Sided Risk Band (±50,000 FX Delta)

```rust
use gs_quant_core::{
    Action, AggregateMode, ExpirySpec, Instrument, RiskMeasure, TradeDuration,
    TransactionCostModel, Trigger, TriggerCondition, TriggerDirection,
};

let risk_band = TriggerCondition::Aggregate {
    mode: AggregateMode::AnyOf,
    conditions: vec![
        TriggerCondition::StrategyRisk {
            measure: RiskMeasure::FxDelta,
            level: 50_000.0,
            direction: TriggerDirection::Above,
        },
        TriggerCondition::StrategyRisk {
            measure: RiskMeasure::FxDelta,
            level: -50_000.0,
            direction: TriggerDirection::Below,
        },
    ],
};

let hedge = Action::Hedge {
    risk: RiskMeasure::FxDelta,
    hedge_instrument: Instrument::fx_forward(
        "hedge_fwd",
        "EURUSD",
        1.0,
        ExpirySpec::parse("1m")?,
    ),
    duration: TradeDuration::Tenor("1m".parse()?),
    name: Some("delta_hedge".into()),
    entry_cost: TransactionCostModel::Constant(50.0),
    exit_cost: TransactionCostModel::Constant(25.0),
};

let hedge_trigger = Trigger::new(risk_band, vec![hedge]);
```

### Capping Open Positions (`MeanReversion` AND `OpenPositionCount`)

```rust
let capped_mr = TriggerCondition::Aggregate {
    mode: AggregateMode::AllOf,
    conditions: vec![
        TriggerCondition::OpenPositionCount {
            limit: 1,
            direction: TriggerDirection::Below,
        },
        TriggerCondition::MeanReversion {
            series_name: "EURUSD".into(),
            z_score_bound: 2.0,
            rolling_mean_window: 30,
            rolling_std_window: 30,
        },
    ],
};
```

> **Note:** When using `MeanReversion` inside `AllOf` with `OpenPositionCount { limit: 1, Below }`, remember that exit signals occur while `open_positions.len() == 1`. Put `MeanReversion` as the primary top-level trigger when you only allow one position at a time (since `MeanReversion` itself holds `0 / +1 / -1` state and never double-enters), OR gate only entry actions with an additional `OpenPositionCount` trigger when stacking multiple strategies.

---

## 3. Actions & Sizing

- **`Action::AddTrade`**: Resolves template instrument on entry date (freezing relative strike/expiry/par rate for the trade's life).
- **`Action::AddScaledTrade`**:
  - `ScalingType::Size`: multiplies size by `scaling_level`.
  - `ScalingType::RiskMeasure(m)`: sets `scale = scaling_level / unit_risk` so trades with different deltas begin with the exact same target sensitivity.
  - `ScalingType::Nav`: deploys `scaling_level` initially, then recycled closed-trade proceeds.
- **`Action::Hedge`**: Computes `-portfolio_risk / hedge_unit_risk` to neutralize `risk`.
- **`Action::ExitTrade { name }`** / **`Action::ExitAllPositions`**: Execute actions in sequence (put exits *before* adds within the same trigger's `actions` vector).

---

## 4. Transaction Costs (`entry_cost` vs `exit_cost`)

Every entry action carries independent `entry_cost` and `exit_cost` models:
- `TransactionCostModel::Zero`
- `TransactionCostModel::Constant(amount)`
- `TransactionCostModel::notional_bps(rate)` / `per_unit(rate)` / `by_measure(measure, rate)`
- `TransactionCostModel::aggregate(models, TransactionAggType::Sum | Max | Min)` — e.g. `max(min_fee, bps_fee)`.

Exit cost models are stored on each live position at entry and charged when that position closes—whether via `TradeDuration`, `ExitTrade`, `ExitAllPositions`, or mean-reversion exit.

---

## 5. Mean-Reversion Empirical Caveat (Unbiased Trading Video Takeaway)

Backtesting Goldman's default `MeanReversion` rule (`30`-day window, `z_score_bound = 2.0`, hold until price crosses back through rolling mean) frequently shows a **65%–70% closed-trade win rate** alongside muted annualized returns or large drawdowns. Because the exit waits indefinitely for mean reversion, **trades that move against the position stay open the longest and dominate average losses**. Always inspect `SummaryStats` (`max_drawdown`, `max_drawdown_duration_days`, `sharpe_ratio`, `avg_winning_hold_days`, and `avg_losing_hold_days`) rather than relying on win rate alone.
