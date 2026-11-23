//! Financial instruments, relative strikes/expiries, and entry-date resolution.

use std::collections::BTreeSet;
use std::str::FromStr;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::QuantError;
use crate::market::MarketSnapshot;
use crate::tenor::Tenor;

/// Vanilla option payoff direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OptionType {
    /// Right to buy the underlying at strike (`max(S - K, 0)`).
    Call,
    /// Right to sell the underlying at strike (`max(K - S, 0)`).
    Put,
}

/// Swap fixed-leg side (`Pay` fixed or `Receive` fixed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PayReceive {
    /// Pay the fixed rate and receive floating.
    Pay,
    /// Receive the fixed rate and pay floating.
    Receive,
}

/// Option strike specification supporting absolute and relative conventions (`instruments.md`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum StrikeSpec {
    /// Explicit numeric strike level.
    Absolute(f64),
    /// At-the-money spot (`'ATMS'` / `'s'`).
    AtmSpot,
    /// At-the-money forward (`'ATMF'` / `'f'`).
    AtmForward,
    /// Percentage of spot (`'95%'` -> `0.95`, `'1.05*s'` -> `1.05`).
    PctOfSpot(f64),
}

/// Expiration or maturity specification: either a fixed calendar date or a relative [`Tenor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExpirySpec {
    /// Concrete expiration or termination date.
    FixedDate(NaiveDate),
    /// Relative tenor resolved from the trade entry date.
    Relative(Tenor),
}

impl ExpirySpec {
    /// Parses either a `YYYY-MM-DD` date or a tenor string such as `"3m"`.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError::InvalidParameter`] if the string is neither a date nor a valid tenor.
    pub fn parse(raw: &str) -> Result<Self, QuantError> {
        let trimmed = raw.trim();
        if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
            return Ok(Self::FixedDate(date));
        }
        if let Ok(tenor) = Tenor::from_str(trimmed) {
            return Ok(Self::Relative(tenor));
        }
        Err(QuantError::invalid_parameter(format!(
            "invalid expiry `{raw}`: expected YYYY-MM-DD or tenor"
        )))
    }

    /// Resolves this specification relative to `anchor_date`, returning a fixed date.
    #[must_use]
    pub fn resolve_date(
        &self,
        anchor_date: NaiveDate,
        holidays: &BTreeSet<NaiveDate>,
    ) -> NaiveDate {
        match *self {
            Self::FixedDate(d) => d,
            Self::Relative(t) => t.add_to_date(anchor_date, holidays),
        }
    }
}

/// Tradeable financial instruments supported by the local analytical pricer and backtest engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Instrument {
    /// Spot equity, ETF, or FX linear position (`quantity` units).
    SpotEquity {
        /// Human-readable instrument name shown in the trade ledger.
        name: String,
        /// Underlying ticker or series key (e.g. `"SPY"` or `"EURUSD"`).
        symbol: String,
        /// Signed position size (positive = long, negative = short).
        quantity: f64,
    },

    /// Deliverable FX forward contract on `pair` (e.g. `"EURUSD"`).
    FxForward {
        /// Instrument identifier.
        name: String,
        /// 6-character currency pair (`BASEQUOTE`, e.g. `"EURUSD"`).
        pair: String,
        /// Signed notional in the base currency (e.g. `+100_000.0` EUR).
        notional: f64,
        /// Settlement/delivery date specification.
        delivery: ExpirySpec,
        /// Contracted outright forward FX rate (`None` resolves to market forward at trade date).
        forward_rate: Option<f64>,
    },

    /// European vanilla FX option on `pair` (`premium` defaults to `0.0` per `instruments.md`).
    FxOption {
        /// Instrument identifier.
        name: String,
        /// 6-character currency pair (`BASEQUOTE`, e.g. `"EURUSD"`).
        pair: String,
        /// Signed notional in the base currency.
        notional: f64,
        /// Call or Put on the base currency.
        option_type: OptionType,
        /// Strike specification (absolute or relative).
        strike: StrikeSpec,
        /// Expiration specification.
        expiry: ExpirySpec,
        /// Upfront premium paid (`0.0` for live MTM valuation without unpaid premium offset).
        premium: f64,
    },

    /// European vanilla equity or index option.
    EqOption {
        /// Instrument identifier.
        name: String,
        /// Underlying symbol (e.g. `".SPX"` or `".STOXX50E"`).
        symbol: String,
        /// Signed option count.
        quantity: f64,
        /// Contract multiplier (e.g. `1.0` or `100.0`).
        multiplier: f64,
        /// Call or Put.
        option_type: OptionType,
        /// Strike specification.
        strike: StrikeSpec,
        /// Expiration specification.
        expiry: ExpirySpec,
    },

    /// Plain-vanilla fixed-for-floating interest rate swap.
    IrSwap {
        /// Instrument identifier.
        name: String,
        /// Notional currency (e.g. `"USD"`, `"EUR"`).
        currency: String,
        /// Pay or receive fixed coupon.
        pay_or_receive: PayReceive,
        /// Signed notional amount.
        notional: f64,
        /// Termination date or swap tenor (e.g. `"10y"`).
        termination: ExpirySpec,
        /// Fixed coupon rate as a decimal (`None` resolves to market par swap rate at entry).
        fixed_rate: Option<f64>,
    },
}

impl Instrument {
    /// Creates an FX forward with `forward_rate = None` (resolved to par forward on entry).
    #[must_use]
    pub fn fx_forward(
        name: impl Into<String>,
        pair: impl Into<String>,
        notional: f64,
        delivery: ExpirySpec,
    ) -> Self {
        Self::FxForward {
            name: name.into(),
            pair: pair.into().to_ascii_uppercase(),
            notional,
            delivery,
            forward_rate: None,
        }
    }

    /// Creates a vanilla FX option with `premium = 0.0` (following `instruments.md`).
    #[must_use]
    pub fn fx_option(
        name: impl Into<String>,
        pair: impl Into<String>,
        notional: f64,
        option_type: OptionType,
        strike: StrikeSpec,
        expiry: ExpirySpec,
    ) -> Self {
        Self::FxOption {
            name: name.into(),
            pair: pair.into().to_ascii_uppercase(),
            notional,
            option_type,
            strike,
            expiry,
            premium: 0.0,
        }
    }

    /// Creates a vanilla equity option.
    #[must_use]
    pub fn eq_option(
        name: impl Into<String>,
        symbol: impl Into<String>,
        quantity: f64,
        multiplier: f64,
        option_type: OptionType,
        strike: StrikeSpec,
        expiry: ExpirySpec,
    ) -> Self {
        Self::EqOption {
            name: name.into(),
            symbol: symbol.into(),
            quantity,
            multiplier,
            option_type,
            strike,
            expiry,
        }
    }

    /// Creates an interest rate swap with `fixed_rate = None` (resolved to par at entry).
    #[must_use]
    pub fn ir_swap(
        name: impl Into<String>,
        currency: impl Into<String>,
        pay_or_receive: PayReceive,
        notional: f64,
        termination: ExpirySpec,
    ) -> Self {
        Self::IrSwap {
            name: name.into(),
            currency: currency.into().to_ascii_uppercase(),
            pay_or_receive,
            notional,
            termination,
            fixed_rate: None,
        }
    }

    /// Creates a linear spot position (for equities, ETFs, or spot FX pairs).
    #[must_use]
    pub fn spot(name: impl Into<String>, symbol: impl Into<String>, quantity: f64) -> Self {
        Self::SpotEquity {
            name: name.into(),
            symbol: symbol.into(),
            quantity,
        }
    }

    /// Returns the instrument's name.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::SpotEquity { name, .. }
            | Self::FxForward { name, .. }
            | Self::FxOption { name, .. }
            | Self::EqOption { name, .. }
            | Self::IrSwap { name, .. } => name,
        }
    }

    /// Returns a copy of this instrument with a new `name`.
    #[must_use]
    pub fn with_name(mut self, new_name: impl Into<String>) -> Self {
        let updated = new_name.into();
        match &mut self {
            Self::SpotEquity { name, .. }
            | Self::FxForward { name, .. }
            | Self::FxOption { name, .. }
            | Self::EqOption { name, .. }
            | Self::IrSwap { name, .. } => *name = updated,
        }
        self
    }

    /// Returns a copy of this instrument whose trade size (`notional` or `quantity`) is scaled by `factor`.
    #[must_use]
    pub fn with_scale(&self, factor: f64) -> Self {
        let mut cloned = self.clone();
        match &mut cloned {
            Self::SpotEquity { quantity, .. } | Self::EqOption { quantity, .. } => {
                *quantity *= factor;
            }
            Self::FxForward { notional, .. } | Self::IrSwap { notional, .. } => {
                *notional *= factor;
            }
            Self::FxOption {
                notional, premium, ..
            } => {
                *notional *= factor;
                *premium *= factor;
            }
        }
        cloned
    }

    /// Returns the resolved fixed expiration or termination date if set.
    #[must_use]
    pub fn fixed_expiry_date(&self) -> Option<NaiveDate> {
        match self {
            Self::SpotEquity { .. } => None,
            Self::FxForward {
                delivery: ExpirySpec::FixedDate(d),
                ..
            }
            | Self::FxOption {
                expiry: ExpirySpec::FixedDate(d),
                ..
            }
            | Self::EqOption {
                expiry: ExpirySpec::FixedDate(d),
                ..
            }
            | Self::IrSwap {
                termination: ExpirySpec::FixedDate(d),
                ..
            } => Some(*d),
            _ => None,
        }
    }

    /// Returns the unsigned economic notional size used by [`crate::costs::TransactionCostModel`].
    #[must_use]
    pub fn notional_amount(&self, market: &MarketSnapshot) -> f64 {
        match self {
            Self::SpotEquity {
                symbol, quantity, ..
            } => {
                let px = market
                    .eq_spot(symbol)
                    .or_else(|_| market.fx_spot(symbol))
                    .unwrap_or(1.0);
                (quantity * px).abs()
            }
            Self::FxForward { notional, .. }
            | Self::FxOption { notional, .. }
            | Self::IrSwap { notional, .. } => notional.abs(),
            Self::EqOption {
                symbol,
                quantity,
                multiplier,
                ..
            } => {
                let spot = market.eq_spot(symbol).unwrap_or(1.0);
                (quantity * multiplier * spot).abs()
            }
        }
    }

    /// Returns the signed primary quantity/notional figure for reporting in the trade ledger.
    #[must_use]
    pub fn signed_size(&self) -> f64 {
        match self {
            Self::SpotEquity { quantity, .. } => *quantity,
            Self::EqOption {
                quantity,
                multiplier,
                ..
            } => quantity * multiplier,
            Self::FxForward { notional, .. }
            | Self::FxOption { notional, .. }
            | Self::IrSwap { notional, .. } => *notional,
        }
    }

    /// Resolves any relative expiries, relative strikes, or `None` par rates against `market` on
    /// `trade_date` so the entered trade carries fixed economic terms over its holding period.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError::MissingMarketData`] if required market quotes are missing.
    pub fn resolve(
        &self,
        trade_date: NaiveDate,
        market: &MarketSnapshot,
        holidays: &BTreeSet<NaiveDate>,
    ) -> Result<Self, QuantError> {
        match self {
            Self::SpotEquity { .. } => Ok(self.clone()),
            Self::FxForward {
                name,
                pair,
                notional,
                delivery,
                forward_rate,
            } => {
                let fixed_del = delivery.resolve_date(trade_date, holidays);
                let resolved_fwd = match forward_rate {
                    Some(k) => *k,
                    None => fx_forward_rate(pair, trade_date, fixed_del, market)?,
                };
                Ok(Self::FxForward {
                    name: name.clone(),
                    pair: pair.clone(),
                    notional: *notional,
                    delivery: ExpirySpec::FixedDate(fixed_del),
                    forward_rate: Some(resolved_fwd),
                })
            }
            Self::FxOption {
                name,
                pair,
                notional,
                option_type,
                strike,
                expiry,
                premium,
            } => {
                let fixed_exp = expiry.resolve_date(trade_date, holidays);
                let spot = market.fx_spot(pair)?;
                let k = match *strike {
                    StrikeSpec::Absolute(v) => v,
                    StrikeSpec::AtmSpot => spot,
                    StrikeSpec::AtmForward => fx_forward_rate(pair, trade_date, fixed_exp, market)?,
                    StrikeSpec::PctOfSpot(pct) => spot * pct,
                };
                Ok(Self::FxOption {
                    name: name.clone(),
                    pair: pair.clone(),
                    notional: *notional,
                    option_type: *option_type,
                    strike: StrikeSpec::Absolute(k),
                    expiry: ExpirySpec::FixedDate(fixed_exp),
                    premium: *premium,
                })
            }
            Self::EqOption {
                name,
                symbol,
                quantity,
                multiplier,
                option_type,
                strike,
                expiry,
            } => {
                let fixed_exp = expiry.resolve_date(trade_date, holidays);
                let spot = market.eq_spot(symbol)?;
                let dt = ((fixed_exp - trade_date).num_days().max(0) as f64) / 365.0;
                let r = market.risk_free_rate_or_zero("USD");
                let q = market.eq_dividend_yield_or_zero(symbol);
                let k = match *strike {
                    StrikeSpec::Absolute(v) => v,
                    StrikeSpec::AtmSpot => spot,
                    StrikeSpec::AtmForward => spot * ((r - q) * dt).exp(),
                    StrikeSpec::PctOfSpot(pct) => spot * pct,
                };
                Ok(Self::EqOption {
                    name: name.clone(),
                    symbol: symbol.clone(),
                    quantity: *quantity,
                    multiplier: *multiplier,
                    option_type: *option_type,
                    strike: StrikeSpec::Absolute(k),
                    expiry: ExpirySpec::FixedDate(fixed_exp),
                })
            }
            Self::IrSwap {
                name,
                currency,
                pay_or_receive,
                notional,
                termination,
                fixed_rate,
            } => {
                let fixed_term = termination.resolve_date(trade_date, holidays);
                let point = match termination {
                    ExpirySpec::Relative(t) => t.to_string().to_ascii_uppercase(),
                    ExpirySpec::FixedDate(d) => {
                        let yrs = (((*d - trade_date).num_days() as f64) / 365.0).round() as i64;
                        format!("{}Y", yrs.max(1))
                    }
                };
                let par = match fixed_rate {
                    Some(r) => *r,
                    None => market.ir_rate(currency, &point)?,
                };
                Ok(Self::IrSwap {
                    name: name.clone(),
                    currency: currency.clone(),
                    pay_or_receive: *pay_or_receive,
                    notional: *notional,
                    termination: ExpirySpec::FixedDate(fixed_term),
                    fixed_rate: Some(par),
                })
            }
        }
    }
}

pub(crate) fn fx_forward_rate(
    pair: &str,
    val_date: NaiveDate,
    delivery_date: NaiveDate,
    market: &MarketSnapshot,
) -> Result<f64, QuantError> {
    let spot = market.fx_spot(pair)?;
    if pair.len() != 6 {
        return Ok(spot);
    }
    let base = &pair[0..3];
    let quote = &pair[3..6];
    let r_d = market.risk_free_rate_or_zero(quote);
    let r_f = market.risk_free_rate_or_zero(base);
    let dt = ((delivery_date - val_date).num_days().max(0) as f64) / 365.0;
    Ok(spot * ((r_d - r_f) * dt).exp())
}
