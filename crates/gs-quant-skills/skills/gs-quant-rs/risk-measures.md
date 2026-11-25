# Risk Measures (`gs-quant-core`)

## Scale & Unit Conventions

- **Rates, implied vols, and forward rates are decimals**: `0.0466` means `4.66%`, `0.08` means `8%` vol. (By contrast, `realized_volatility` returns annualized percent where `20.0` means `20%`, matching `gs_quant.timeseries.volatility`.)
- **PVs and Greeks scale linearly with trade size** (`notional` or `quantity * multiplier`); spot and vol levels (`FxSpot`, `FxImpliedVol`, `EqSpot`, `EqImpliedVol`, `IrFwdRate`) do not.

---

## FX Measures

| Measure | Meaning | Approximate Taylor Contribution |
|---|---|---|
| `FxDelta` | USD PV change per `+1.0` move in spot | `FxDelta * dSpot` |
| `FxGamma` | `d(FxDelta) / dSpot` in USD per spot^2 | `0.5 * FxGamma * dSpot^2` |
| `FxVega` | USD PV change per `+0.01` (`+1` vol pt) rise in implied vol | `100.0 * FxVega * dVol` |
| `FxTheta` | 1-calendar-day time decay in USD | `FxTheta * dDays` |
| `FxSpot` | Market FX spot quote | — |
| `FxImpliedVol` | Market ATM implied vol (decimal) | — |

---

## Equity Measures

| Measure | Meaning | Approximate Taylor Contribution |
|---|---|---|
| `EqDelta` | USD PV change for a **100% move** in spot (`S * dPV/dS`); divide by `100` for a 1% move | `EqDelta * (dS / S)` |
| `EqGamma` | Second-order equity convexity (`0.01 * S^2 * d2PV/dS2`) | `50.0 * EqGamma * (dS / S)^2` |
| `EqVega` | USD PV change per `+0.01` (`+1` vol pt) rise in implied vol | `100.0 * EqVega * dVol` |
| `EqTheta` | 1-calendar-day time decay in USD | `EqTheta * dDays` |

---

## Rates Measures

| Measure | Meaning |
|---|---|
| `IrDeltaParallel` | USD PV change for a `+1bp` (`+0.0001`) parallel shift in swap par rate |
| `IrFwdRate` | Market par swap rate (decimal) |

> **Volga / Vanna**: There is no built-in Volga or Vanna enum variant. Compute `FxVega` and `FxDelta` under a `MarketDataShockBasedScenario` bumping implied vol and subtract the unshocked baseline.
