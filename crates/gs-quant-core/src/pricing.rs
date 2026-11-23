//! Analytical local pricing, Greeks, portfolios, and static historical valuation.

use std::collections::BTreeSet;
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_2_SQRT_PI, PI};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::QuantError;
use crate::instrument::{
    fx_forward_rate, ExpirySpec, Instrument, OptionType, PayReceive, StrikeSpec,
};
use crate::market::{MarketHistory, MarketSnapshot, MissingDataStrategy};
use crate::risk::{RiskMeasure, RiskResult};

/// Interface for instrument pricing and risk calculation backends.
pub trait Pricer {
    /// Calculates `measure` for `instrument` on `market`'s date.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError`] if required market data is missing or `measure` does not apply
    /// to `instrument`.
    fn calc(
        &self,
        instrument: &Instrument,
        market: &MarketSnapshot,
        measure: &RiskMeasure,
    ) -> Result<RiskResult, QuantError>;
}

/// Pure Rust offline analytical pricer (Black-Scholes-Merton, Garman-Kohlhagen, forwards, swaps).
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalPricer;

impl LocalPricer {
    /// Creates a new [`LocalPricer`].
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Pricer for LocalPricer {
    fn calc(
        &self,
        instrument: &Instrument,
        market: &MarketSnapshot,
        measure: &RiskMeasure,
    ) -> Result<RiskResult, QuantError> {
        match instrument {
            Instrument::SpotEquity {
                symbol, quantity, ..
            } => calc_spot_equity(symbol, *quantity, market, measure),
            Instrument::FxForward {
                pair,
                notional,
                delivery,
                forward_rate,
                ..
            } => calc_fx_forward(pair, *notional, *delivery, *forward_rate, market, measure),
            Instrument::FxOption {
                pair,
                notional,
                option_type,
                strike,
                expiry,
                premium,
                ..
            } => calc_fx_option(
                pair,
                *notional,
                *option_type,
                *strike,
                *expiry,
                *premium,
                market,
                measure,
            ),
            Instrument::EqOption {
                symbol,
                quantity,
                multiplier,
                option_type,
                strike,
                expiry,
                ..
            } => calc_eq_option(
                symbol,
                *quantity,
                *multiplier,
                *option_type,
                *strike,
                *expiry,
                market,
                measure,
            ),
            Instrument::IrSwap {
                currency,
                pay_or_receive,
                notional,
                termination,
                fixed_rate,
                ..
            } => calc_ir_swap(
                currency,
                *pay_or_receive,
                *notional,
                *termination,
                *fixed_rate,
                market,
                measure,
            ),
        }
    }
}

fn calc_spot_equity(
    symbol: &str,
    quantity: f64,
    market: &MarketSnapshot,
    measure: &RiskMeasure,
) -> Result<RiskResult, QuantError> {
    let is_fx = market.eq_spot(symbol).is_err() && market.fx_spot(symbol).is_ok();
    let spot = if is_fx {
        market.fx_spot(symbol)?
    } else {
        market.eq_spot(symbol)?
    };
    let pv = quantity * spot;

    match measure {
        RiskMeasure::Price | RiskMeasure::DollarPrice => {
            Ok(RiskResult::new(measure.clone(), pv, Some("USD".to_owned())))
        }
        RiskMeasure::FxDelta if is_fx => Ok(RiskResult::new(
            RiskMeasure::FxDelta,
            quantity,
            Some("USD".to_owned()),
        )),
        RiskMeasure::EqDelta if !is_fx => Ok(RiskResult::new(
            RiskMeasure::EqDelta,
            quantity * spot,
            Some("USD".to_owned()),
        )),
        RiskMeasure::FxGamma
        | RiskMeasure::FxVega
        | RiskMeasure::FxTheta
        | RiskMeasure::EqGamma
        | RiskMeasure::EqVega
        | RiskMeasure::EqTheta => Ok(RiskResult::new(
            measure.clone(),
            0.0,
            Some("USD".to_owned()),
        )),
        RiskMeasure::FxSpot => Ok(RiskResult::new(RiskMeasure::FxSpot, spot, None)),
        RiskMeasure::EqSpot => Ok(RiskResult::new(RiskMeasure::EqSpot, spot, None)),
        other => Err(QuantError::invalid_parameter(format!(
            "measure {other} is not supported on spot instrument `{symbol}`"
        ))),
    }
}

fn calc_fx_forward(
    pair: &str,
    notional: f64,
    delivery: ExpirySpec,
    forward_rate: Option<f64>,
    market: &MarketSnapshot,
    measure: &RiskMeasure,
) -> Result<RiskResult, QuantError> {
    let empty_holidays = BTreeSet::new();
    let del_date = delivery.resolve_date(market.date, &empty_holidays);
    let dt = ((del_date - market.date).num_days().max(0) as f64) / 365.0;
    let spot = market.fx_spot(pair)?;
    let (r_d, r_f) = fx_pair_rates(pair, market);
    let mkt_fwd = spot * ((r_d - r_f) * dt).exp();
    let strike_fwd = forward_rate.unwrap_or(mkt_fwd);

    // PV in quote currency: notional * (S * exp(-r_f * T) - K * exp(-r_d * T))
    let pv_quote = notional * (spot * (-r_f * dt).exp() - strike_fwd * (-r_d * dt).exp());
    let quote_ccy = fx_quote_currency(pair);
    let usd_per_quote = quote_to_usd_rate(quote_ccy, market)?;
    let pv_usd = pv_quote * usd_per_quote;

    match measure {
        RiskMeasure::Price => Ok(RiskResult::new(
            RiskMeasure::Price,
            pv_quote,
            Some(quote_ccy.to_owned()),
        )),
        RiskMeasure::DollarPrice => Ok(RiskResult::new(
            RiskMeasure::DollarPrice,
            pv_usd,
            Some("USD".to_owned()),
        )),
        RiskMeasure::FxDelta => {
            let delta = notional * (-r_f * dt).exp() * usd_per_quote;
            Ok(RiskResult::new(
                RiskMeasure::FxDelta,
                delta,
                Some("USD".to_owned()),
            ))
        }
        RiskMeasure::FxGamma | RiskMeasure::FxVega => Ok(RiskResult::new(
            measure.clone(),
            0.0,
            Some("USD".to_owned()),
        )),
        RiskMeasure::FxTheta => {
            let theta = notional
                * (r_f * spot * (-r_f * dt).exp() - r_d * strike_fwd * (-r_d * dt).exp())
                * usd_per_quote
                / 365.0;
            Ok(RiskResult::new(
                RiskMeasure::FxTheta,
                theta,
                Some("USD".to_owned()),
            ))
        }
        RiskMeasure::FxSpot => Ok(RiskResult::new(RiskMeasure::FxSpot, spot, None)),
        RiskMeasure::FxImpliedVol => Ok(RiskResult::new(
            RiskMeasure::FxImpliedVol,
            market.fx_vol(pair, "ATM").unwrap_or(0.0),
            None,
        )),
        other => Err(QuantError::invalid_parameter(format!(
            "measure {other} is not supported on FxForward"
        ))),
    }
}

#[allow(clippy::too_many_arguments)]
fn calc_fx_option(
    pair: &str,
    notional: f64,
    option_type: OptionType,
    strike_spec: StrikeSpec,
    expiry: ExpirySpec,
    premium: f64,
    market: &MarketSnapshot,
    measure: &RiskMeasure,
) -> Result<RiskResult, QuantError> {
    let empty_holidays = BTreeSet::new();
    let exp_date = expiry.resolve_date(market.date, &empty_holidays);
    let dt = ((exp_date - market.date).num_days().max(0) as f64) / 365.0;
    let spot = market.fx_spot(pair)?;
    let vol = market.fx_vol(pair, "ATM")?;
    let (r_d, r_f) = fx_pair_rates(pair, market);

    let strike = match strike_spec {
        StrikeSpec::Absolute(k) => k,
        StrikeSpec::AtmSpot => spot,
        StrikeSpec::AtmForward => fx_forward_rate(pair, market.date, exp_date, market)?,
        StrikeSpec::PctOfSpot(pct) => spot * pct,
    };

    let quote_ccy = fx_quote_currency(pair);
    let usd_per_quote = quote_to_usd_rate(quote_ccy, market)?;
    let g = garman_kohlhagen(spot, strike, dt, r_d, r_f, vol, option_type);

    let pv_quote = notional * g.price - premium;
    let pv_usd = pv_quote * usd_per_quote;

    match measure {
        RiskMeasure::Price => Ok(RiskResult::new(
            RiskMeasure::Price,
            pv_quote,
            Some(quote_ccy.to_owned()),
        )),
        RiskMeasure::DollarPrice => Ok(RiskResult::new(
            RiskMeasure::DollarPrice,
            pv_usd,
            Some("USD".to_owned()),
        )),
        RiskMeasure::FxDelta => Ok(RiskResult::new(
            RiskMeasure::FxDelta,
            notional * g.delta * usd_per_quote,
            Some("USD".to_owned()),
        )),
        RiskMeasure::FxGamma => Ok(RiskResult::new(
            RiskMeasure::FxGamma,
            notional * g.gamma * usd_per_quote,
            Some("USD".to_owned()),
        )),
        RiskMeasure::FxVega => Ok(RiskResult::new(
            RiskMeasure::FxVega,
            // 1 vol point = +0.01 in sigma
            notional * g.vega_per_unit * 0.01 * usd_per_quote,
            Some("USD".to_owned()),
        )),
        RiskMeasure::FxTheta => Ok(RiskResult::new(
            RiskMeasure::FxTheta,
            // 1 calendar day time decay
            notional * g.theta_per_year * (1.0 / 365.0) * usd_per_quote,
            Some("USD".to_owned()),
        )),
        RiskMeasure::FxSpot => Ok(RiskResult::new(RiskMeasure::FxSpot, spot, None)),
        RiskMeasure::FxImpliedVol => Ok(RiskResult::new(RiskMeasure::FxImpliedVol, vol, None)),
        other => Err(QuantError::invalid_parameter(format!(
            "measure {other} is not supported on FxOption"
        ))),
    }
}

#[allow(clippy::too_many_arguments)]
fn calc_eq_option(
    symbol: &str,
    quantity: f64,
    multiplier: f64,
    option_type: OptionType,
    strike_spec: StrikeSpec,
    expiry: ExpirySpec,
    market: &MarketSnapshot,
    measure: &RiskMeasure,
) -> Result<RiskResult, QuantError> {
    let empty_holidays = BTreeSet::new();
    let exp_date = expiry.resolve_date(market.date, &empty_holidays);
    let dt = ((exp_date - market.date).num_days().max(0) as f64) / 365.0;
    let spot = market.eq_spot(symbol)?;
    let vol = market.eq_vol(symbol, "ATM")?;
    let r = market.risk_free_rate_or_zero("USD");
    let q = market.eq_dividend_yield_or_zero(symbol);

    let strike = match strike_spec {
        StrikeSpec::Absolute(k) => k,
        StrikeSpec::AtmSpot => spot,
        StrikeSpec::AtmForward => spot * ((r - q) * dt).exp(),
        StrikeSpec::PctOfSpot(pct) => spot * pct,
    };

    let scale = quantity * multiplier;
    let g = garman_kohlhagen(spot, strike, dt, r, q, vol, option_type);
    let pv = scale * g.price;

    match measure {
        RiskMeasure::Price | RiskMeasure::DollarPrice => {
            Ok(RiskResult::new(measure.clone(), pv, Some("USD".to_owned())))
        }
        // Per gs_quant risk-measures.md:
        // EqDelta is USD PV change for a 100% move in spot (spot * dPV/dS).
        RiskMeasure::EqDelta => Ok(RiskResult::new(
            RiskMeasure::EqDelta,
            scale * g.delta * spot,
            Some("USD".to_owned()),
        )),
        // Per risk-measures.md: PnL ≈ EqDelta * x + 50 * EqGamma * x^2 where x = dS/S
        // => 50 * EqGamma = 0.5 * S^2 * d2PV/dS2 => EqGamma = 0.01 * S^2 * gamma.
        RiskMeasure::EqGamma => Ok(RiskResult::new(
            RiskMeasure::EqGamma,
            scale * 0.01 * spot * spot * g.gamma,
            Some("USD".to_owned()),
        )),
        RiskMeasure::EqVega => Ok(RiskResult::new(
            RiskMeasure::EqVega,
            scale * g.vega_per_unit * 0.01,
            Some("USD".to_owned()),
        )),
        RiskMeasure::EqTheta => Ok(RiskResult::new(
            RiskMeasure::EqTheta,
            scale * g.theta_per_year / 365.0,
            Some("USD".to_owned()),
        )),
        RiskMeasure::EqSpot => Ok(RiskResult::new(RiskMeasure::EqSpot, spot, None)),
        RiskMeasure::EqImpliedVol => Ok(RiskResult::new(RiskMeasure::EqImpliedVol, vol, None)),
        other => Err(QuantError::invalid_parameter(format!(
            "measure {other} is not supported on EqOption"
        ))),
    }
}

fn calc_ir_swap(
    currency: &str,
    pay_or_receive: PayReceive,
    notional: f64,
    termination: ExpirySpec,
    fixed_rate: Option<f64>,
    market: &MarketSnapshot,
    measure: &RiskMeasure,
) -> Result<RiskResult, QuantError> {
    let empty_holidays = BTreeSet::new();
    let term_date = termination.resolve_date(market.date, &empty_holidays);
    let years = ((term_date - market.date).num_days().max(0) as f64) / 365.0;
    let point = format!("{}Y", years.round().max(1.0) as i64);
    let mkt_par = market.ir_rate(currency, &point)?;
    let contract_rate = fixed_rate.unwrap_or(mkt_par);

    // Semi-annual annuity approximation on flat yield curve at mkt_par
    let periods = (years * 2.0).max(0.0);
    let disc_rate = mkt_par.max(0.0001);
    let annuity = if years <= 0.0 {
        0.0
    } else if periods < 1.0 {
        years * (-disc_rate * years).exp()
    } else {
        let n = periods.round() as i32;
        let step = years / f64::from(n);
        (1..=n)
            .map(|k| step * (-disc_rate * step * f64::from(k)).exp())
            .sum::<f64>()
    };

    // Pay fixed gains when market rates rise above contract_rate; Receive fixed gains when rates fall.
    let sign = match pay_or_receive {
        PayReceive::Pay => 1.0,
        PayReceive::Receive => -1.0,
    };
    let pv_local = sign * notional * (mkt_par - contract_rate) * annuity;
    let usd_rate = quote_to_usd_rate(currency, market).unwrap_or(1.0);

    match measure {
        RiskMeasure::Price => Ok(RiskResult::new(
            RiskMeasure::Price,
            pv_local,
            Some(currency.to_owned()),
        )),
        RiskMeasure::DollarPrice => Ok(RiskResult::new(
            RiskMeasure::DollarPrice,
            pv_local * usd_rate,
            Some("USD".to_owned()),
        )),
        RiskMeasure::IrDeltaParallel => Ok(RiskResult::new(
            RiskMeasure::IrDeltaParallel,
            sign * notional * annuity * 0.0001 * usd_rate,
            Some("USD".to_owned()),
        )),
        RiskMeasure::IrFwdRate => Ok(RiskResult::new(RiskMeasure::IrFwdRate, mkt_par, None)),
        other => Err(QuantError::invalid_parameter(format!(
            "measure {other} is not supported on IrSwap"
        ))),
    }
}

fn fx_pair_rates(pair: &str, market: &MarketSnapshot) -> (f64, f64) {
    if pair.len() != 6 {
        return (0.0, 0.0);
    }
    let base = &pair[0..3];
    let quote = &pair[3..6];
    (
        market.risk_free_rate_or_zero(quote),
        market.risk_free_rate_or_zero(base),
    )
}

fn fx_quote_currency(pair: &str) -> &str {
    if pair.len() == 6 {
        &pair[3..6]
    } else {
        "USD"
    }
}

fn quote_to_usd_rate(ccy: &str, market: &MarketSnapshot) -> Result<f64, QuantError> {
    if ccy.eq_ignore_ascii_case("USD") {
        return Ok(1.0);
    }
    let direct = format!("{ccy}USD");
    if let Ok(rate) = market.fx_spot(&direct) {
        return Ok(rate);
    }
    Ok(1.0)
}

struct OptionGreeks {
    price: f64,
    delta: f64,
    gamma: f64,
    vega_per_unit: f64,
    theta_per_year: f64,
}

fn garman_kohlhagen(
    spot: f64,
    strike: f64,
    t: f64,
    r_d: f64,
    r_f: f64,
    sigma: f64,
    option_type: OptionType,
) -> OptionGreeks {
    if t <= 1e-12 || sigma <= 1e-12 {
        let intrinsic = match option_type {
            OptionType::Call => (spot - strike).max(0.0),
            OptionType::Put => (strike - spot).max(0.0),
        };
        let delta = match option_type {
            OptionType::Call => {
                if spot > strike {
                    (-r_f * t).exp()
                } else {
                    0.0
                }
            }
            OptionType::Put => {
                if spot < strike {
                    -(-r_f * t).exp()
                } else {
                    0.0
                }
            }
        };
        return OptionGreeks {
            price: intrinsic,
            delta,
            gamma: 0.0,
            vega_per_unit: 0.0,
            theta_per_year: 0.0,
        };
    }

    let sqrt_t = t.sqrt();
    let vol_sqrt_t = sigma * sqrt_t;
    let d1 = ((spot / strike).ln() + (r_d - r_f + 0.5 * sigma * sigma) * t) / vol_sqrt_t;
    let d2 = d1 - vol_sqrt_t;

    let df_f = (-r_f * t).exp();
    let df_d = (-r_d * t).exp();
    let pdf_d1 = norm_pdf(d1);

    let (price, delta, theta_per_year) = match option_type {
        OptionType::Call => {
            let n_d1 = norm_cdf(d1);
            let n_d2 = norm_cdf(d2);
            let px = spot * df_f * n_d1 - strike * df_d * n_d2;
            let del = df_f * n_d1;
            let th = -(spot * df_f * pdf_d1 * sigma) / (2.0 * sqrt_t) + r_f * spot * df_f * n_d1
                - r_d * strike * df_d * n_d2;
            (px, del, th)
        }
        OptionType::Put => {
            let n_neg_d1 = norm_cdf(-d1);
            let n_neg_d2 = norm_cdf(-d2);
            let px = strike * df_d * n_neg_d2 - spot * df_f * n_neg_d1;
            let del = -df_f * n_neg_d1;
            let th = -(spot * df_f * pdf_d1 * sigma) / (2.0 * sqrt_t)
                - r_f * spot * df_f * n_neg_d1
                + r_d * strike * df_d * n_neg_d2;
            (px, del, th)
        }
    };

    let gamma = (df_f * pdf_d1) / (spot * vol_sqrt_t);
    let vega_per_unit = spot * df_f * pdf_d1 * sqrt_t;

    OptionGreeks {
        price,
        delta,
        gamma,
        vega_per_unit,
        theta_per_year,
    }
}

/// Standard normal probability density function.
#[must_use]
pub fn norm_pdf(x: f64) -> f64 {
    const INV_SQRT_2PI: f64 = 0.398_942_280_401_432_7;
    INV_SQRT_2PI * (-0.5 * x * x).exp()
}

/// Standard normal cumulative distribution function accurate to machine precision (`~1e-15`).
#[must_use]
pub fn norm_cdf(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    let z = x.abs() * FRAC_1_SQRT_2;
    if z < 3.5 {
        // Positive-term series: erf(z) = (2/sqrt(pi)) * exp(-z^2) * sum_{k>=0} (2^k z^{2k+1}) / (2k+1)!!
        let two_z2 = 2.0 * z * z;
        let mut term = z;
        let mut sum = term;
        for k in 1_u32..120 {
            term *= two_z2 / f64::from(2 * k + 1);
            sum += term;
            if term.abs() <= sum.abs() * 1e-16 {
                break;
            }
        }
        let erf_z = (FRAC_2_SQRT_PI * (-z * z).exp() * sum).min(1.0);
        if x >= 0.0 {
            0.5 * (1.0 + erf_z)
        } else {
            0.5 * (1.0 - erf_z)
        }
    } else {
        // Laplace continued fraction for erfc(z) valid for z >= 3.5
        let mut cf = z;
        for k in (1_u32..=40).rev() {
            cf = z + (0.5 * f64::from(k)) / cf;
        }
        let erfc_z = (-z * z).exp() / (PI.sqrt() * cf);
        if x >= 0.0 {
            1.0 - 0.5 * erfc_z
        } else {
            0.5 * erfc_z
        }
    }
}

/// Ordered collection of [`Instrument`] positions for batch pricing and risk aggregation.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Portfolio {
    /// Portfolio name.
    pub name: String,
    /// Instruments held in the portfolio.
    pub instruments: Vec<Instrument>,
}

impl Portfolio {
    /// Creates an empty [`Portfolio`].
    #[must_use]
    pub fn new(name: impl Into<String>, instruments: Vec<Instrument>) -> Self {
        Self {
            name: name.into(),
            instruments,
        }
    }

    /// Appends an instrument to the portfolio.
    pub fn push(&mut self, instrument: Instrument) {
        self.instruments.push(instrument);
    }

    /// Resolves every instrument in the portfolio against `market` on `trade_date`.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError`] if any instrument fails resolution.
    pub fn resolve(
        &self,
        trade_date: NaiveDate,
        market: &MarketSnapshot,
        holidays: &BTreeSet<NaiveDate>,
    ) -> Result<Self, QuantError> {
        let mut resolved = Vec::with_capacity(self.instruments.len());
        for inst in &self.instruments {
            resolved.push(inst.resolve(trade_date, market, holidays)?);
        }
        Ok(Self {
            name: self.name.clone(),
            instruments: resolved,
        })
    }

    /// Computes and sums `measure` across all instruments in the portfolio.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError`] if any constituent calculation fails.
    pub fn calc_aggregate(
        &self,
        market: &MarketSnapshot,
        measure: &RiskMeasure,
        pricer: &dyn Pricer,
    ) -> Result<f64, QuantError> {
        let mut total = 0.0;
        for inst in &self.instruments {
            total += pricer.calc(inst, market, measure)?.value;
        }
        Ok(total)
    }
}

/// Single row produced by [`HistoricalPricer::run`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoricalPricePoint {
    /// Valuation date.
    pub date: NaiveDate,
    /// Aggregate portfolio measure value.
    pub value: f64,
}

/// Direct static portfolio historical pricer (`HistoricalPricingContext` analog).
///
/// Per `gs_quant`'s `backtesting.md`: if a strategy holds a single instrument or fixed portfolio
/// over a date range with no triggers, rebalancing, or hedging, price directly with
/// [`HistoricalPricer`] instead of running [`crate::backtest::GenericEngine`].
#[derive(Debug, Clone, Copy, Default)]
pub struct HistoricalPricer;

impl HistoricalPricer {
    /// Prices `portfolio` across `dates` for `measure`.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError`] if market data is missing or pricing fails on any date.
    pub fn run(
        portfolio: &Portfolio,
        dates: &[NaiveDate],
        history: &MarketHistory,
        measure: &RiskMeasure,
        pricer: &dyn Pricer,
    ) -> Result<Vec<HistoricalPricePoint>, QuantError> {
        let mut out = Vec::with_capacity(dates.len());
        for &date in dates {
            let snap = history.snapshot(date, MissingDataStrategy::FillForward)?;
            let value = portfolio.calc_aggregate(&snap, measure, pricer)?;
            out.push(HistoricalPricePoint { date, value });
        }
        Ok(out)
    }
}
