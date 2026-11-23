//! FX option strategy with +/- 50k FXDelta hedge band, Max() transaction fees,
//! scenario shock testing, and P&L Explain attribution.

use std::collections::BTreeSet;
use std::path::PathBuf;

use gs_quant_core::{
    fx_pnl_definition, Action, AggregateMode, BacktestConfig, ExpirySpec, GenericEngine,
    Instrument, LocalPricer, MarketDataPattern, MarketDataShock, MarketDataShockBasedScenario,
    MarketHistory, MissingDataStrategy, OptionType, Pricer, RiskMeasure, ScalingType, Strategy,
    StrikeSpec, Tenor, TradeDuration, TransactionAggType, TransactionCostModel, Trigger,
    TriggerCondition, TriggerDirection,
};

fn sample_csv_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .join("..")
        .join("..")
        .join("data")
        .join("sample_eurusd.csv")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let history = MarketHistory::from_fx_csv(
        sample_csv_path(),
        "EURUSD",
        "date",
        "close",
        Some("implied_vol"),
    )?;
    let dates = history.dates();
    let start = *dates.first().ok_or("empty sample CSV")?;
    let end = *dates.last().ok_or("empty sample CSV")?;

    // 1. Periodic monthly entry: buy 3m ATMF EURUSD Call scaled to +100,000 USD of initial FxDelta.
    let call_template = Instrument::fx_option(
        "EURUSD_3m_Call",
        "EURUSD",
        1.0,
        OptionType::Call,
        StrikeSpec::AtmForward,
        ExpirySpec::Relative(Tenor::Months(3)),
    );

    let option_entry_fee = TransactionCostModel::aggregate(
        vec![
            TransactionCostModel::constant(100.0),
            TransactionCostModel::by_measure(RiskMeasure::DollarPrice, 0.01),
        ],
        TransactionAggType::Max,
    );

    let entry_trigger = Trigger::new(
        TriggerCondition::Periodic {
            start_date: start,
            end_date: end,
            frequency: Tenor::Months(1),
            calendar: BTreeSet::new(),
        },
        vec![Action::AddScaledTrade {
            instrument: call_template,
            duration: TradeDuration::Tenor(Tenor::Months(1)),
            scaling_type: ScalingType::RiskMeasure(RiskMeasure::FxDelta),
            scaling_level: 100_000.0,
            name: Some("EURUSD_3m_Call".to_owned()),
            entry_cost: option_entry_fee,
            exit_cost: TransactionCostModel::constant(50.0),
            calendar: BTreeSet::new(),
        }],
    );

    // 2. Dual-sided +/- 50,000 FXDelta band triggers ExitTrade("delta_hedge") then Hedge with FXForward.
    let risk_band = TriggerCondition::Aggregate {
        mode: AggregateMode::AnyOf,
        conditions: vec![
            TriggerCondition::StrategyRisk {
                measure: RiskMeasure::FxDelta,
                level: 50_000.0,
                direction: TriggerDirection::Above,
            },
            TriggerCondition::StrategyRisk {
                measure: RiskMeasure::FxDelta,
                level: -50_000.0,
                direction: TriggerDirection::Below,
            },
        ],
    };

    let hedge_trigger = Trigger::new(
        risk_band,
        vec![
            Action::ExitTrade {
                name: "delta_hedge".to_owned(),
            },
            Action::Hedge {
                risk: RiskMeasure::FxDelta,
                hedge_instrument: Instrument::fx_forward(
                    "delta_hedge",
                    "EURUSD",
                    1.0,
                    ExpirySpec::Relative(Tenor::Months(1)),
                ),
                duration: TradeDuration::Tenor(Tenor::Months(1)),
                name: Some("delta_hedge".to_owned()),
                entry_cost: TransactionCostModel::notional_bps(0.0001),
                exit_cost: TransactionCostModel::constant(20.0),
            },
        ],
    );

    let strategy = Strategy::new(Vec::new(), vec![entry_trigger, hedge_trigger]);
    let mut cfg = BacktestConfig::daily(start, end);
    cfg.states = Some(dates.clone());
    cfg.risks = vec![RiskMeasure::FxDelta, RiskMeasure::FxVega];
    cfg.pnl_explain = Some(fx_pnl_definition());

    let pricer = LocalPricer::new();
    let result = GenericEngine.run(&strategy, &cfg, &history, &pricer)?;
    let stats = result.summary_stats()?;

    println!("=== FX Option +/-50k Delta-Hedge Strategy ===");
    println!("{stats}\n");

    if let Some(last_explain) = result.pnl_explain.as_ref().and_then(|rows| rows.last()) {
        println!(
            "Cumulative P&L Explain on {}: total_change={:+.2}, attrs={:?}, unexplained={:+.2}",
            last_explain.date,
            last_explain.total_pnl_change,
            last_explain.attributes,
            last_explain.unexplained
        );
    }

    // 3. Demonstrate Scenario shock (+2% spot, +1 vol pt) on an unresolved vs resolved snapshot.
    let snap = history.snapshot(end, MissingDataStrategy::FillForward)?;
    let sample_opt = Instrument::fx_option(
        "Shock_Test_Call",
        "EURUSD",
        1_000_000.0,
        OptionType::Call,
        StrikeSpec::AtmForward,
        ExpirySpec::Relative(Tenor::Months(3)),
    )
    .resolve(end, &snap, &BTreeSet::new())?;

    let scenario = MarketDataShockBasedScenario::new()
        .with_shock(
            MarketDataPattern::any()
                .with_type("FX")
                .with_asset("EURUSD")
                .with_class("SPOT"),
            MarketDataShock::Proportional(0.02),
        )
        .with_shock(
            MarketDataPattern::any()
                .with_type("FX VOL")
                .with_asset("EURUSD"),
            MarketDataShock::Absolute(0.01),
        );

    let shocked = scenario.apply(&snap);
    let base_pv = pricer
        .calc(&sample_opt, &snap, &RiskMeasure::DollarPrice)?
        .value;
    let shocked_pv = pricer
        .calc(&sample_opt, &shocked.snapshot, &RiskMeasure::DollarPrice)?
        .value;
    println!(
        "\nScenario (+2% EURUSD, +1 vol pt): matched_points={}, PV {:.2} -> {:.2} ({:+.2})",
        shocked.matched_points,
        base_pv,
        shocked_pv,
        shocked_pv - base_pv
    );

    Ok(())
}
