//! Market data shock scenarios and forward-rolling valuation helpers.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::QuantError;
use crate::instrument::Instrument;
use crate::market::{MarketDataKey, MarketSnapshot};

/// Wildcard pattern selecting quotes in a [`MarketSnapshot`].
///
/// Any field left as `None` matches all values for that key component, mirroring
/// `gs_quant.risk.MarketDataPattern`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct MarketDataPattern {
    /// Asset/curve group filter (e.g. `Some("FX")`, `Some("FX VOL")`, `Some("EQ VOL")`, `Some("IR")`).
    pub mkt_type: Option<String>,
    /// Asset filter (e.g. `Some("EURUSD")`, `Some(".SPX")`, `Some("EUR")`).
    pub mkt_asset: Option<String>,
    /// Sub-class filter (e.g. `Some("SPOT")`, `Some("ATM VOL")`, `Some("SWAP")`, `Some("Scale")`).
    pub mkt_class: Option<String>,
    /// Tenor/point filter (e.g. `Some("10Y")`, `Some("3M")`).
    pub mkt_point: Option<String>,
    /// Quoting convention filter (e.g. `Some("Mid")`, `Some("Value")`).
    pub mkt_quoting_style: Option<String>,
}

impl MarketDataPattern {
    /// Matches every market data quote.
    #[must_use]
    pub fn any() -> Self {
        Self::default()
    }

    /// Filters by `mkt_type`.
    #[must_use]
    pub fn with_type(mut self, mkt_type: impl Into<String>) -> Self {
        self.mkt_type = Some(mkt_type.into());
        self
    }

    /// Filters by `mkt_asset`.
    #[must_use]
    pub fn with_asset(mut self, mkt_asset: impl Into<String>) -> Self {
        self.mkt_asset = Some(mkt_asset.into());
        self
    }

    /// Filters by `mkt_class`.
    #[must_use]
    pub fn with_class(mut self, mkt_class: impl Into<String>) -> Self {
        self.mkt_class = Some(mkt_class.into());
        self
    }

    /// Filters by `mkt_point`.
    #[must_use]
    pub fn with_point(mut self, mkt_point: impl Into<String>) -> Self {
        self.mkt_point = Some(mkt_point.into());
        self
    }

    /// Filters by `mkt_quoting_style`.
    #[must_use]
    pub fn with_quoting_style(mut self, style: impl Into<String>) -> Self {
        self.mkt_quoting_style = Some(style.into());
        self
    }

    /// Returns `true` if `key` matches every specified component of this pattern (case-insensitively).
    #[must_use]
    pub fn matches(&self, key: &MarketDataKey) -> bool {
        matches_opt(&self.mkt_type, &key.mkt_type)
            && matches_opt(&self.mkt_asset, &key.mkt_asset)
            && matches_opt(&self.mkt_class, &key.mkt_class)
            && matches_opt(&self.mkt_point, &key.mkt_point)
            && matches_opt(&self.mkt_quoting_style, &key.mkt_quoting_style)
    }
}

fn matches_opt(pattern: &Option<String>, actual: &str) -> bool {
    match pattern {
        Some(expected) => expected.eq_ignore_ascii_case(actual),
        None => true,
    }
}

/// Transformation applied to each matched market data quote.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MarketDataShock {
    /// Adds a fixed decimal offset (`value + delta`), e.g. `0.0001` for +1bp or `0.01` for +1 vol point.
    Absolute(f64),
    /// Scales by a fractional change (`value * (1.0 + pct)`), e.g. `0.06` raises by 6%.
    Proportional(f64),
    /// Replaces matched quotes with a constant level.
    Override(f64),
}

impl MarketDataShock {
    /// Applies this shock to `value`.
    #[must_use]
    pub fn apply(self, value: f64) -> f64 {
        match self {
            Self::Absolute(delta) => value + delta,
            Self::Proportional(pct) => value * (1.0 + pct),
            Self::Override(level) => level,
        }
    }
}

/// Output of applying a [`MarketDataShockBasedScenario`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioResult {
    /// Shocked market snapshot.
    pub snapshot: MarketSnapshot,
    /// Number of market data points matched and transformed.
    ///
    /// Inspecting `matched_points > 0` avoids `gs_quant`'s silent-no-op pitfall when a pattern
    /// matches nothing.
    pub matched_points: usize,
}

/// Scenario mapping [`MarketDataPattern`] selectors to [`MarketDataShock`] transformations.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MarketDataShockBasedScenario {
    shocks: Vec<(MarketDataPattern, MarketDataShock)>,
}

impl MarketDataShockBasedScenario {
    /// Creates an empty scenario.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a scenario from `(pattern, shock)` rules.
    #[must_use]
    pub fn from_rules(rules: Vec<(MarketDataPattern, MarketDataShock)>) -> Self {
        Self { shocks: rules }
    }

    /// Appends a `(pattern, shock)` rule.
    #[must_use]
    pub fn with_shock(mut self, pattern: MarketDataPattern, shock: MarketDataShock) -> Self {
        self.shocks.push((pattern, shock));
        self
    }

    /// Applies all matching shock rules to `base`, returning the shocked snapshot and count of matched quotes.
    #[must_use]
    pub fn apply(&self, base: &MarketSnapshot) -> ScenarioResult {
        let mut shocked = base.clone();

        // Ensure equity vol Scale entries default to 1.0 when targeted by a Scale shock
        // and the snapshot already holds EQ VOL quotes for that asset.
        for (pattern, _) in &self.shocks {
            if pattern
                .mkt_type
                .as_deref()
                .is_some_and(|t| t.eq_ignore_ascii_case("EQ VOL"))
                && pattern
                    .mkt_class
                    .as_deref()
                    .is_some_and(|c| c.eq_ignore_ascii_case("Scale"))
            {
                if let Some(asset) = &pattern.mkt_asset {
                    let has_eq_vol = base.quotes.keys().any(|k| {
                        k.mkt_type.eq_ignore_ascii_case("EQ VOL")
                            && k.mkt_asset.eq_ignore_ascii_case(asset)
                    });
                    if has_eq_vol {
                        let scale_key = MarketDataKey::eq_vol_scale(asset);
                        shocked.quotes.entry(scale_key).or_insert(1.0);
                    }
                }
            }
        }

        let mut matched_points = 0usize;
        for (key, value) in &mut shocked.quotes {
            let mut key_matched = false;
            for (pattern, shock) in &self.shocks {
                if pattern.matches(key) {
                    *value = shock.apply(*value);
                    key_matched = true;
                }
            }
            if key_matched {
                matched_points += 1;
            }
        }

        ScenarioResult {
            snapshot: shocked,
            matched_points,
        }
    }
}

/// Spot-path convention for [`RollFwd`], mirroring `realise_fwd` in `gs_quant`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ForwardRealisation {
    /// Spot moves to today's forward for the roll date (`realise_fwd=True`, `gs_quant` default).
    #[default]
    SpotMovesToForward,
    /// Spot remains unchanged while option time-to-expiry shrinks (`realise_fwd=False`).
    KeepSpotFixed,
}

/// Advances a [`MarketSnapshot`] forward to `target_date` to simulate time decay and forward carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollFwd {
    /// Valuation date to roll forward to.
    pub target_date: NaiveDate,
    /// Controls whether spot evolves along today's forward curve or stays fixed.
    pub realisation: ForwardRealisation,
}

impl RollFwd {
    /// Creates a [`RollFwd`] scenario.
    #[must_use]
    pub fn new(target_date: NaiveDate, realisation: ForwardRealisation) -> Self {
        Self {
            target_date,
            realisation,
        }
    }

    /// Rolls `snapshot` forward to `self.target_date`, updating FX and equity spots if
    /// [`ForwardRealisation::SpotMovesToForward`] is active.
    ///
    /// ATM implied vols remain attached to their tenors per `gs_quant`'s `RollFwd` semantics;
    /// resolve relative-expiry instruments before rolling so they do not re-anchor their expiry.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError::InvalidScenario`] if `target_date < snapshot.date`.
    pub fn apply(
        &self,
        snapshot: &MarketSnapshot,
        instruments: &[Instrument],
    ) -> Result<MarketSnapshot, QuantError> {
        if self.target_date < snapshot.date {
            return Err(QuantError::InvalidScenario {
                message: format!(
                    "roll target date {} precedes snapshot date {}",
                    self.target_date, snapshot.date
                ),
            });
        }

        let dt_years = (self.target_date - snapshot.date).num_days() as f64 / 365.0;
        let mut rolled = snapshot.clone();
        rolled.date = self.target_date;

        if self.realisation == ForwardRealisation::SpotMovesToForward && dt_years > 0.0 {
            let keys: Vec<MarketDataKey> = snapshot.quotes.keys().cloned().collect();
            for key in keys {
                if key.mkt_type == "FX" && key.mkt_class == "SPOT" {
                    let pair = &key.mkt_asset;
                    if pair.len() == 6 {
                        let base = &pair[0..3];
                        let quote = &pair[3..6];
                        let r_d = snapshot.risk_free_rate_or_zero(quote);
                        let r_f = snapshot.risk_free_rate_or_zero(base);
                        if let Some(spot) = snapshot.get(&key) {
                            let fwd = spot * ((r_d - r_f) * dt_years).exp();
                            rolled.insert(key, fwd);
                        }
                    }
                } else if key.mkt_type == "EQ" && key.mkt_class == "SPOT" {
                    let symbol = &key.mkt_asset;
                    let r = snapshot.risk_free_rate_or_zero("USD");
                    let q = snapshot.eq_dividend_yield_or_zero(symbol);
                    if let Some(spot) = snapshot.get(&key) {
                        let fwd = spot * ((r - q) * dt_years).exp();
                        rolled.insert(key, fwd);
                    }
                }
            }
            // Also validate any FX pair referenced in instruments has spot if needed?
            // Keeping pure quote-based roll handles all loaded market data uniformly.
            let _ = instruments;
        }

        Ok(rolled)
    }
}
