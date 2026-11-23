//! Error types returned by [`gs_quant_core`](crate).

use std::io;
use std::path::PathBuf;

use chrono::NaiveDate;
use thiserror::Error;

/// Error returned by offline pricing, scenario, volatility, and backtest operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum QuantError {
    /// An input argument violated domain constraints.
    #[error("invalid parameter: {message}")]
    InvalidParameter {
        /// Human-readable description of the invalid argument.
        message: String,
    },

    /// Required market data was absent on the valuation date.
    #[error("missing market data on {date}: {key}")]
    MissingMarketData {
        /// Valuation date where lookup failed.
        date: NaiveDate,
        /// Market data identifier or series name that could not be found.
        key: String,
    },

    /// Tenor string could not be parsed.
    #[error("invalid tenor `{input}`: expected <positive-integer>(b|w|m|y)")]
    InvalidTenor {
        /// Original tenor string.
        input: String,
    },

    /// CSV parsing or column lookup failed.
    #[error("invalid csv `{path}`: {message}")]
    InvalidCsv {
        /// File path of the CSV being loaded.
        path: PathBuf,
        /// Cause description.
        message: String,
    },

    /// Underlying filesystem I/O failed.
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    /// A rolling or summary calculation received an empty or insufficiently long series.
    #[error("empty or insufficient series: {message}")]
    EmptySeries {
        /// Description of the missing observations.
        message: String,
    },

    /// Risk-based trade scaling encountered a zero unit risk sensitivity.
    #[error("zero risk sensitivity for {measure} on {date}")]
    ZeroRiskSensitivity {
        /// Risk measure used for scaling.
        measure: String,
        /// Trade date when scaling was attempted.
        date: NaiveDate,
    },

    /// A scenario or roll-forward operation could not proceed.
    #[error("invalid scenario: {message}")]
    InvalidScenario {
        /// Description of the scenario error.
        message: String,
    },
}

impl QuantError {
    pub(crate) fn invalid_parameter(message: impl Into<String>) -> Self {
        Self::InvalidParameter {
            message: message.into(),
        }
    }

    pub(crate) fn missing_market_data(date: NaiveDate, key: impl Into<String>) -> Self {
        Self::MissingMarketData {
            date,
            key: key.into(),
        }
    }

    pub(crate) fn empty_series(message: impl Into<String>) -> Self {
        Self::EmptySeries {
            message: message.into(),
        }
    }
}
