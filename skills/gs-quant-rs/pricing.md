# Pricing & Valuation Contexts (`gs-quant-core`)

## Local Analytical Pricer vs Remote Marquee API

`LocalPricer` implements offline Pure Rust analytical valuation behind the `Pricer` trait:
- **FX Options (`FxOption`)**: Garman-Kohlhagen closed form (`spot`, `vol`, domestic/quote risk-free rate `r_d`, foreign/base risk-free rate `r_f`).
- **Equity Options (`EqOption`)**: Black-Scholes-Merton with continuous dividend yield `q` and optional `EQ VOL Scale`.
- **FX Forwards (`FxForward`)**: Discounted outright mark-to-market `notional * (S * exp(-r_f T) - K * exp(-r_d T))`.
- **Interest Rate Swaps (`IrSwap`)**: Semi-annual annuity mark-to-market against the snapshot's par curve point.
- **Spot (`SpotEquity`)**: `quantity * spot`.

Custom pricing backends (e.g. Monte Carlo, binomial lattice, or HTTP gateway adapters) only need to implement `gs_quant_core::Pricer`.

---

## Relative Terms and Entry Resolution

Per `gs_quant` semantics:
1. Constructing an instrument with `StrikeSpec::AtmSpot` or `ExpirySpec::Relative("3m")` keeps those terms floating until `resolve(trade_date, &snapshot, &holidays)` is called.
2. Calling `resolve(...)` on entry fixes the strike (`StrikeSpec::Absolute`), fixed expiry date (`ExpirySpec::FixedDate`), par swap rate, or outright forward rate for the rest of the trade's holding period.
3. Pricing a resolved trade across a date sequence via `HistoricalPricer::run` tracks genuine mark-to-market P&L as spot, implied vol, and time-to-expiry evolve.

---

## Direct Historical Pricing (`HistoricalPricer`)

When there are no triggers or dynamic position changes, skip `GenericEngine` and call `HistoricalPricer::run(&portfolio, &dates, &history, &RiskMeasure::DollarPrice, &pricer)`.
