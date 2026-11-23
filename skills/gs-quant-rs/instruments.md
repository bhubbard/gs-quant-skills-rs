# Constructing Instruments (`gs-quant-core`)

Every product is represented by a variant of [`Instrument`](gs_quant_core::Instrument).

## Strikes & Expiries

- `StrikeSpec::Absolute(level)`: explicit numeric strike (`0.04` for a 4% rate, `1.085` for EURUSD).
- `StrikeSpec::AtmSpot`: spot rate at trade entry (`'ATMS'` / `'s'`).
- `StrikeSpec::AtmForward`: forward rate at trade entry (`'ATMF'` / `'f'`).
- `StrikeSpec::PctOfSpot(pct)`: e.g. `StrikeSpec::PctOfSpot(0.95)` (`'95%'`).
- `ExpirySpec::FixedDate(date)` or `ExpirySpec::Relative(tenor)` (`"3m"`, `"1y"`, parsed via `ExpirySpec::parse`).

---

## Instrument Constructors

### `FxOption` (Defaults `premium = 0.0`)

`Instrument::fx_option` sets `premium = 0.0` automatically (matching `gs_quant`'s `instruments.md` rule so live MTM equals option fair value rather than net of an unpaid fixed premium).

```rust
let call = Instrument::fx_option(
    "EURUSD 3m ATMF Call",
    "EURUSD",
    100_000.0,
    OptionType::Call,
    StrikeSpec::AtmForward,
    ExpirySpec::parse("3m")?,
);
```

### `FxForward`

`forward_rate` starts as `None` and resolves to the covered-interest-parity forward rate on the entry date:

```rust
let fwd = Instrument::fx_forward("EURUSD 1m Fwd", "EURUSD", 100_000.0, ExpirySpec::parse("1m")?);
```

### `EqOption`

```rust
let spx_call = Instrument::eq_option(
    "SPX 3m Call",
    ".SPX",
    10.0,
    100.0,
    OptionType::Call,
    StrikeSpec::AtmSpot,
    ExpirySpec::parse("3m")?,
);
```

### `SpotEquity`

Linear spot position for equities, ETFs, or FX pairs (`quantity` units):

```rust
let eur_spot = Instrument::spot("EURUSD Spot", "EURUSD", 100_000.0);
```

### `IrSwap`

```rust
let swap = Instrument::ir_swap(
    "USD 10y Payer",
    "USD",
    PayReceive::Pay,
    10_000_000.0,
    ExpirySpec::parse("10y")?,
);
```

## Resolving Relative Terms

Call `instrument.resolve(trade_date, &snapshot, &holidays)?` to freeze relative strikes, relative expiries, forward rates, and par swap rates on entry. `GenericEngine` does this automatically for every trade added by an action.
