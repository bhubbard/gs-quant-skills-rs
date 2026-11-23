//! Risk measure definitions, units, and typed calculation outputs.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Currency code used for valuation and risk reporting.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Currency {
    /// United States Dollar (`USD`).
    Usd,
    /// Euro (`EUR`).
    Eur,
    /// British Pound (`GBP`).
    Gbp,
    /// Japanese Yen (`JPY`).
    Jpy,
    /// Swiss Franc (`CHF`).
    Chf,
    /// Local instrument currency (`currency='local'`).
    Local,
    /// Any other ISO currency code.
    Custom(String),
}

impl Currency {
    /// Returns the uppercase ISO currency label.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Usd => "USD",
            Self::Eur => "EUR",
            Self::Gbp => "GBP",
            Self::Jpy => "JPY",
            Self::Chf => "CHF",
            Self::Local => "LOCAL",
            Self::Custom(code) => code.as_str(),
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Granularity of a risk measure aggregation, mirroring `gs_quant.common.AggregationLevel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AggregationLevel {
    /// Per market data curve point (`Point`, default in `gs_quant`).
    Point,
    /// Per market instrument class (`Class`).
    Class,
    /// Per asset or currency (`Asset`).
    Asset,
    /// Single aggregate scalar (`Type`).
    #[default]
    Type,
}

/// Risk and market-data measures supported by the local analytical pricer and backtest engine.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RiskMeasure {
    /// Present value in the instrument's local settlement currency.
    Price,
    /// Present value converted to USD.
    DollarPrice,
    /// USD PV change for a `+1.0` move in FX spot (`PnL ≈ dSpot * FxDelta`).
    FxDelta,
    /// Second derivative sensitivity for FX options.
    FxGamma,
    /// USD PV change for a `+0.01` (1 vol point) rise in FX implied volatility.
    FxVega,
    /// 1-calendar-day time decay in USD.
    FxTheta,
    /// FX spot rate for the instrument's pair.
    FxSpot,
    /// FX annualized implied volatility as a decimal (`0.08` = 8%).
    FxImpliedVol,
    /// USD PV change for a `+100%` move in equity spot (`gs_quant` convention: divide by 100 for 1% move).
    EqDelta,
    /// Second-order equity sensitivity (`PnL ≈ EqDelta * x + 50 * EqGamma * x^2`).
    EqGamma,
    /// USD PV change for a `+0.01` (1 vol point) rise in equity implied volatility.
    EqVega,
    /// 1-calendar-day equity option time decay in USD.
    EqTheta,
    /// Equity spot price.
    EqSpot,
    /// Equity annualized implied volatility as a decimal.
    EqImpliedVol,
    /// Parallel interest rate PV01 (PV change for `+1bp` = `+0.0001` shift in par rate).
    IrDeltaParallel,
    /// Forward par swap rate as a decimal.
    IrFwdRate,
}

impl RiskMeasure {
    /// Returns the canonical human-readable name used in summary tables and reports.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Price => "Price",
            Self::DollarPrice => "DollarPrice",
            Self::FxDelta => "FXDelta",
            Self::FxGamma => "FXGamma",
            Self::FxVega => "FXVega",
            Self::FxTheta => "FXTheta",
            Self::FxSpot => "FXSpot",
            Self::FxImpliedVol => "FXAnnualImpliedVol",
            Self::EqDelta => "EqDelta",
            Self::EqGamma => "EqGamma",
            Self::EqVega => "EqVega",
            Self::EqTheta => "EqTheta",
            Self::EqSpot => "EqSpot",
            Self::EqImpliedVol => "EqAnnualImpliedVol",
            Self::IrDeltaParallel => "IRDeltaParallel",
            Self::IrFwdRate => "IRFwdRate",
        }
    }

    /// Returns `true` if this measure is a market-data quote level (spot, vol, rate) rather than a PV/Greek.
    ///
    /// Market-data measures do not aggregate by summing across portfolio instruments in `BacktestResult`.
    #[must_use]
    pub fn is_market_quote(&self) -> bool {
        matches!(
            self,
            Self::FxSpot | Self::FxImpliedVol | Self::EqSpot | Self::EqImpliedVol | Self::IrFwdRate
        )
    }
}

impl fmt::Display for RiskMeasure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Scalar risk result carrying its numeric value, measure, and optional unit metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskResult {
    /// Computed measure.
    pub measure: RiskMeasure,
    /// Numeric value.
    pub value: f64,
    /// Optional reporting unit (for example `"USD"` or `"EUR"`).
    pub unit: Option<String>,
}

impl RiskResult {
    /// Creates a new [`RiskResult`].
    #[must_use]
    pub fn new(measure: RiskMeasure, value: f64, unit: Option<String>) -> Self {
        Self {
            measure,
            value,
            unit,
        }
    }
}
