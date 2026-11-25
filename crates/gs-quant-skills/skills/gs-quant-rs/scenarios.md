# Scenarios & Forward Rolling (`gs-quant-core`)

## Shocking Market Data

`MarketDataShockBasedScenario` maps `MarketDataPattern` filters to `MarketDataShock` transformations:

```rust
use gs_quant_core::{MarketDataPattern, MarketDataShock, MarketDataShockBasedScenario};

let scenario = MarketDataShockBasedScenario::new()
    .with_shock(
        MarketDataPattern::any()
            .with_type("FX")
            .with_asset("EURUSD")
            .with_class("SPOT"),
        MarketDataShock::Proportional(0.02), // +2% spot move
    )
    .with_shock(
        MarketDataPattern::any()
            .with_type("FX VOL")
            .with_asset("EURUSD"),
        MarketDataShock::Absolute(0.01), // +1 vol point (+0.01)
    );

let result = scenario.apply(&snapshot);
assert!(result.matched_points > 0, "pattern matched nothing!");
```

### Shock Conventions

| Variant | Formula | Example |
|---|---|---|
| `MarketDataShock::Absolute(d)` | `value + d` | `0.0001` = +1bp rate, `0.01` = +1 vol pt |
| `MarketDataShock::Proportional(p)` | `value * (1.0 + p)` | `0.06` = +6% relative bump |
| `MarketDataShock::Override(v)` | `v` | `0.104` overrides curve/surface point to 10.4% |

### Guarding Against Silent No-Ops

In Python `gs_quant`, a misspelled asset or inverted pair matches zero points silently. In Rust, `ScenarioResult::matched_points` reports the exact count of transformed quotes so you can assert `result.matched_points > 0` before trusting shocked P&L.

### Equity Vol Scale

Equity vols in `MarketSnapshot` support an optional `EQ VOL` `Scale` multiplier (`1.0` by default). An `Absolute(0.1)` shock on `MarketDataPattern::any().with_type("EQ VOL").with_asset(".SPX").with_class("Scale")` multiplies effective implied volatility by `1.10`.

---

## Rolling Forward (`RollFwd`)

`RollFwd` moves the valuation date forward to simulate time decay and carry:

- `ForwardRealisation::SpotMovesToForward` (Goldman default `realise_fwd=True`): moves FX/equity spot along today's implied forward to `target_date`.
- `ForwardRealisation::KeepSpotFixed` (`realise_fwd=False`): holds spot unchanged while reducing option time-to-expiry.

Always resolve relative-expiry instruments (`ExpirySpec::Relative("3m")`) **before** calling `RollFwd::apply`; otherwise the relative tenor re-anchors from `target_date`.
