//! Pure Rust offline quantitative analytics, pricing, scenarios, volatility, and backtesting engine.
//!
//! Derived conceptually from the Goldman Sachs `gs_quant` AI skill specification, allowing any
//! user to price instruments, run scenarios, calculate realized volatility, and execute stateful
//! multi-asset backtests locally on user-supplied CSV or in-memory market data with zero Python
//! dependencies and zero Marquee API credentials.

pub mod backtest;
pub mod costs;
pub mod error;
pub mod instrument;
pub mod market;
pub mod pricing;
pub mod risk;
pub mod scenario;
pub mod stats;
pub mod tenor;
pub mod volatility;

pub use backtest::{
    eq_pnl_definition, fx_pnl_definition, Action, AggregateMode, AttributionOrder, BacktestConfig,
    BacktestResult, GenericEngine, PnlAttribute, PnlDefinition, PnlExplainRow, ResultSummaryRow,
    ScalingType, Strategy, TradeDuration, TradeLedgerEntry, Trigger, TriggerCondition,
    TriggerDirection,
};
pub use costs::{CostScaling, TransactionAggType, TransactionCostModel};
pub use error::QuantError;
pub use instrument::{ExpirySpec, Instrument, OptionType, PayReceive, StrikeSpec};
pub use market::{DataSeries, MarketDataKey, MarketHistory, MarketSnapshot, MissingDataStrategy};
pub use pricing::{
    norm_cdf, norm_pdf, HistoricalPricePoint, HistoricalPricer, LocalPricer, Portfolio, Pricer,
};
pub use risk::{AggregationLevel, Currency, RiskMeasure, RiskResult};
pub use scenario::{
    ForwardRealisation, MarketDataPattern, MarketDataShock, MarketDataShockBasedScenario, RollFwd,
    ScenarioResult,
};
pub use stats::SummaryStats;
pub use tenor::{
    business_day_schedule, is_business_day, next_business_day, roll_to_business_day, Tenor,
};
pub use volatility::{realized_volatility, ReturnsType, VolatilityMean};
