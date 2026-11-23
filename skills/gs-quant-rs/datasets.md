# Market Data & Realized Volatility (`gs-quant-core`)

## Loading CSV Data

`DataSeries::load_csv(path, date_col, value_col)` parses `YYYY-MM-DD` date rows and `f64` values into a sorted `BTreeMap<NaiveDate, f64>`:

```rust
use gs_quant_core::{DataSeries, MarketHistory, MissingDataStrategy};

let series = DataSeries::load_csv("data/sample_eurusd.csv", "date", "close")?;
let history = MarketHistory::from_fx_csv(
    "data/sample_eurusd.csv",
    "EURUSD",
    "date",
    "close",
    Some("implied_vol"),
)?;
```

### Missing Data Strategies (`MissingDataStrategy`)

- `FillForward` (default): carries forward the latest observation on or before `date`.
- `Fail`: returns `QuantError::MissingMarketData` if `date` has no exact quote.
- `Skip`: leaves missing dates unfired in `Market` triggers.

---

## Realized Volatility (`realized_volatility`)

Matches `gs_quant.timeseries.econometrics.volatility`, returning annualized percent (`20.0` = `20%`):

```rust
use gs_quant_core::{ReturnsType, VolatilityMean, realized_volatility};

// Sample mean with N - 1 denominator:
let sample_vol = realized_volatility(&series, 22, ReturnsType::Simple, 252, VolatilityMean::SampleMean)?;

// Zero-mean RMS with N denominator (variance/correlation swap convention):
let rms_vol = realized_volatility(&series, 22, ReturnsType::Logarithmic, 252, VolatilityMean::ZeroMean)?;
```

- `ReturnsType`: `Simple` (`P_t / P_{t-1} - 1`), `Logarithmic` (`ln(P_t / P_{t-1})`), `Absolute` (`P_t - P_{t-1}`).
- `VolatilityMean`: `SampleMean` (`N - 1` denominator) vs `ZeroMean` (`N` denominator, RMS around zero).
