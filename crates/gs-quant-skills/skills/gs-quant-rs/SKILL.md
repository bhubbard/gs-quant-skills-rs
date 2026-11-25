---
name: gs-quant-rs
description: "Guide to writing Pure Rust quantitative finance, local pricing, scenario shocks, realized volatility, and stateful backtesting code using gs-quant-core and gs-quant-cli without Python or Marquee API credentials."
---

# gs-quant-rs Quick Start

`gs-quant-skills-rs` is a Pure Rust fork of Goldman Sachs `gs_quant/skills` paired with an offline pricing, scenario, volatility, and backtesting engine (`gs-quant-core`). Unlike Python `gs_quant`, **no Marquee login or Goldman backend is required** — you supply your own CSV or in-memory market data.

Before writing pricing, risk, or backtesting code, open `pricing.md` and `backtesting.md`.

---

## 1. Loading Market Data (Zero Goldman Login)

All valuation and backtests run locally against [`MarketSnapshot`] and [`MarketHistory`]:

```rust
use gs_quant_core::{MarketHistory, MarketSnapshot};
use chrono::NaiveDate;

// Load daily FX spot + optional implied vol from CSV:
let history = MarketHistory::from_fx_csv(
    "data/sample_eurusd.csv",
    "EURUSD",
    "date",
    "close",
    Some("implied_vol"),
)?;

// Or build an explicit single-date snapshot in memory:
let date = NaiveDate::from_ymd_opt(2025, 1, 15).expect("valid date");
let snap = MarketSnapshot::new(date)
    .with_fx_spot("EURUSD", 1.0850)
    .with_fx_vol("EURUSD", "ATM", 0.082)
    .with_risk_free_rate("USD", 0.045)
    .with_risk_free_rate("EUR", 0.028);
```

---

## 2. Constructing & Resolving Instruments

Relative terms (`StrikeSpec::AtmSpot`, `StrikeSpec::AtmForward`, `ExpirySpec::Relative("3m")`, `IrSwap { fixed_rate: None }`) must be resolved against the trade-date snapshot before holding over time:

```rust
use gs_quant_core::{ExpirySpec, Instrument, LocalPricer, OptionType, Pricer, RiskMeasure, StrikeSpec};
use std::collections::BTreeSet;

let opt = Instrument::fx_option(
    "EURUSD 3m Call",
    "EURUSD",
    1_000_000.0,
    OptionType::Call,
    StrikeSpec::AtmForward,
    ExpirySpec::parse("3m")?,
);

let resolved = opt.resolve(date, &snap, &BTreeSet::new())?;
let pricer = LocalPricer::new();
let pv = pricer.calc(&resolved, &snap, &RiskMeasure::DollarPrice)?;
let delta = pricer.calc(&resolved, &snap, &RiskMeasure::FxDelta)?;
```

---

## 3. Combining Instruments in a `Portfolio`

```rust
use gs_quant_core::{HistoricalPricer, Portfolio, RiskMeasure};

let portfolio = Portfolio::new("FX Book", vec![resolved]);
let total_pv = portfolio.calc_aggregate(&snap, &RiskMeasure::DollarPrice, &pricer)?;

// Static buy-and-hold across dates does NOT need GenericEngine:
let curve = HistoricalPricer::run(&portfolio, &dates, &history, &RiskMeasure::DollarPrice, &pricer)?;
```

---

## 4. Stateful Backtesting (`GenericEngine`)

Use `GenericEngine` whenever positions change over time via triggers, position caps, risk hedges, or exit rules:

- **Triggers**: `Periodic`, `Dates`, `Market`, `StrategyRisk`, `MeanReversion`, `OpenPositionCount`, `Aggregate` (`AllOf` / `AnyOf`), `Not`.
- **Actions**: `AddTrade`, `AddScaledTrade` (`Size`, `RiskMeasure`, `Nav`), `Hedge`, `ExitTrade`, `ExitAllPositions`.
- **Transaction Costs**: Separate `entry_cost` and `exit_cost` per action (`Zero`, `Constant`, `Scaled`, `Aggregate { Sum | Max | Min }`).

---

## 5. CLI Quick Commands

```sh
gs-quant-skills list
gs-quant-skills install --global
gs-quant-skills backtest mean-reversion --csv data/sample_eurusd.csv
gs-quant-skills volatility --csv data/sample_eurusd.csv --window 22 --zero-mean
gs-quant-skills scenario --spot 1.10 --strike 1.10 --vol 0.08 --expiry-years 0.25 --spot-shock-pct 0.02
```

---

## Topic Guides

- `backtesting.md` — `HistoricalPricer` vs `GenericEngine`, triggers, risk bands, position caps, risk-scaled sizing, entry/exit transaction costs, 19 summary stats, P&L Explain, and empirical mean-reversion caveats.
- `instruments.md` — Constructing and resolving `SpotEquity`, `FxForward`, `FxOption` (`premium: 0.0`), `EqOption`, and `IrSwap`.
- `pricing.md` — `LocalPricer`, `Pricer` trait, `Portfolio`, `HistoricalPricer`, relative term resolution on entry date.
- `scenarios.md` — `MarketDataPattern`, `MarketDataShock`, `MarketDataShockBasedScenario` (`matched_points` safety check), and `RollFwd`.
- `risk-measures.md` — Greek units, scaling conventions (`EqDelta` 100% move vs `FxDelta` 1.0 spot move), and market quote measures.
- `datasets.md` — Loading CSV price/vol feeds, `MissingDataStrategy`, and `realized_volatility` (`SampleMean` N-1 vs `ZeroMean` N).
- `results.md` — `ResultSummaryRow`, `TradeLedgerEntry`, `SummaryStats` (19 metrics + win rate & holding days), and `PnlExplainRow`.
- `errors.md` — `QuantError` variants, unmatched scenario diagnostics, and missing data handling.
