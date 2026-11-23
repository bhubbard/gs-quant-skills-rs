//! Stateful backtesting engine (`GenericEngine`), triggers, actions, and P&L attribution.

use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::costs::TransactionCostModel;
use crate::error::QuantError;
use crate::instrument::Instrument;
use crate::market::{MarketHistory, MarketSnapshot, MissingDataStrategy};
use crate::pricing::Pricer;
use crate::risk::RiskMeasure;
use crate::stats::SummaryStats;
use crate::tenor::{business_day_schedule, Tenor};

/// Comparison direction for market, risk, and position-count triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TriggerDirection {
    /// Fires when the observed value is strictly greater than the threshold (`>`).
    Above,
    /// Fires when the observed value is strictly less than the threshold (`<`).
    Below,
    /// Fires when the observed value equals the threshold within floating-point tolerance (`==`).
    Equal,
}

impl TriggerDirection {
    fn check(self, value: f64, threshold: f64) -> bool {
        match self {
            Self::Above => value > threshold,
            Self::Below => value < threshold,
            Self::Equal => (value - threshold).abs() <= 1e-9,
        }
    }
}

/// Logical combination operator for [`TriggerCondition::Aggregate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AggregateMode {
    /// Logical AND: all child conditions must fire on the evaluation date.
    AllOf,
    /// Logical OR: at least one child condition must fire on the evaluation date.
    AnyOf,
}

/// Condition determining *when* a [`Trigger`] executes its [`Action`] sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TriggerCondition {
    /// Fires on a periodic tenor schedule between `start_date` and `end_date`.
    Periodic {
        /// First date on which the trigger can fire.
        start_date: NaiveDate,
        /// Last date on which the trigger can fire.
        end_date: NaiveDate,
        /// Schedule spacing (`1b`, `1w`, `1m`, etc.).
        frequency: Tenor,
        /// Optional holiday dates excluded from this trigger's schedule.
        calendar: BTreeSet<NaiveDate>,
    },
    /// Fires on an explicit list of calendar dates.
    Dates(Vec<NaiveDate>),
    /// Fires when a named market data series crosses `level` in `direction`.
    Market {
        /// Key of the [`crate::market::DataSeries`] registered in [`MarketHistory`].
        series_name: String,
        /// Trigger threshold level.
        level: f64,
        /// Comparison direction (`Above`, `Below`, `Equal`).
        direction: TriggerDirection,
        /// Handling when `date` is absent in the series.
        missing_strategy: MissingDataStrategy,
    },
    /// Fires when the aggregate portfolio [`RiskMeasure`] crosses `level` in `direction`.
    StrategyRisk {
        /// Portfolio risk measure to monitor (computed before trigger evaluation).
        measure: RiskMeasure,
        /// Trigger threshold level.
        level: f64,
        /// Comparison direction (`Above`, `Below`, `Equal`).
        direction: TriggerDirection,
    },
    /// Mean-reversion z-score trigger tracking flat (`0`), long (`+1`), or short (`-1`) state.
    ///
    /// Mirrors `gs_quant.backtests.triggers.MeanReversionTriggerRequirements`:
    /// - When flat and `|(price - rolling_mean) / rolling_std| > z_score_bound`, enters short (`-1`)
    ///   if `price > rolling_mean` or long (`+1`) if `price < rolling_mean`.
    /// - When long (`+1`) and `price > rolling_mean`, exits back to flat (`0`).
    /// - When short (`-1`) and `price < rolling_mean`, exits back to flat (`0`).
    MeanReversion {
        /// Key of the [`crate::market::DataSeries`] in [`MarketHistory`].
        series_name: String,
        /// Entry z-score magnitude threshold (e.g. `2.0`).
        z_score_bound: f64,
        /// Number of trailing observations used for the rolling mean (e.g. `30`).
        rolling_mean_window: usize,
        /// Number of trailing observations used for the rolling sample std dev (e.g. `30`).
        rolling_std_window: usize,
    },
    /// Fires based on the number of currently open positions (open-position cap / `PortfolioTrigger`).
    ///
    /// Combine `OpenPositionCount { limit: max_positions, direction: TriggerDirection::Below }`
    /// with an entry condition inside [`TriggerCondition::Aggregate`] (`AggregateMode::AllOf`)
    /// to block new orders once the book is at capacity.
    OpenPositionCount {
        /// Open position count threshold.
        limit: usize,
        /// Comparison direction against `limit`.
        direction: TriggerDirection,
    },
    /// Boolean AND/OR combination of nested [`TriggerCondition`] values.
    Aggregate {
        /// Child conditions.
        conditions: Vec<TriggerCondition>,
        /// `AllOf` (AND) or `AnyOf` (OR).
        mode: AggregateMode,
    },
    /// Logical negation of a child [`TriggerCondition`].
    Not(Box<TriggerCondition>),
}

/// Specifies when an entered position should automatically close in [`GenericEngine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TradeDuration {
    /// Held until an explicit exit action closes it or the backtest ends.
    #[default]
    HoldUntilExit,
    /// Closed `Tenor` after trade entry date (adjusted by action `calendar`).
    Tenor(Tenor),
    /// Closed on a fixed calendar date.
    FixedDate(NaiveDate),
    /// Closed on the resolved expiration or termination date of the instrument.
    InstrumentExpiry,
    /// Closed on the triggering schedule's next fire date (for `Periodic` or `Dates` triggers).
    NextSchedule,
}

/// Position-sizing rule for [`Action::AddScaledTrade`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ScalingType {
    /// Multiply the template instrument size by `scaling_level`.
    Size,
    /// Scale so the trade starts with `scaling_level` units of the specified [`RiskMeasure`]:
    /// `scale = scaling_level / unit_risk`.
    RiskMeasure(RiskMeasure),
    /// Scale using available cash/NAV proceeds (`scaling_level` on first trade, then closed-trade
    /// proceeds plus initial allocation thereafter).
    Nav,
}

/// Action executed sequentially when its enclosing [`Trigger`] fires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Action {
    /// Adds a resolved instrument trade to the portfolio.
    AddTrade {
        /// Template instrument (resolved on the trade date).
        instrument: Instrument,
        /// Closing schedule for the entered trade.
        duration: TradeDuration,
        /// Optional name prefix overriding the template instrument's name.
        name: Option<String>,
        /// Transaction cost charged when opening the position.
        entry_cost: TransactionCostModel,
        /// Transaction cost charged when closing the position.
        exit_cost: TransactionCostModel,
        /// Holiday calendar applied to [`TradeDuration::Tenor`] end-date calculation.
        calendar: BTreeSet<NaiveDate>,
    },
    /// Adds a trade scaled by size, target risk measure, or available NAV.
    AddScaledTrade {
        /// Template instrument.
        instrument: Instrument,
        /// Closing schedule for the entered trade.
        duration: TradeDuration,
        /// Scaling methodology (`Size`, `RiskMeasure`, `Nav`).
        scaling_type: ScalingType,
        /// Target size multiplier, risk amount, or initial NAV allocation.
        scaling_level: f64,
        /// Optional name prefix.
        name: Option<String>,
        /// Transaction cost charged on entry.
        entry_cost: TransactionCostModel,
        /// Transaction cost charged on exit.
        exit_cost: TransactionCostModel,
        /// Holiday calendar applied to [`TradeDuration::Tenor`].
        calendar: BTreeSet<NaiveDate>,
    },
    /// Enters a hedge trade offsetting the portfolio's aggregate exposure to `risk`.
    Hedge {
        /// Portfolio risk measure to neutralize (e.g. `RiskMeasure::FxDelta`).
        risk: RiskMeasure,
        /// Template hedging instrument (scaled by `-portfolio_risk / hedge_unit_risk`).
        hedge_instrument: Instrument,
        /// Closing schedule for the hedge trade.
        duration: TradeDuration,
        /// Optional name prefix.
        name: Option<String>,
        /// Transaction cost charged on entry.
        entry_cost: TransactionCostModel,
        /// Transaction cost charged on exit.
        exit_cost: TransactionCostModel,
    },
    /// Exits all currently open positions whose base name starts with `name`.
    ExitTrade {
        /// Instrument name or prefix to close.
        name: String,
    },
    /// Exits every open position in the portfolio.
    ExitAllPositions,
}

/// Pairs a [`TriggerCondition`] with an ordered list of [`Action`] items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trigger {
    /// Condition evaluated on each simulation date.
    pub condition: TriggerCondition,
    /// Ordered actions run whenever `condition` fires.
    pub actions: Vec<Action>,
}

impl Trigger {
    /// Creates a new [`Trigger`].
    #[must_use]
    pub fn new(condition: TriggerCondition, actions: Vec<Action>) -> Self {
        Self { condition, actions }
    }
}

/// Complete strategy specification: optional initial portfolio plus ordered triggers.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Strategy {
    /// Positions held at the start of the backtest (resolved on the first simulation date).
    pub initial_portfolio: Vec<Instrument>,
    /// Triggers evaluated in order on every simulation date.
    pub triggers: Vec<Trigger>,
}

impl Strategy {
    /// Creates a new [`Strategy`].
    #[must_use]
    pub fn new(initial_portfolio: Vec<Instrument>, triggers: Vec<Trigger>) -> Self {
        Self {
            initial_portfolio,
            triggers,
        }
    }
}

/// Order of a [`PnlAttribute`] Taylor term.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AttributionOrder {
    /// Linear term: `scaling_factor * sensitivity * move`.
    FirstOrder,
    /// Quadratic term: `0.5 * scaling_factor * sensitivity * move^2`.
    SecondOrder,
}

/// Named sensitivity-times-market-move term for P&L attribution (`PnlExplain`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PnlAttribute {
    /// Column name (e.g. `"PNL_delta"`, `"PNL_gamma"`, `"VegaPnL"`).
    pub name: String,
    /// Sensitivity risk measure computed on day `t - 1`.
    pub sensitivity_measure: RiskMeasure,
    /// Market quantity whose daily difference drives the move (`dM = M_t - M_{t-1}`).
    pub market_data_measure: RiskMeasure,
    /// Scaling factor matching unit conventions (e.g. `100.0` for `FxVega` vs decimal `FxImpliedVol`).
    pub scaling_factor: f64,
    /// First-order or second-order Taylor term.
    pub order: AttributionOrder,
}

/// Collection of [`PnlAttribute`] terms decomposing daily P&L.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PnlDefinition {
    /// Attribution components.
    pub attributes: Vec<PnlAttribute>,
}

/// Standard FX option P&L Explain definition (`PNL_delta`, `PNL_gamma`, `VegaPnL`),
/// matching `gs_quant.backtests.backtest_objects.fx_pnl_definition`.
///
/// Remember that delta + gamma + vega omit theta/time decay and higher-order cross terms,
/// leaving a non-zero `unexplained` residual in [`PnlExplainRow`].
#[must_use]
pub fn fx_pnl_definition() -> PnlDefinition {
    PnlDefinition {
        attributes: vec![
            PnlAttribute {
                name: "PNL_delta".to_owned(),
                sensitivity_measure: RiskMeasure::FxDelta,
                market_data_measure: RiskMeasure::FxSpot,
                scaling_factor: 1.0,
                order: AttributionOrder::FirstOrder,
            },
            PnlAttribute {
                name: "PNL_gamma".to_owned(),
                sensitivity_measure: RiskMeasure::FxGamma,
                market_data_measure: RiskMeasure::FxSpot,
                scaling_factor: 1.0,
                order: AttributionOrder::SecondOrder,
            },
            PnlAttribute {
                name: "VegaPnL".to_owned(),
                sensitivity_measure: RiskMeasure::FxVega,
                market_data_measure: RiskMeasure::FxImpliedVol,
                scaling_factor: 100.0,
                order: AttributionOrder::FirstOrder,
            },
        ],
    }
}

/// Standard equity option P&L Explain definition (`PNL_delta`, `PNL_gamma`, `VegaPnL`).
///
/// Accounts for `gs_quant`'s 100%-move convention on `EqDelta` and `EqGamma` when paired
/// with raw `EqSpot` changes via `EqSpot` fractionalization or raw bump equivalence;
/// here `EqVega` pairs with `EqImpliedVol * 100.0`.
#[must_use]
pub fn eq_pnl_definition() -> PnlDefinition {
    PnlDefinition {
        attributes: vec![PnlAttribute {
            name: "VegaPnL".to_owned(),
            sensitivity_measure: RiskMeasure::EqVega,
            market_data_measure: RiskMeasure::EqImpliedVol,
            scaling_factor: 100.0,
            order: AttributionOrder::FirstOrder,
        }],
    }
}

/// Configuration for [`GenericEngine::run`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestConfig {
    /// Backtest start date (inclusive).
    pub start: NaiveDate,
    /// Backtest end date (inclusive).
    pub end: NaiveDate,
    /// Evaluation schedule frequency (`1b` recommended for daily P&L).
    pub frequency: Tenor,
    /// Optional explicit list of evaluation dates (overrides `start`/`end`/`frequency` when `Some`).
    pub states: Option<Vec<NaiveDate>>,
    /// Price measure reported as `Price` (typically [`RiskMeasure::DollarPrice`] or [`RiskMeasure::Price`]).
    pub price_measure: RiskMeasure,
    /// Additional risk measures calculated and stored per date.
    pub risks: Vec<RiskMeasure>,
    /// Starting cash value (`0.0` by default).
    pub initial_value: f64,
    /// Holiday dates excluded when building the evaluation schedule.
    pub holiday_calendar: BTreeSet<NaiveDate>,
    /// Missing market data handling strategy.
    pub missing_strategy: MissingDataStrategy,
    /// Optional P&L attribution definition (`fx_pnl_definition()`, etc.).
    pub pnl_explain: Option<PnlDefinition>,
    /// Business-day annualisation factor used by `summary_stats` (`252` default).
    pub annualisation_factor: u32,
}

impl BacktestConfig {
    /// Creates a daily (`1b`) configuration over `[start, end]`.
    #[must_use]
    pub fn daily(start: NaiveDate, end: NaiveDate) -> Self {
        Self {
            start,
            end,
            frequency: Tenor::BusinessDays(1),
            states: None,
            price_measure: RiskMeasure::DollarPrice,
            risks: Vec::new(),
            initial_value: 0.0,
            holiday_calendar: BTreeSet::new(),
            missing_strategy: MissingDataStrategy::FillForward,
            pnl_explain: None,
            annualisation_factor: 252,
        }
    }
}

/// One row of the daily `result_summary` table (`Price`, `Cumulative Cash`, `Transaction Costs`, `Total`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultSummaryRow {
    /// Valuation date.
    pub date: NaiveDate,
    /// Mark-to-market value of live open positions.
    pub price: f64,
    /// Cumulative cash from initial balance, entry payments, and closed-trade proceeds.
    pub cumulative_cash: f64,
    /// Cumulative transaction costs (`<= 0.0`).
    pub transaction_costs: f64,
    /// Total strategy value: `price + cumulative_cash + transaction_costs`.
    pub total: f64,
    /// Additional computed risk measures keyed by [`RiskMeasure::name`].
    pub risks: BTreeMap<String, f64>,
}

/// Record of an entered (and optionally closed) trade in the backtest ledger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeLedgerEntry {
    /// Sequential trade ID.
    pub trade_id: u64,
    /// Instrument name at entry.
    pub instrument_name: String,
    /// Date the trade was opened.
    pub open_date: NaiveDate,
    /// Signed size (`notional` or `quantity * multiplier`).
    pub signed_size: f64,
    /// Entry mark-to-market value paid (`entry_pv`).
    pub entry_value: f64,
    /// Entry transaction cost magnitude (`>= 0.0`).
    pub entry_cost: f64,
    /// Date the trade was closed (`None` if still open at backtest end).
    pub close_date: Option<NaiveDate>,
    /// Exit mark-to-market value received (`None` if still open).
    pub exit_value: Option<f64>,
    /// Exit transaction cost magnitude (`None` if still open).
    pub exit_cost: Option<f64>,
    /// Net realized P&L (`(exit_value - entry_value) - entry_cost - exit_cost`) when closed.
    pub realized_pnl: Option<f64>,
}

/// Cumulative P&L attribution breakdown for a single date.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PnlExplainRow {
    /// Valuation date.
    pub date: NaiveDate,
    /// Cumulative attributed P&L per [`PnlAttribute::name`].
    pub attributes: BTreeMap<String, f64>,
    /// Cumulative residual (`(Total_t - Total_0) - sum(attributes)`), capturing theta/cross-terms.
    pub unexplained: f64,
    /// Cumulative total P&L change relative to day 0 (`Total_t - Total_0`).
    pub total_pnl_change: f64,
}

/// Complete output of [`GenericEngine::run`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestResult {
    /// Daily P&L and risk time series.
    pub result_summary: Vec<ResultSummaryRow>,
    /// Full trade entry/exit ledger.
    pub trade_ledger: Vec<TradeLedgerEntry>,
    /// Optional cumulative P&L Explain rows (`Some` when `pnl_explain` was configured).
    pub pnl_explain: Option<Vec<PnlExplainRow>>,
    /// Annualisation factor configured for summary calculations.
    pub annualisation_factor: u32,
}

impl BacktestResult {
    /// Computes all 19 Goldman Sachs summary statistics plus closed-trade diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError`] if the backtest has no summary rows.
    pub fn summary_stats(&self) -> Result<SummaryStats, QuantError> {
        SummaryStats::compute(
            &self.result_summary,
            &self.trade_ledger,
            self.annualisation_factor,
        )
    }
}

/// Stateful multi-asset backtest engine mirroring `gs_quant.backtests.generic_engine.GenericEngine`.
#[derive(Debug, Clone, Copy, Default)]
pub struct GenericEngine;

#[derive(Debug, Clone)]
struct OpenPosition {
    trade_id: u64,
    base_name: String,
    instrument: Instrument,
    scheduled_close: Option<NaiveDate>,
    exit_cost_model: TransactionCostModel,
    mean_reversion_tag: Option<(usize, i8)>,
    action_idx: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
enum TriggerSignal {
    Standard { next_schedule: Option<NaiveDate> },
    MeanReversionEntry { trigger_idx: usize, sign: i8 },
    MeanReversionExit { trigger_idx: usize, exited_sign: i8 },
}

struct TriggerRuntimeState {
    periodic_dates: Option<Vec<NaiveDate>>,
    mean_reversion_pos: i8,
}

impl GenericEngine {
    /// Runs `strategy` over the configured schedule against `history` and `pricer`.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError`] on invalid dates/tenors, missing market data, or zero risk sensitivity
    /// during risk-scaled sizing.
    pub fn run(
        &self,
        strategy: &Strategy,
        config: &BacktestConfig,
        history: &MarketHistory,
        pricer: &dyn Pricer,
    ) -> Result<BacktestResult, QuantError> {
        let dates = match &config.states {
            Some(explicit) if !explicit.is_empty() => explicit.clone(),
            Some(_) => {
                return Err(QuantError::empty_series(
                    "explicit backtest `states` list cannot be empty",
                ));
            }
            None => business_day_schedule(
                config.start,
                config.end,
                config.frequency,
                &config.holiday_calendar,
            )?,
        };
        if dates.is_empty() {
            return Err(QuantError::empty_series(
                "backtest schedule produced zero dates",
            ));
        }

        // Precompute periodic schedules and initialize per-trigger state.
        let mut trigger_states: Vec<TriggerRuntimeState> = strategy
            .triggers
            .iter()
            .map(|t| TriggerRuntimeState {
                periodic_dates: precompute_schedule(&t.condition),
                mean_reversion_pos: 0,
            })
            .collect();

        // Collect all risk measures needed for StrategyRisk triggers, Hedges, and P&L Explain.
        let mut open_positions: Vec<OpenPosition> = Vec::new();
        let mut ledger: Vec<TradeLedgerEntry> = Vec::new();
        let mut next_trade_id = 1_u64;
        let mut cumulative_cash = config.initial_value;
        let mut cumulative_costs = 0.0_f64; // negative or zero per gs_quant
        let mut action_last_proceeds: BTreeMap<usize, f64> = BTreeMap::new();

        // Open initial portfolio on day 0.
        let first_snap = history.snapshot(dates[0], config.missing_strategy)?;
        for inst in &strategy.initial_portfolio {
            let resolved = inst.resolve(dates[0], &first_snap, &config.holiday_calendar)?;
            let entry_pv = pricer
                .calc(&resolved, &first_snap, &config.price_measure)?
                .value;
            cumulative_cash -= entry_pv;
            let trade_id = next_trade_id;
            next_trade_id += 1;
            let name = resolved.name().to_owned();
            ledger.push(TradeLedgerEntry {
                trade_id,
                instrument_name: name.clone(),
                open_date: dates[0],
                signed_size: resolved.signed_size(),
                entry_value: entry_pv,
                entry_cost: 0.0,
                close_date: None,
                exit_value: None,
                exit_cost: None,
                realized_pnl: None,
            });
            open_positions.push(OpenPosition {
                trade_id,
                base_name: name,
                instrument: resolved,
                scheduled_close: None,
                exit_cost_model: TransactionCostModel::Zero,
                mean_reversion_tag: None,
                action_idx: None,
            });
        }

        let mut summary_rows = Vec::with_capacity(dates.len());
        let mut explain_rows = config
            .pnl_explain
            .as_ref()
            .map(|_| Vec::with_capacity(dates.len()));
        let mut cum_explain: BTreeMap<String, f64> = BTreeMap::new();
        if let Some(ref def) = config.pnl_explain {
            for attr in &def.attributes {
                cum_explain.insert(attr.name.clone(), 0.0);
            }
        }
        let mut prev_snap: Option<MarketSnapshot> = None;
        let mut prev_positions: Vec<Instrument> = Vec::new();
        let mut initial_total: Option<f64> = None;

        for (day_idx, &date) in dates.iter().enumerate() {
            let snap = history.snapshot(date, config.missing_strategy)?;

            // 1. P&L Explain uses end-of-day-(t-1) positions against market moves from (t-1) -> t.
            if let (Some(def), Some(prev_s)) = (&config.pnl_explain, &prev_snap) {
                for pos_inst in &prev_positions {
                    for attr in &def.attributes {
                        let sens = pricer
                            .calc(pos_inst, prev_s, &attr.sensitivity_measure)?
                            .value;
                        let m_prev = pricer
                            .calc(pos_inst, prev_s, &attr.market_data_measure)?
                            .value;
                        let m_curr = pricer
                            .calc(pos_inst, &snap, &attr.market_data_measure)?
                            .value;
                        let dm = m_curr - m_prev;
                        let contrib = match attr.order {
                            AttributionOrder::FirstOrder => attr.scaling_factor * sens * dm,
                            AttributionOrder::SecondOrder => {
                                0.5 * attr.scaling_factor * sens * dm * dm
                            }
                        };
                        if let Some(slot) = cum_explain.get_mut(&attr.name) {
                            *slot += contrib;
                        }
                    }
                }
            }

            // 2. Close positions whose scheduled_close <= date.
            let mut due_indices = Vec::new();
            for (idx, pos) in open_positions.iter().enumerate() {
                if pos.scheduled_close.is_some_and(|close_d| date >= close_d) {
                    due_indices.push(idx);
                }
            }
            for idx in due_indices.into_iter().rev() {
                let pos = open_positions.remove(idx);
                close_position(
                    pos,
                    date,
                    &snap,
                    &config.price_measure,
                    pricer,
                    &mut cumulative_cash,
                    &mut cumulative_costs,
                    &mut ledger,
                    &mut action_last_proceeds,
                )?;
            }

            // 3. Evaluate triggers in order and execute actions sequentially.
            let mut global_action_id = 0usize;
            for (trig_idx, trigger) in strategy.triggers.iter().enumerate() {
                let signal = evaluate_condition(
                    trig_idx,
                    &trigger.condition,
                    date,
                    &snap,
                    history,
                    pricer,
                    &open_positions,
                    &mut trigger_states[trig_idx],
                )?;

                if let Some(sig) = signal {
                    for action in &trigger.actions {
                        let aid = global_action_id;
                        global_action_id += 1;
                        execute_action(
                            action,
                            aid,
                            sig,
                            date,
                            &snap,
                            &config.price_measure,
                            &config.holiday_calendar,
                            pricer,
                            &mut open_positions,
                            &mut ledger,
                            &mut next_trade_id,
                            &mut cumulative_cash,
                            &mut cumulative_costs,
                            &mut action_last_proceeds,
                        )?;
                    }
                } else {
                    global_action_id += trigger.actions.len();
                }
            }

            // 4. Value live open portfolio and requested risk measures at end of day.
            let mut live_price = 0.0;
            for pos in &open_positions {
                live_price += pricer
                    .calc(&pos.instrument, &snap, &config.price_measure)?
                    .value;
            }
            let total = live_price + cumulative_cash + cumulative_costs;

            let mut risk_map = BTreeMap::new();
            for measure in &config.risks {
                let val = if measure.is_market_quote() {
                    if let Some(first) = open_positions.first() {
                        pricer.calc(&first.instrument, &snap, measure)?.value
                    } else {
                        0.0
                    }
                } else {
                    let mut sum = 0.0;
                    for pos in &open_positions {
                        sum += pricer.calc(&pos.instrument, &snap, measure)?.value;
                    }
                    sum
                };
                risk_map.insert(measure.name().to_owned(), val);
            }

            summary_rows.push(ResultSummaryRow {
                date,
                price: live_price,
                cumulative_cash,
                transaction_costs: cumulative_costs,
                total,
                risks: risk_map,
            });

            let base_total = *initial_total.get_or_insert(total);
            if let Some(ref mut rows) = explain_rows {
                let total_change = if day_idx == 0 {
                    0.0
                } else {
                    total - base_total
                };
                let explained_sum: f64 = cum_explain.values().sum();
                let unexplained = total_change - explained_sum;
                rows.push(PnlExplainRow {
                    date,
                    attributes: cum_explain.clone(),
                    unexplained,
                    total_pnl_change: total_change,
                });
            }

            prev_snap = Some(snap);
            prev_positions = open_positions
                .iter()
                .map(|p| p.instrument.clone())
                .collect();
        }

        Ok(BacktestResult {
            result_summary: summary_rows,
            trade_ledger: ledger,
            pnl_explain: explain_rows,
            annualisation_factor: config.annualisation_factor,
        })
    }
}

fn precompute_schedule(condition: &TriggerCondition) -> Option<Vec<NaiveDate>> {
    match condition {
        TriggerCondition::Periodic {
            start_date,
            end_date,
            frequency,
            calendar,
        } => business_day_schedule(*start_date, *end_date, *frequency, calendar).ok(),
        TriggerCondition::Dates(dates) => {
            let mut sorted = dates.clone();
            sorted.sort_unstable();
            sorted.dedup();
            Some(sorted)
        }
        TriggerCondition::Aggregate { conditions, .. } => {
            conditions.iter().find_map(precompute_schedule)
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn evaluate_condition(
    trig_idx: usize,
    condition: &TriggerCondition,
    date: NaiveDate,
    snap: &MarketSnapshot,
    history: &MarketHistory,
    pricer: &dyn Pricer,
    open_positions: &[OpenPosition],
    state: &mut TriggerRuntimeState,
) -> Result<Option<TriggerSignal>, QuantError> {
    match condition {
        TriggerCondition::Periodic { .. } | TriggerCondition::Dates(_) => {
            let Some(ref sched) = state.periodic_dates else {
                return Ok(None);
            };
            if let Ok(pos) = sched.binary_search(&date) {
                let next = sched.get(pos + 1).copied();
                Ok(Some(TriggerSignal::Standard {
                    next_schedule: next,
                }))
            } else {
                Ok(None)
            }
        }
        TriggerCondition::Market {
            series_name,
            level,
            direction,
            missing_strategy,
        } => {
            let Some(series) = history.series(series_name) else {
                return Err(QuantError::missing_market_data(date, series_name.clone()));
            };
            let Some(obs) = series.get_with_strategy(date, *missing_strategy) else {
                return if *missing_strategy == MissingDataStrategy::Fail {
                    Err(QuantError::missing_market_data(date, series_name.clone()))
                } else {
                    Ok(None)
                };
            };
            if direction.check(obs, *level) {
                Ok(Some(TriggerSignal::Standard {
                    next_schedule: None,
                }))
            } else {
                Ok(None)
            }
        }
        TriggerCondition::StrategyRisk {
            measure,
            level,
            direction,
        } => {
            let mut total_risk = 0.0;
            for pos in open_positions {
                total_risk += pricer.calc(&pos.instrument, snap, measure)?.value;
            }
            if direction.check(total_risk, *level) {
                Ok(Some(TriggerSignal::Standard {
                    next_schedule: None,
                }))
            } else {
                Ok(None)
            }
        }
        TriggerCondition::MeanReversion {
            series_name,
            z_score_bound,
            rolling_mean_window,
            rolling_std_window,
        } => {
            let Some(series) = history.series(series_name) else {
                return Err(QuantError::missing_market_data(date, series_name.clone()));
            };
            let Some(current_price) =
                series.get_with_strategy(date, MissingDataStrategy::FillForward)
            else {
                return Ok(None);
            };
            let mean_vals = series.trailing_window(date, *rolling_mean_window);
            let std_vals = series.trailing_window(date, *rolling_std_window);
            if mean_vals.len() < *rolling_mean_window || std_vals.len() < *rolling_std_window {
                return Ok(None);
            }
            let rolling_mean = mean_vals.iter().sum::<f64>() / (mean_vals.len() as f64);
            let std_mean = std_vals.iter().sum::<f64>() / (std_vals.len() as f64);
            let rolling_std = if std_vals.len() < 2 {
                0.0
            } else {
                let ss = std_vals.iter().map(|x| (x - std_mean).powi(2)).sum::<f64>();
                (ss / ((std_vals.len() - 1) as f64)).sqrt()
            };
            if rolling_std <= 1e-12 {
                return Ok(None);
            }

            if state.mean_reversion_pos == 0 {
                let z = (current_price - rolling_mean) / rolling_std;
                if z.abs() > *z_score_bound {
                    let sign = if current_price > rolling_mean { -1 } else { 1 };
                    state.mean_reversion_pos = sign;
                    return Ok(Some(TriggerSignal::MeanReversionEntry {
                        trigger_idx: trig_idx,
                        sign,
                    }));
                }
            } else if state.mean_reversion_pos == 1 && current_price > rolling_mean {
                state.mean_reversion_pos = 0;
                return Ok(Some(TriggerSignal::MeanReversionExit {
                    trigger_idx: trig_idx,
                    exited_sign: 1,
                }));
            } else if state.mean_reversion_pos == -1 && current_price < rolling_mean {
                state.mean_reversion_pos = 0;
                return Ok(Some(TriggerSignal::MeanReversionExit {
                    trigger_idx: trig_idx,
                    exited_sign: -1,
                }));
            }
            Ok(None)
        }
        TriggerCondition::OpenPositionCount { limit, direction } => {
            if direction.check(open_positions.len() as f64, *limit as f64) {
                Ok(Some(TriggerSignal::Standard {
                    next_schedule: None,
                }))
            } else {
                Ok(None)
            }
        }
        TriggerCondition::Aggregate { conditions, mode } => {
            if conditions.is_empty() {
                return Ok(None);
            }
            let mut selected_signal: Option<TriggerSignal> = None;
            match mode {
                AggregateMode::AllOf => {
                    for child in conditions {
                        let res = evaluate_condition(
                            trig_idx,
                            child,
                            date,
                            snap,
                            history,
                            pricer,
                            open_positions,
                            state,
                        )?;
                        let Some(sig) = res else {
                            return Ok(None);
                        };
                        if selected_signal.is_none()
                            || !matches!(
                                sig,
                                TriggerSignal::Standard {
                                    next_schedule: None
                                }
                            )
                        {
                            selected_signal = Some(sig);
                        }
                    }
                    Ok(selected_signal)
                }
                AggregateMode::AnyOf => {
                    for child in conditions {
                        let res = evaluate_condition(
                            trig_idx,
                            child,
                            date,
                            snap,
                            history,
                            pricer,
                            open_positions,
                            state,
                        )?;
                        if res.is_some() {
                            return Ok(res);
                        }
                    }
                    Ok(None)
                }
            }
        }
        TriggerCondition::Not(inner) => {
            let res = evaluate_condition(
                trig_idx,
                inner,
                date,
                snap,
                history,
                pricer,
                open_positions,
                state,
            )?;
            if res.is_none() {
                Ok(Some(TriggerSignal::Standard {
                    next_schedule: None,
                }))
            } else {
                Ok(None)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_action(
    action: &Action,
    action_idx: usize,
    signal: TriggerSignal,
    date: NaiveDate,
    snap: &MarketSnapshot,
    price_measure: &RiskMeasure,
    engine_holidays: &BTreeSet<NaiveDate>,
    pricer: &dyn Pricer,
    open_positions: &mut Vec<OpenPosition>,
    ledger: &mut Vec<TradeLedgerEntry>,
    next_trade_id: &mut u64,
    cumulative_cash: &mut f64,
    cumulative_costs: &mut f64,
    action_last_proceeds: &mut BTreeMap<usize, f64>,
) -> Result<(), QuantError> {
    match action {
        Action::AddTrade {
            instrument,
            duration,
            name,
            entry_cost,
            exit_cost,
            calendar,
        } => {
            if let TriggerSignal::MeanReversionExit {
                trigger_idx,
                exited_sign,
            } = signal
            {
                close_matching_mean_reversion(
                    trigger_idx,
                    exited_sign,
                    date,
                    snap,
                    price_measure,
                    pricer,
                    open_positions,
                    cumulative_cash,
                    cumulative_costs,
                    ledger,
                    action_last_proceeds,
                )?;
                return Ok(());
            }

            let base_name = name.clone().unwrap_or_else(|| instrument.name().to_owned());
            let mut inst = instrument.clone().with_name(format!("{base_name}_{date}"));
            let mut mr_tag = None;
            if let TriggerSignal::MeanReversionEntry { trigger_idx, sign } = signal {
                inst = inst.with_scale(f64::from(sign));
                mr_tag = Some((trigger_idx, sign));
            }
            let merged_holidays = merge_holidays(engine_holidays, calendar);
            let resolved = inst.resolve(date, snap, &merged_holidays)?;
            let close_date =
                compute_close_date(duration, date, &resolved, signal, &merged_holidays);

            open_new_position(
                base_name,
                resolved,
                close_date,
                date,
                snap,
                price_measure,
                pricer,
                entry_cost,
                exit_cost.clone(),
                mr_tag,
                Some(action_idx),
                open_positions,
                ledger,
                next_trade_id,
                cumulative_cash,
                cumulative_costs,
            )?;
        }
        Action::AddScaledTrade {
            instrument,
            duration,
            scaling_type,
            scaling_level,
            name,
            entry_cost,
            exit_cost,
            calendar,
        } => {
            if let TriggerSignal::MeanReversionExit {
                trigger_idx,
                exited_sign,
            } = signal
            {
                close_matching_mean_reversion(
                    trigger_idx,
                    exited_sign,
                    date,
                    snap,
                    price_measure,
                    pricer,
                    open_positions,
                    cumulative_cash,
                    cumulative_costs,
                    ledger,
                    action_last_proceeds,
                )?;
                return Ok(());
            }

            let base_name = name.clone().unwrap_or_else(|| instrument.name().to_owned());
            let merged_holidays = merge_holidays(engine_holidays, calendar);
            let unit_resolved = instrument
                .clone()
                .with_name(format!("{base_name}_{date}"))
                .resolve(date, snap, &merged_holidays)?;

            let mut scale_factor = match scaling_type {
                ScalingType::Size => *scaling_level,
                ScalingType::RiskMeasure(measure) => {
                    let unit_risk = pricer.calc(&unit_resolved, snap, measure)?.value;
                    if unit_risk.abs() <= 1e-12 {
                        return Err(QuantError::ZeroRiskSensitivity {
                            measure: measure.to_string(),
                            date,
                        });
                    }
                    scaling_level / unit_risk
                }
                ScalingType::Nav => {
                    let avail_cash = action_last_proceeds
                        .remove(&action_idx)
                        .unwrap_or(*scaling_level);
                    let unit_pv = pricer.calc(&unit_resolved, snap, price_measure)?.value;
                    if unit_pv.abs() <= 1e-12 {
                        return Err(QuantError::ZeroRiskSensitivity {
                            measure: price_measure.to_string(),
                            date,
                        });
                    }
                    avail_cash / unit_pv
                }
            };

            let mut mr_tag = None;
            if let TriggerSignal::MeanReversionEntry { trigger_idx, sign } = signal {
                scale_factor *= f64::from(sign);
                mr_tag = Some((trigger_idx, sign));
            }
            let scaled_inst = unit_resolved.with_scale(scale_factor);
            let close_date =
                compute_close_date(duration, date, &scaled_inst, signal, &merged_holidays);

            open_new_position(
                base_name,
                scaled_inst,
                close_date,
                date,
                snap,
                price_measure,
                pricer,
                entry_cost,
                exit_cost.clone(),
                mr_tag,
                Some(action_idx),
                open_positions,
                ledger,
                next_trade_id,
                cumulative_cash,
                cumulative_costs,
            )?;
        }
        Action::Hedge {
            risk,
            hedge_instrument,
            duration,
            name,
            entry_cost,
            exit_cost,
        } => {
            let mut current_risk = 0.0;
            for pos in open_positions.iter() {
                current_risk += pricer.calc(&pos.instrument, snap, risk)?.value;
            }
            if current_risk.abs() <= 1e-9 {
                return Ok(());
            }

            let base_name = name
                .clone()
                .unwrap_or_else(|| hedge_instrument.name().to_owned());
            let unit_hedge = hedge_instrument
                .clone()
                .with_name(format!("Scaled_{base_name}_{date}"))
                .resolve(date, snap, engine_holidays)?;
            let hedge_unit_risk = pricer.calc(&unit_hedge, snap, risk)?.value;
            if hedge_unit_risk.abs() <= 1e-12 {
                return Err(QuantError::ZeroRiskSensitivity {
                    measure: risk.to_string(),
                    date,
                });
            }
            let hedge_scale = -current_risk / hedge_unit_risk;
            let scaled_hedge = unit_hedge.with_scale(hedge_scale);
            let close_date =
                compute_close_date(duration, date, &scaled_hedge, signal, engine_holidays);

            open_new_position(
                base_name,
                scaled_hedge,
                close_date,
                date,
                snap,
                price_measure,
                pricer,
                entry_cost,
                exit_cost.clone(),
                None,
                Some(action_idx),
                open_positions,
                ledger,
                next_trade_id,
                cumulative_cash,
                cumulative_costs,
            )?;
        }
        Action::ExitTrade { name } => {
            let mut to_close = Vec::new();
            for (idx, pos) in open_positions.iter().enumerate() {
                if pos.base_name == *name || pos.instrument.name().starts_with(name) {
                    to_close.push(idx);
                }
            }
            for idx in to_close.into_iter().rev() {
                let pos = open_positions.remove(idx);
                close_position(
                    pos,
                    date,
                    snap,
                    price_measure,
                    pricer,
                    cumulative_cash,
                    cumulative_costs,
                    ledger,
                    action_last_proceeds,
                )?;
            }
        }
        Action::ExitAllPositions => {
            let draining = std::mem::take(open_positions);
            for pos in draining {
                close_position(
                    pos,
                    date,
                    snap,
                    price_measure,
                    pricer,
                    cumulative_cash,
                    cumulative_costs,
                    ledger,
                    action_last_proceeds,
                )?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn open_new_position(
    base_name: String,
    resolved: Instrument,
    close_date: Option<NaiveDate>,
    date: NaiveDate,
    snap: &MarketSnapshot,
    price_measure: &RiskMeasure,
    pricer: &dyn Pricer,
    entry_cost: &TransactionCostModel,
    exit_cost_model: TransactionCostModel,
    mean_reversion_tag: Option<(usize, i8)>,
    action_idx: Option<usize>,
    open_positions: &mut Vec<OpenPosition>,
    ledger: &mut Vec<TradeLedgerEntry>,
    next_trade_id: &mut u64,
    cumulative_cash: &mut f64,
    cumulative_costs: &mut f64,
) -> Result<(), QuantError> {
    let entry_pv = pricer.calc(&resolved, snap, price_measure)?.value;
    let cost = entry_cost.compute_cost(&resolved, snap, pricer)?;
    *cumulative_cash -= entry_pv;
    *cumulative_costs -= cost;

    let trade_id = *next_trade_id;
    *next_trade_id += 1;

    ledger.push(TradeLedgerEntry {
        trade_id,
        instrument_name: resolved.name().to_owned(),
        open_date: date,
        signed_size: resolved.signed_size(),
        entry_value: entry_pv,
        entry_cost: cost,
        close_date: None,
        exit_value: None,
        exit_cost: None,
        realized_pnl: None,
    });

    open_positions.push(OpenPosition {
        trade_id,
        base_name,
        instrument: resolved,
        scheduled_close: close_date,
        exit_cost_model,
        mean_reversion_tag,
        action_idx,
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn close_matching_mean_reversion(
    trigger_idx: usize,
    exited_sign: i8,
    date: NaiveDate,
    snap: &MarketSnapshot,
    price_measure: &RiskMeasure,
    pricer: &dyn Pricer,
    open_positions: &mut Vec<OpenPosition>,
    cumulative_cash: &mut f64,
    cumulative_costs: &mut f64,
    ledger: &mut [TradeLedgerEntry],
    action_last_proceeds: &mut BTreeMap<usize, f64>,
) -> Result<(), QuantError> {
    let mut matching = Vec::new();
    for (idx, pos) in open_positions.iter().enumerate() {
        if pos.mean_reversion_tag == Some((trigger_idx, exited_sign)) {
            matching.push(idx);
        }
    }
    for idx in matching.into_iter().rev() {
        let pos = open_positions.remove(idx);
        close_position(
            pos,
            date,
            snap,
            price_measure,
            pricer,
            cumulative_cash,
            cumulative_costs,
            ledger,
            action_last_proceeds,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn close_position(
    pos: OpenPosition,
    date: NaiveDate,
    snap: &MarketSnapshot,
    price_measure: &RiskMeasure,
    pricer: &dyn Pricer,
    cumulative_cash: &mut f64,
    cumulative_costs: &mut f64,
    ledger: &mut [TradeLedgerEntry],
    action_last_proceeds: &mut BTreeMap<usize, f64>,
) -> Result<(), QuantError> {
    let exit_pv = pricer.calc(&pos.instrument, snap, price_measure)?.value;
    let exit_cost = pos
        .exit_cost_model
        .compute_cost(&pos.instrument, snap, pricer)?;
    *cumulative_cash += exit_pv;
    *cumulative_costs -= exit_cost;

    if let Some(aid) = pos.action_idx {
        *action_last_proceeds.entry(aid).or_insert(0.0) += exit_pv - exit_cost;
    }

    if let Some(entry) = ledger.iter_mut().find(|e| e.trade_id == pos.trade_id) {
        entry.close_date = Some(date);
        entry.exit_value = Some(exit_pv);
        entry.exit_cost = Some(exit_cost);
        entry.realized_pnl = Some((exit_pv - entry.entry_value) - entry.entry_cost - exit_cost);
    }
    Ok(())
}

fn merge_holidays(a: &BTreeSet<NaiveDate>, b: &BTreeSet<NaiveDate>) -> BTreeSet<NaiveDate> {
    a.union(b).copied().collect()
}

fn compute_close_date(
    duration: &TradeDuration,
    trade_date: NaiveDate,
    resolved: &Instrument,
    signal: TriggerSignal,
    holidays: &BTreeSet<NaiveDate>,
) -> Option<NaiveDate> {
    match *duration {
        TradeDuration::HoldUntilExit => None,
        TradeDuration::Tenor(tenor) => Some(tenor.add_to_date(trade_date, holidays)),
        TradeDuration::FixedDate(d) => Some(d),
        TradeDuration::InstrumentExpiry => resolved.fixed_expiry_date(),
        TradeDuration::NextSchedule => match signal {
            TriggerSignal::Standard { next_schedule } => next_schedule,
            TriggerSignal::MeanReversionEntry { .. } | TriggerSignal::MeanReversionExit { .. } => {
                None
            }
        },
    }
}
