//! Volatility calculation conventions (`SampleMean` N-1 vs `ZeroMean` N RMS).

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::QuantError;
use crate::market::DataSeries;

/// Return calculation convention for realized volatility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ReturnsType {
    /// Simple percentage returns `(P_t / P_{t-1}) - 1`.
    #[default]
    Simple,
    /// Continuously compounded log returns `ln(P_t / P_{t-1})`.
    Logarithmic,
    /// Absolute level differences `P_t - P_{t-1}`.
    Absolute,
}

/// Centering and denominator convention for realized volatility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum VolatilityMean {
    /// Sample standard deviation around the window mean using `N - 1` in the denominator.
    #[default]
    SampleMean,
    /// Root-mean-square around zero (`mean = 0`) using `N` in the denominator
    /// (standard for variance swaps and correlation swaps).
    ZeroMean,
}

/// Computes rolling annualized realized volatility in percent (`20.0` means 20%).
///
/// Mirrors `gs_quant.timeseries.econometrics.volatility`:
/// - `VolatilityMean::SampleMean`: `sqrt((1 / (N - 1)) * sum((r_i - mean)^2)) * sqrt(ann_factor) * 100`
/// - `VolatilityMean::ZeroMean`: `sqrt((1 / N) * sum(r_i^2)) * sqrt(ann_factor) * 100`
///
/// When `returns_type` is [`ReturnsType::Simple`] or [`ReturnsType::Logarithmic`], consecutive price
/// pairs yield `N = window` returns from `window + 1` prices (or fewer only if `window < 2` for `SampleMean`,
/// which is rejected). Here `window` specifies the exact number of returns in each rolling window.
///
/// # Errors
///
/// Returns [`QuantError::InvalidParameter`] if `window == 0`, if `window < 2` under
/// [`VolatilityMean::SampleMean`], or if `annualisation_factor == 0`. Returns [`QuantError::EmptySeries`]
/// if `series` does not contain enough observations to form at least one full window, or
/// [`QuantError::InvalidParameter`] if non-positive prices are encountered for `Simple` / `Logarithmic` returns.
pub fn realized_volatility(
    series: &DataSeries,
    window: usize,
    returns_type: ReturnsType,
    annualisation_factor: u32,
    mean_mode: VolatilityMean,
) -> Result<DataSeries, QuantError> {
    if window == 0 {
        return Err(QuantError::invalid_parameter("window must be positive"));
    }
    if mean_mode == VolatilityMean::SampleMean && window < 2 {
        return Err(QuantError::invalid_parameter(
            "window must be at least 2 for sample-mean volatility",
        ));
    }
    if annualisation_factor == 0 {
        return Err(QuantError::invalid_parameter(
            "annualisation_factor must be positive",
        ));
    }

    let obs: Vec<(NaiveDate, f64)> = series.iter().collect();
    if obs.len() < window + 1 {
        return Err(QuantError::empty_series(format!(
            "need at least {} observations for window {window}, got {}",
            window + 1,
            obs.len()
        )));
    }

    let mut returns: Vec<(NaiveDate, f64)> = Vec::with_capacity(obs.len() - 1);
    for pair in obs.windows(2) {
        let prev = pair[0].1;
        let curr = pair[1].1;
        let date = pair[1].0;
        let ret = match returns_type {
            ReturnsType::Simple => {
                if prev <= 0.0 {
                    return Err(QuantError::invalid_parameter(format!(
                        "non-positive price {prev} on {} for simple returns",
                        pair[0].0
                    )));
                }
                curr / prev - 1.0
            }
            ReturnsType::Logarithmic => {
                if prev <= 0.0 || curr <= 0.0 {
                    return Err(QuantError::invalid_parameter(format!(
                        "non-positive price on {}..={date} for log returns",
                        pair[0].0
                    )));
                }
                (curr / prev).ln()
            }
            ReturnsType::Absolute => curr - prev,
        };
        returns.push((date, ret));
    }

    let scale = f64::from(annualisation_factor).sqrt() * 100.0;
    let mut out = DataSeries::new();

    for slice in returns.windows(window) {
        let end_date = slice[slice.len() - 1].0;
        let vol = match mean_mode {
            VolatilityMean::SampleMean => {
                let n = slice.len() as f64;
                let mean = slice.iter().map(|(_, r)| *r).sum::<f64>() / n;
                let sum_sq = slice.iter().map(|(_, r)| (*r - mean).powi(2)).sum::<f64>();
                (sum_sq / (n - 1.0)).sqrt() * scale
            }
            VolatilityMean::ZeroMean => {
                let n = slice.len() as f64;
                let sum_sq = slice.iter().map(|(_, r)| r.powi(2)).sum::<f64>();
                (sum_sq / n).sqrt() * scale
            }
        };
        out.insert(end_date, vol);
    }

    Ok(out)
}
