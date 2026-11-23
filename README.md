# `gs-quant-skills-rs`

Pure Rust fork of Goldman Sachs [`gs_quant/skills`](https://github.com/goldmansachs/gs-quant/tree/master/gs_quant/skills) paired with an offline Pure Rust quantitative pricing, scenario, realized volatility, and stateful backtesting engine (`gs-quant-core`).

Built with **zero Python** and **zero Marquee API login requirements** — inspired by both Goldman Sachs' open-sourced AI skill architecture and Unbiased Trading's empirical breakdown ([YouTube: *"A $58 Billion Bank Open-Sourced an AI Skill for Backtesting. I Tested It"*](https://www.youtube.com/watch?v=cLUa-qOQS5Q), detailed in [`docs/VIDEO_NOTES.md`](docs/VIDEO_NOTES.md)).

---

## Workspace Crates

| Crate | Purpose |
|---|---|
| [`gs-quant-core`](crates/gs-quant-core) | Offline analytical pricers (`LocalPricer`), `Portfolio` & `HistoricalPricer`, `MarketDataShockBasedScenario` & `RollFwd`, `realized_volatility` (`SampleMean` N-1 vs `ZeroMean` N), separate entry/exit `TransactionCostModel`, stateful `GenericEngine` with triggers/hedges/caps, P&L Explain (`fx_pnl_definition`), and all 19 Goldman `SummaryStats` + closed-trade diagnostics. |
| [`gs-quant-skills`](crates/gs-quant-skills) | Pure Rust skill discovery, compile-time embedded [`skills/gs-quant-rs`](skills/gs-quant-rs) bundle, and symlink/copy installer (`--global`, `--project`, `--antigravity`, `--target-dir`). |
| [`gs-quant-cli`](crates/gs-quant-cli) | `gs-quant-skills` CLI exposing skill management (`list`, `install`, `uninstall`) plus offline `backtest mean-reversion`, `scenario`, and `volatility` subcommands. |

---

## Quick Start

### 1. Build & Test

```sh
cargo test --workspace --all-targets
```

### 2. Skill Installer CLI (`gs-quant-skills`)

Direct Pure Rust replacement for `python -m gs_quant.skills`:

```sh
# List bundled skills and current install status
cargo run -p gs-quant-cli -- list

# Install into ~/.claude/skills (global), ./.claude/skills (project), or ~/.gemini/config/skills (antigravity)
cargo run -p gs-quant-cli -- install --global
cargo run -p gs-quant-cli -- install --project
cargo run -p gs-quant-cli -- install --antigravity
cargo run -p gs-quant-cli -- install --target-dir /tmp/custom-skills --copy

# Uninstall
cargo run -p gs-quant-cli -- uninstall --project
```

When run non-interactively without a scope flag (`--global`, `--project`, `--antigravity`, `--target-dir`), or when given an unknown skill name, `gs-quant-skills` exits with status code `2` just like upstream `gs_quant/skills/__main__.py`.

### 3. Run Offline Quant Commands (No Goldman Login Required)

```sh
# Mean-reversion backtest (30-day rolling mean/std, z-score > 2.0, separate entry/exit fees)
cargo run -p gs-quant-cli -- backtest mean-reversion \
  --csv data/sample_eurusd.csv \
  --z-score 2.0 --mean-window 30 --std-window 30 \
  --entry-fee 25 --exit-fee 15

# Realized volatility (SampleMean N-1 vs ZeroMean N RMS)
cargo run -p gs-quant-cli -- volatility --csv data/sample_eurusd.csv --window 22
cargo run -p gs-quant-cli -- volatility --csv data/sample_eurusd.csv --window 22 --zero-mean

# Option scenario shock (+2% spot, +1 vol point)
cargo run -p gs-quant-cli -- scenario \
  --spot 1.10 --strike 1.10 --vol 0.08 --expiry-years 0.25 \
  --rate 0.045 --foreign-rate 0.03 \
  --spot-shock-pct 0.02 --vol-shock-abs 0.01
```

### 4. Run Examples

```sh
cargo run -p gs-quant-core --example eurusd_mean_reversion
cargo run -p gs-quant-core --example fx_option_delta_hedge
```

---

## Upstream Python `gs_quant/skills` vs `gs-quant-skills-rs`

| Capability | Upstream Python `gs_quant/skills` | `gs-quant-skills-rs` |
|---|---|---|
| Runtime requirement | Python + `gs_quant` package | Single Pure Rust binary (`gs-quant-skills`) with embedded skills |
| Skill installation | `list`, `install`, `uninstall` (`--global` / `--project`) | Exact CLI parity + `--antigravity`, `--target-dir`, `--copy` / `--symlink`, `--json` |
| Market data & pricing | Requires Goldman Marquee API session (`GsSession.use`) | Offline `LocalPricer` (Garman-Kohlhagen, Black-Scholes-Merton, FX fwd, IR swap, spot) + CSV loader |
| Triggers & actions | `Periodic`, `Date`, `Mkt`, `StrategyRisk`, `MeanReversion`, `Portfolio`/`TradeCount`, `Aggregate`, `Not` | Full Rust enum parity (`TriggerCondition`, `Action`, `ScalingType::RiskMeasure`, `Hedge`) |
| Transaction costs | `Constant`, `Scaled`, `Aggregate` (`Sum`/`Max`/`Min`), separate entry/exit | Exact `TransactionCostModel` parity with separate `entry_cost` and `exit_cost` |
| Summary statistics | 3 date fields + 19 `summary_stats()` metrics | All 3 metadata fields + all 19 metrics + closed-trade win rate & winning/losing hold duration |
| Scenarios & Vol | `MarketDataShockBasedScenario`, `RollFwd`, `volatility(..., assume_zero_mean=...)` | Same transformations + `matched_points` count guard against silent pattern mismatches |

---

## Key Takeaways from the Video Analysis

See [`docs/VIDEO_NOTES.md`](docs/VIDEO_NOTES.md) for full notes from Unbiased Trading's test of `gs_quant/skills`:

1. **External Pricing Catch Solved**: Goldman's repo teaches the AI *how* to write `gs_quant`, but refuses pricing calls without Marquee credentials; `gs-quant-core` runs completely locally on user CSVs.
2. **Direct Pricing vs. Engine**: Static buy-and-hold uses `HistoricalPricer`; path-dependent rules (mean reversion, risk-band hedges, position caps) use `GenericEngine`.
3. **19 Metrics & Win-Rate Trap**: Mean reversion often wins 65%–70% of trades while underperforming when losing trades sit open waiting to revert — always examine drawdown, drawdown duration, Sharpe/Sortino, and `avg_losing_hold_days`.
4. **P&L Explain Residual**: Delta + Gamma + Vega attribution omits theta/time decay and higher-order cross terms; `PnlExplainRow::unexplained` exposes the gap explicitly.

---

## License

Licensed under the Apache License, Version 2.0 ([`LICENSE`](LICENSE)). See [`NOTICE`](NOTICE) for Goldman Sachs upstream attribution.
