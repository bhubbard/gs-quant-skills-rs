//! Transaction cost models (constant, risk/notional scaled, and aggregate max/min/sum).

use serde::{Deserialize, Serialize};

use crate::error::QuantError;
use crate::instrument::Instrument;
use crate::market::MarketSnapshot;
use crate::pricing::Pricer;
use crate::risk::RiskMeasure;

/// Basis for [`TransactionCostModel::Scaled`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CostScaling {
    /// Scale by the instrument's unsigned economic notional (`|notional| * rate`).
    Notional,
    /// Scale by the instrument's unsigned contract quantity (`|quantity| * rate`).
    Quantity,
    /// Scale by the absolute value of a computed [`RiskMeasure`] (`|measure| * rate`, e.g. 1% of `|Price|`).
    RiskMeasure(RiskMeasure),
}

/// Aggregation operator for [`TransactionCostModel::Aggregate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TransactionAggType {
    /// Sum of constituent models (e.g. flat fee + bps of notional).
    #[default]
    Sum,
    /// Maximum of constituent models (e.g. `max(min_ticket_fee, bps_fee)`).
    Max,
    /// Minimum of constituent models (e.g. capped fee).
    Min,
}

/// Entry or exit transaction cost model applied per trade event.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TransactionCostModel {
    /// Zero transaction costs.
    #[default]
    Zero,
    /// Fixed flat dollar fee per trade.
    Constant(f64),
    /// Proportional cost scaled by notional, quantity, or a risk measure (e.g. option premium).
    Scaled {
        /// Scaling metric.
        scaling: CostScaling,
        /// Multiplier applied to the metric (`0.0001` = 1bp, `0.01` = 1%).
        rate: f64,
    },
    /// Combines multiple cost models via `Sum`, `Max`, or `Min`.
    Aggregate {
        /// Sub-models to evaluate.
        models: Vec<TransactionCostModel>,
        /// Aggregation rule.
        agg_type: TransactionAggType,
    },
}

impl TransactionCostModel {
    /// Fixed per-trade cost.
    #[must_use]
    pub fn constant(amount: f64) -> Self {
        Self::Constant(amount)
    }

    /// Cost proportional to notional (`|notional| * rate`).
    #[must_use]
    pub fn notional_bps(rate: f64) -> Self {
        Self::Scaled {
            scaling: CostScaling::Notional,
            rate,
        }
    }

    /// Cost proportional to unsigned contract quantity (`|quantity| * rate`).
    #[must_use]
    pub fn per_unit(rate: f64) -> Self {
        Self::Scaled {
            scaling: CostScaling::Quantity,
            rate,
        }
    }

    /// Cost proportional to `|measure|` (`|measure_value| * rate`).
    #[must_use]
    pub fn by_measure(measure: RiskMeasure, rate: f64) -> Self {
        Self::Scaled {
            scaling: CostScaling::RiskMeasure(measure),
            rate,
        }
    }

    /// Combines sub-models using `agg_type`.
    #[must_use]
    pub fn aggregate(models: Vec<TransactionCostModel>, agg_type: TransactionAggType) -> Self {
        Self::Aggregate { models, agg_type }
    }

    /// Evaluates the non-negative cost magnitude for `instrument` on `market`.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError`] if a [`CostScaling::RiskMeasure`] lookup fails to price.
    pub fn compute_cost(
        &self,
        instrument: &Instrument,
        market: &MarketSnapshot,
        pricer: &dyn Pricer,
    ) -> Result<f64, QuantError> {
        match self {
            Self::Zero => Ok(0.0),
            Self::Constant(fee) => Ok(fee.abs()),
            Self::Scaled { scaling, rate } => {
                let base = match scaling {
                    CostScaling::Notional => instrument.notional_amount(market),
                    CostScaling::Quantity => instrument.signed_size().abs(),
                    CostScaling::RiskMeasure(measure) => {
                        pricer.calc(instrument, market, measure)?.value.abs()
                    }
                };
                Ok((base * rate).abs())
            }
            Self::Aggregate { models, agg_type } => {
                if models.is_empty() {
                    return Ok(0.0);
                }
                let mut vals = Vec::with_capacity(models.len());
                for m in models {
                    vals.push(m.compute_cost(instrument, market, pricer)?);
                }
                let agg = match agg_type {
                    TransactionAggType::Sum => vals.into_iter().sum(),
                    TransactionAggType::Max => vals.into_iter().fold(0.0_f64, f64::max),
                    TransactionAggType::Min => vals.into_iter().fold(f64::INFINITY, f64::min),
                };
                Ok(agg)
            }
        }
    }
}
