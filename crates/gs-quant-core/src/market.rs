//! Market data snapshots, historical stores, and CSV-backed series.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::QuantError;

/// Canonical five-part identifier matching `gs_quant`'s market data point naming:
/// `<mkt_type>_<mkt_asset>_<mkt_class>_<mkt_point>.<mkt_quoting_style>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MarketDataKey {
    /// Asset class or curve group (e.g. `"FX"`, `"FX VOL"`, `"EQ"`, `"EQ VOL"`, `"IR"`).
    pub mkt_type: String,
    /// Market asset identifier (e.g. `"USD/EUR"`, `"EURUSD"`, `".SPX"`, `"USD"`).
    pub mkt_asset: String,
    /// Sub-class (e.g. `"SPOT"`, `"ATM VOL"`, `"SWAP"`, `"Scale"`, `"DIV"`).
    pub mkt_class: String,
    /// Curve/surface tenor point (e.g. `"SPOT"`, `"3M"`, `"10Y"`).
    pub mkt_point: String,
    /// Quoting convention (e.g. `"Mid"`, `"ImpliedVol"`, `"Rate"`, `"Value"`).
    pub mkt_quoting_style: String,
}

impl MarketDataKey {
    /// Constructs a new [`MarketDataKey`].
    #[must_use]
    pub fn new(
        mkt_type: impl Into<String>,
        mkt_asset: impl Into<String>,
        mkt_class: impl Into<String>,
        mkt_point: impl Into<String>,
        mkt_quoting_style: impl Into<String>,
    ) -> Self {
        Self {
            mkt_type: mkt_type.into(),
            mkt_asset: mkt_asset.into(),
            mkt_class: mkt_class.into(),
            mkt_point: mkt_point.into(),
            mkt_quoting_style: mkt_quoting_style.into(),
        }
    }

    /// Key for an FX spot quote (e.g. pair `"EURUSD"`).
    #[must_use]
    pub fn fx_spot(pair: &str) -> Self {
        Self::new("FX", pair.to_ascii_uppercase(), "SPOT", "SPOT", "Mid")
    }

    /// Key for an FX annualized implied volatility quote (decimal).
    #[must_use]
    pub fn fx_vol(pair: &str, point: &str) -> Self {
        Self::new(
            "FX VOL",
            pair.to_ascii_uppercase(),
            "ATM VOL",
            point,
            "ImpliedVol",
        )
    }

    /// Key for an equity spot price.
    #[must_use]
    pub fn eq_spot(symbol: &str) -> Self {
        Self::new("EQ", symbol, "SPOT", "SPOT", "Mid")
    }

    /// Key for an equity implied volatility (decimal).
    #[must_use]
    pub fn eq_vol(symbol: &str, point: &str) -> Self {
        Self::new("EQ VOL", symbol, "ATM VOL", point, "ImpliedVol")
    }

    /// Key for an equity vol scale multiplier (default `1.0` per `scenarios.md`).
    #[must_use]
    pub fn eq_vol_scale(symbol: &str) -> Self {
        Self::new("EQ VOL", symbol, "Scale", "ALL", "Value")
    }

    /// Key for an interest rate curve point (e.g. currency `"USD"`, point `"10Y"`).
    #[must_use]
    pub fn ir_rate(currency: &str, point: &str) -> Self {
        Self::new(
            "IR",
            currency.to_ascii_uppercase(),
            "SWAP",
            point.to_ascii_uppercase(),
            "Rate",
        )
    }

    /// Key for a continuously compounded risk-free rate used for discounting/drift.
    #[must_use]
    pub fn risk_free_rate(currency: &str) -> Self {
        Self::new("IR", currency.to_ascii_uppercase(), "CASH", "OIS", "Rate")
    }

    /// Key for an equity continuous dividend yield.
    #[must_use]
    pub fn eq_dividend_yield(symbol: &str) -> Self {
        Self::new("EQ", symbol, "DIV", "YIELD", "Rate")
    }
}

/// Complete market data state for a single pricing date.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketSnapshot {
    /// Valuation date associated with this snapshot.
    pub date: NaiveDate,
    /// All market quotes keyed by [`MarketDataKey`].
    pub quotes: BTreeMap<MarketDataKey, f64>,
}

impl MarketSnapshot {
    /// Creates an empty [`MarketSnapshot`] for `date`.
    #[must_use]
    pub fn new(date: NaiveDate) -> Self {
        Self {
            date,
            quotes: BTreeMap::new(),
        }
    }

    /// Inserts or updates a market data quote.
    pub fn insert(&mut self, key: MarketDataKey, value: f64) {
        self.quotes.insert(key, value);
    }

    /// Builder-style quote insertion.
    #[must_use]
    pub fn with_quote(mut self, key: MarketDataKey, value: f64) -> Self {
        self.quotes.insert(key, value);
        self
    }

    /// Sets the FX spot rate for `pair` (e.g. `"EURUSD"`).
    #[must_use]
    pub fn with_fx_spot(self, pair: &str, spot: f64) -> Self {
        self.with_quote(MarketDataKey::fx_spot(pair), spot)
    }

    /// Sets the FX implied vol for `pair` and `point` (e.g. `"3M"` or `"ATM"`).
    #[must_use]
    pub fn with_fx_vol(self, pair: &str, point: &str, vol: f64) -> Self {
        self.with_quote(MarketDataKey::fx_vol(pair, point), vol)
    }

    /// Sets the equity spot price for `symbol`.
    #[must_use]
    pub fn with_eq_spot(self, symbol: &str, spot: f64) -> Self {
        self.with_quote(MarketDataKey::eq_spot(symbol), spot)
    }

    /// Sets the equity implied vol for `symbol` and `point`.
    #[must_use]
    pub fn with_eq_vol(self, symbol: &str, point: &str, vol: f64) -> Self {
        self.with_quote(MarketDataKey::eq_vol(symbol, point), vol)
    }

    /// Sets an interest rate swap curve point (e.g. `"USD"`, `"10Y"`, `0.04`).
    #[must_use]
    pub fn with_ir_rate(self, currency: &str, point: &str, rate: f64) -> Self {
        self.with_quote(MarketDataKey::ir_rate(currency, point), rate)
    }

    /// Sets the continuous risk-free rate for `currency`.
    #[must_use]
    pub fn with_risk_free_rate(self, currency: &str, rate: f64) -> Self {
        self.with_quote(MarketDataKey::risk_free_rate(currency), rate)
    }

    /// Reads an exact key from this snapshot.
    #[must_use]
    pub fn get(&self, key: &MarketDataKey) -> Option<f64> {
        self.quotes.get(key).copied()
    }

    /// Looks up FX spot for `pair`, supporting inverse pair lookup (`USDEUR` -> `1 / EURUSD`) if needed.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError::MissingMarketData`] if neither direct nor inverse spot is present.
    pub fn fx_spot(&self, pair: &str) -> Result<f64, QuantError> {
        let upper = pair.to_ascii_uppercase();
        if let Some(val) = self.get(&MarketDataKey::fx_spot(&upper)) {
            return Ok(val);
        }
        if upper.len() == 6 {
            let inverse = format!("{}{}", &upper[3..6], &upper[0..3]);
            if let Some(val) = self.get(&MarketDataKey::fx_spot(&inverse)) {
                if val.abs() > f64::EPSILON {
                    return Ok(1.0 / val);
                }
            }
        }
        Err(QuantError::missing_market_data(
            self.date,
            format!("FX spot for {pair}"),
        ))
    }

    /// Looks up FX implied vol for `pair`, checking `point` then `"ATM"` fallback.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError::MissingMarketData`] if no matching FX vol is present.
    pub fn fx_vol(&self, pair: &str, point: &str) -> Result<f64, QuantError> {
        let upper = pair.to_ascii_uppercase();
        if let Some(val) = self.get(&MarketDataKey::fx_vol(&upper, point)) {
            return Ok(val);
        }
        if let Some(val) = self.get(&MarketDataKey::fx_vol(&upper, "ATM")) {
            return Ok(val);
        }
        if let Some((_, &val)) = self
            .quotes
            .iter()
            .find(|(k, _)| k.mkt_type == "FX VOL" && k.mkt_asset.eq_ignore_ascii_case(&upper))
        {
            return Ok(val);
        }
        Err(QuantError::missing_market_data(
            self.date,
            format!("FX vol for {pair}"),
        ))
    }

    /// Looks up equity spot price for `symbol`.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError::MissingMarketData`] if not found.
    pub fn eq_spot(&self, symbol: &str) -> Result<f64, QuantError> {
        self.get(&MarketDataKey::eq_spot(symbol)).ok_or_else(|| {
            QuantError::missing_market_data(self.date, format!("EQ spot for {symbol}"))
        })
    }

    /// Looks up effective equity implied volatility, applying any `EQ VOL Scale` multiplier.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError::MissingMarketData`] if no matching equity vol is present.
    pub fn eq_vol(&self, symbol: &str, point: &str) -> Result<f64, QuantError> {
        let scale = self
            .get(&MarketDataKey::eq_vol_scale(symbol))
            .unwrap_or(1.0);
        if let Some(val) = self.get(&MarketDataKey::eq_vol(symbol, point)) {
            return Ok(val * scale);
        }
        if let Some(val) = self.get(&MarketDataKey::eq_vol(symbol, "ATM")) {
            return Ok(val * scale);
        }
        if let Some((_, &val)) = self.quotes.iter().find(|(k, _)| {
            k.mkt_type == "EQ VOL" && k.mkt_asset == symbol && k.mkt_class != "Scale"
        }) {
            return Ok(val * scale);
        }
        Err(QuantError::missing_market_data(
            self.date,
            format!("EQ vol for {symbol}"),
        ))
    }

    /// Looks up the continuous risk-free rate for `currency`, defaulting to `0.0` if omitted.
    #[must_use]
    pub fn risk_free_rate_or_zero(&self, currency: &str) -> f64 {
        self.get(&MarketDataKey::risk_free_rate(currency))
            .unwrap_or(0.0)
    }

    /// Looks up equity dividend yield for `symbol`, defaulting to `0.0` if omitted.
    #[must_use]
    pub fn eq_dividend_yield_or_zero(&self, symbol: &str) -> f64 {
        self.get(&MarketDataKey::eq_dividend_yield(symbol))
            .unwrap_or(0.0)
    }

    /// Looks up an interest rate swap par rate for `currency` and `point`, falling back to any IR rate for `currency`.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError::MissingMarketData`] if no rate is available.
    pub fn ir_rate(&self, currency: &str, point: &str) -> Result<f64, QuantError> {
        let upper_ccy = currency.to_ascii_uppercase();
        if let Some(val) = self.get(&MarketDataKey::ir_rate(&upper_ccy, point)) {
            return Ok(val);
        }
        if let Some((_, &val)) = self
            .quotes
            .iter()
            .find(|(k, _)| k.mkt_type == "IR" && k.mkt_asset.eq_ignore_ascii_case(&upper_ccy))
        {
            return Ok(val);
        }
        Err(QuantError::missing_market_data(
            self.date,
            format!("IR rate for {currency} {point}"),
        ))
    }
}

/// Strategy for handling missing observations in [`DataSeries`] lookups,
/// mirroring `gs_quant.backtests.data_sources.MissingDataStrategy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MissingDataStrategy {
    /// Return an error when the exact date is missing.
    Fail,
    /// Carry forward the most recent observation on or before the requested date.
    #[default]
    FillForward,
    /// Ignore missing dates during trigger evaluation (`Skip`).
    Skip,
}

/// Ordered daily time series (prices, FX spots, vols, or macro indicators) plus named series lookups.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DataSeries {
    points: BTreeMap<NaiveDate, f64>,
}

impl DataSeries {
    /// Creates an empty [`DataSeries`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            points: BTreeMap::new(),
        }
    }

    /// Builds a [`DataSeries`] from `(NaiveDate, f64)` pairs.
    #[must_use]
    pub fn from_points(iter: impl IntoIterator<Item = (NaiveDate, f64)>) -> Self {
        Self {
            points: iter.into_iter().collect(),
        }
    }

    /// Inserts or overwrites an observation for `date`.
    pub fn insert(&mut self, date: NaiveDate, value: f64) {
        self.points.insert(date, value);
    }

    /// Number of observations in the series.
    #[must_use]
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Returns `true` if the series contains no observations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Returns all `(NaiveDate, f64)` entries in chronological order.
    pub fn iter(&self) -> impl Iterator<Item = (NaiveDate, f64)> + '_ {
        self.points.iter().map(|(&d, &v)| (d, v))
    }

    /// Returns the dates present in this series in chronological order.
    #[must_use]
    pub fn dates(&self) -> Vec<NaiveDate> {
        self.points.keys().copied().collect()
    }

    /// Returns the observation on `date` according to `strategy`.
    #[must_use]
    pub fn get_with_strategy(&self, date: NaiveDate, strategy: MissingDataStrategy) -> Option<f64> {
        match strategy {
            MissingDataStrategy::Fail | MissingDataStrategy::Skip => {
                self.points.get(&date).copied()
            }
            MissingDataStrategy::FillForward => {
                self.points.range(..=date).next_back().map(|(_, &v)| v)
            }
        }
    }

    /// Returns the trailing `window` observations on or before `date` in chronological order.
    #[must_use]
    pub fn trailing_window(&self, date: NaiveDate, window: usize) -> Vec<f64> {
        if window == 0 {
            return Vec::new();
        }
        let mut tail: Vec<f64> = self
            .points
            .range(..=date)
            .rev()
            .take(window)
            .map(|(_, &v)| v)
            .collect();
        tail.reverse();
        tail
    }

    /// Loads a date-indexed series from a CSV file with configurable date and numeric column names.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError::Io`] or [`QuantError::InvalidCsv`] if the file cannot be read,
    /// the columns are missing, or values fail to parse as `YYYY-MM-DD` / `f64`.
    pub fn load_csv(
        path: impl AsRef<Path>,
        date_col: &str,
        value_col: &str,
    ) -> Result<Self, QuantError> {
        let path_ref = path.as_ref();
        let mut reader =
            csv::Reader::from_path(path_ref).map_err(|err| QuantError::InvalidCsv {
                path: path_ref.to_path_buf(),
                message: err.to_string(),
            })?;

        let headers = reader
            .headers()
            .map_err(|err| QuantError::InvalidCsv {
                path: path_ref.to_path_buf(),
                message: err.to_string(),
            })?
            .clone();

        let date_idx = headers
            .iter()
            .position(|h| h.trim() == date_col)
            .ok_or_else(|| QuantError::InvalidCsv {
                path: path_ref.to_path_buf(),
                message: format!("missing date column `{date_col}`"),
            })?;

        let val_idx = headers
            .iter()
            .position(|h| h.trim() == value_col)
            .ok_or_else(|| QuantError::InvalidCsv {
                path: path_ref.to_path_buf(),
                message: format!("missing value column `{value_col}`"),
            })?;

        let mut points = BTreeMap::new();
        for (row_num, rec_res) in reader.records().enumerate() {
            let rec = rec_res.map_err(|err| QuantError::InvalidCsv {
                path: path_ref.to_path_buf(),
                message: format!("row {}: {err}", row_num + 1),
            })?;
            let raw_date = rec.get(date_idx).unwrap_or("").trim();
            let raw_val = rec.get(val_idx).unwrap_or("").trim();
            let date = NaiveDate::parse_from_str(raw_date, "%Y-%m-%d").map_err(|err| {
                QuantError::InvalidCsv {
                    path: path_ref.to_path_buf(),
                    message: format!("row {} invalid date `{raw_date}`: {err}", row_num + 1),
                }
            })?;
            let val: f64 = raw_val.parse().map_err(|err| QuantError::InvalidCsv {
                path: path_ref.to_path_buf(),
                message: format!("row {} invalid number `{raw_val}`: {err}", row_num + 1),
            })?;
            points.insert(date, val);
        }

        if points.is_empty() {
            return Err(QuantError::empty_series(format!(
                "no rows found in `{}`",
                path_ref.display()
            )));
        }

        Ok(Self { points })
    }
}

/// Multi-date market history storing snapshots and auxiliary named [`DataSeries`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MarketHistory {
    snapshots: BTreeMap<NaiveDate, MarketSnapshot>,
    series: BTreeMap<String, DataSeries>,
}

impl MarketHistory {
    /// Creates an empty [`MarketHistory`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a daily [`MarketSnapshot`].
    pub fn insert_snapshot(&mut self, snapshot: MarketSnapshot) {
        self.snapshots.insert(snapshot.date, snapshot);
    }

    /// Registers a named [`DataSeries`] (used by `MktTrigger` and `MeanReversionTrigger`).
    pub fn insert_series(&mut self, name: impl Into<String>, series: DataSeries) {
        self.series.insert(name.into(), series);
    }

    /// Returns a named [`DataSeries`] if present.
    #[must_use]
    pub fn series(&self, name: &str) -> Option<&DataSeries> {
        self.series.get(name)
    }

    /// Returns the snapshot for `date`, falling back to the most recent prior snapshot if `strategy`
    /// is [`MissingDataStrategy::FillForward`].
    ///
    /// # Errors
    ///
    /// Returns [`QuantError::MissingMarketData`] if no matching snapshot exists.
    pub fn snapshot(
        &self,
        date: NaiveDate,
        strategy: MissingDataStrategy,
    ) -> Result<MarketSnapshot, QuantError> {
        if let Some(snap) = self.snapshots.get(&date) {
            return Ok(snap.clone());
        }
        if strategy == MissingDataStrategy::FillForward {
            if let Some((_, snap)) = self.snapshots.range(..=date).next_back() {
                let mut filled = snap.clone();
                filled.date = date;
                return Ok(filled);
            }
        }
        Err(QuantError::missing_market_data(
            date,
            "market snapshot for date",
        ))
    }

    /// Returns all snapshot dates in chronological order.
    #[must_use]
    pub fn dates(&self) -> Vec<NaiveDate> {
        self.snapshots.keys().copied().collect()
    }

    /// Convenience loader that builds an FX spot [`MarketHistory`] (plus optional implied vol and
    /// named trigger series) directly from a CSV file.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError::InvalidCsv`] or [`QuantError::Io`] on read/parse failure.
    pub fn from_fx_csv(
        path: impl AsRef<Path>,
        pair: &str,
        date_col: &str,
        close_col: &str,
        vol_col: Option<&str>,
    ) -> Result<Self, QuantError> {
        let close_series = DataSeries::load_csv(&path, date_col, close_col)?;
        let vol_series = match vol_col {
            Some(col) => Some(DataSeries::load_csv(&path, date_col, col)?),
            None => None,
        };

        let mut history = Self::new();
        for (date, spot) in close_series.iter() {
            let mut snap = MarketSnapshot::new(date)
                .with_fx_spot(pair, spot)
                .with_risk_free_rate("USD", 0.0)
                .with_risk_free_rate(&pair[0..3.min(pair.len())], 0.0);
            if let Some(ref vs) = vol_series {
                if let Some(iv) = vs.get_with_strategy(date, MissingDataStrategy::FillForward) {
                    snap = snap.with_fx_vol(pair, "ATM", iv);
                }
            } else {
                snap = snap.with_fx_vol(pair, "ATM", 0.08);
            }
            history.insert_snapshot(snap);
        }
        history.insert_series(pair.to_ascii_uppercase(), close_series);
        Ok(history)
    }
}
