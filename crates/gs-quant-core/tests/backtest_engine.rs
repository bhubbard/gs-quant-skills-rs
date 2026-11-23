//! Integration tests for `GenericEngine`, triggers, position caps, hedging, and P&L Explain.

use std::collections::BTreeSet;
use std::path::PathBuf;

use chrono::{Duration, NaiveDate};
use gs_quant_core::{
    business_day_schedule, fx_pnl_definition, Action, AggregateMode, BacktestConfig, CostScaling,
    DataSeries, ExpirySpec, GenericEngine, HistoricalPricer, Instrument, LocalPricer,
    MarketHistory, MarketSnapshot, OptionType, Portfolio, RiskMeasure, ScalingType, Strategy,
    StrikeSpec, Tenor, TradeDuration, TransactionAggType, TransactionCostModel, Trigger,
    TriggerCondition, TriggerDirection,
};

fn sample_csv() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("data")
        .join("sample_eurusd.csv")
}

#[test]
fn periodic_next_schedule_and_historical_pricer() {
    let start = NaiveDate::from_ymd_opt(2025, 1, 2).unwrap();
    let end = NaiveDate::from_ymd_opt(2025, 3, 31).unwrap();
    let dates =
        business_day_schedule(start, end, Tenor::BusinessDays(1), &BTreeSet::new()).unwrap();

    let mut history = MarketHistory::new();
    for (idx, &d) in dates.iter().enumerate() {
        history.insert_snapshot(MarketSnapshot::new(d).with_eq_spot("SPY", 100.0 + idx as f64));
    }

    // 1. Static buy-and-hold via HistoricalPricer vs GenericEngine initial_portfolio
    let spy = Instrument::spot("SPY", "SPY", 10.0);
    let port = Portfolio::new("Static", vec![spy.clone()]);
    let direct = HistoricalPricer::run(
        &port,
        &dates,
        &history,
        &RiskMeasure::DollarPrice,
        &LocalPricer,
    )
    .unwrap();

    let cfg = BacktestConfig::daily(start, end);
    let engine_static = GenericEngine
        .run(
            &Strategy::new(vec![spy], Vec::new()),
            &cfg,
            &history,
            &LocalPricer,
        )
        .unwrap();

    // Direct PV rises by 10.0 per day; GenericEngine Total tracks cumulative P&L from 0.0
    let direct_gain = direct.last().unwrap().value - direct.first().unwrap().value;
    let engine_gain = engine_static.result_summary.last().unwrap().total;
    assert!((direct_gain - engine_gain).abs() < 1e-9);

    // 2. Periodic weekly trigger with TradeDuration::NextSchedule + OpenPositionCount cap
    let trig = Trigger::new(
        TriggerCondition::Periodic {
            start_date: start,
            end_date: end,
            frequency: Tenor::Weeks(2),
            calendar: BTreeSet::new(),
        },
        vec![Action::AddTrade {
            instrument: Instrument::spot("SPY_Roll", "SPY", 5.0),
            duration: TradeDuration::NextSchedule,
            name: Some("SPY_Roll".into()),
            entry_cost: TransactionCostModel::Constant(10.0),
            exit_cost: TransactionCostModel::Constant(5.0),
            calendar: BTreeSet::new(),
        }],
    );
    let res = GenericEngine
        .run(
            &Strategy::new(Vec::new(), vec![trig]),
            &cfg,
            &history,
            &LocalPricer,
        )
        .unwrap();
    assert!(res.trade_ledger.len() >= 5);
    assert!(res.trade_ledger[0].close_date.is_some());
    let stats = res.summary_stats().unwrap();
    assert!(stats.total_transaction_costs < 0.0);
}

#[test]
fn open_position_cap_blocks_excess_entries() {
    let start = NaiveDate::from_ymd_opt(2025, 1, 2).unwrap();
    let end = NaiveDate::from_ymd_opt(2025, 1, 15).unwrap();
    let dates =
        business_day_schedule(start, end, Tenor::BusinessDays(1), &BTreeSet::new()).unwrap();
    let mut history = MarketHistory::new();
    for &d in &dates {
        history.insert_snapshot(MarketSnapshot::new(d).with_eq_spot("SPY", 500.0));
    }

    // Periodic daily trigger gated by OpenPositionCount < 2
    let trig = Trigger::new(
        TriggerCondition::Aggregate {
            mode: AggregateMode::AllOf,
            conditions: vec![
                TriggerCondition::Periodic {
                    start_date: start,
                    end_date: end,
                    frequency: Tenor::BusinessDays(1),
                    calendar: BTreeSet::new(),
                },
                TriggerCondition::OpenPositionCount {
                    limit: 2,
                    direction: TriggerDirection::Below,
                },
            ],
        },
        vec![Action::AddTrade {
            instrument: Instrument::spot("CapTrade", "SPY", 1.0),
            duration: TradeDuration::HoldUntilExit,
            name: None,
            entry_cost: TransactionCostModel::Zero,
            exit_cost: TransactionCostModel::Zero,
            calendar: BTreeSet::new(),
        }],
    );

    let cfg = BacktestConfig::daily(start, end);
    let res = GenericEngine
        .run(
            &Strategy::new(Vec::new(), vec![trig]),
            &cfg,
            &history,
            &LocalPricer,
        )
        .unwrap();
    assert_eq!(
        res.trade_ledger.len(),
        2,
        "open position cap must block entries once 2 open"
    );
}

#[test]
fn risk_measure_scaling_and_max_transaction_cost_and_pnl_explain() {
    let start = NaiveDate::from_ymd_opt(2025, 1, 2).unwrap();
    let end = NaiveDate::from_ymd_opt(2025, 2, 14).unwrap();
    let dates =
        business_day_schedule(start, end, Tenor::BusinessDays(1), &BTreeSet::new()).unwrap();
    let mut history = MarketHistory::new();
    for (i, &d) in dates.iter().enumerate() {
        let spot = 1.08 + 0.0008 * (i as f64) + if i % 4 == 0 { 0.002 } else { -0.001 };
        let vol = 0.085 + 0.0005 * ((i % 7) as f64);
        history.insert_snapshot(
            MarketSnapshot::new(d)
                .with_fx_spot("EURUSD", spot)
                .with_fx_vol("EURUSD", "ATM", vol)
                .with_risk_free_rate("USD", 0.04)
                .with_risk_free_rate("EUR", 0.025),
        );
    }

    // Two options with different strikes scaled to the exact same 75,000 FxDelta target
    let opt_atm = Instrument::fx_option(
        "ATM_Call",
        "EURUSD",
        1.0,
        OptionType::Call,
        StrikeSpec::AtmSpot,
        ExpirySpec::Relative(Tenor::Months(3)),
    );
    let opt_otm = Instrument::fx_option(
        "OTM_Call",
        "EURUSD",
        1.0,
        OptionType::Call,
        StrikeSpec::PctOfSpot(1.03),
        ExpirySpec::Relative(Tenor::Months(3)),
    );

    let max_fee = TransactionCostModel::aggregate(
        vec![
            TransactionCostModel::Constant(150.0),
            TransactionCostModel::Scaled {
                scaling: CostScaling::Notional,
                rate: 0.0001,
            },
        ],
        TransactionAggType::Max,
    );

    let entry = Trigger::new(
        TriggerCondition::Dates(vec![dates[0]]),
        vec![
            Action::AddScaledTrade {
                instrument: opt_atm,
                duration: TradeDuration::HoldUntilExit,
                scaling_type: ScalingType::RiskMeasure(RiskMeasure::FxDelta),
                scaling_level: 75_000.0,
                name: Some("ATM_Call".into()),
                entry_cost: max_fee,
                exit_cost: TransactionCostModel::Constant(75.0),
                calendar: BTreeSet::new(),
            },
            Action::AddScaledTrade {
                instrument: opt_otm,
                duration: TradeDuration::HoldUntilExit,
                scaling_type: ScalingType::RiskMeasure(RiskMeasure::FxDelta),
                scaling_level: 75_000.0,
                name: Some("OTM_Call".into()),
                entry_cost: TransactionCostModel::Constant(100.0),
                exit_cost: TransactionCostModel::Constant(50.0),
                calendar: BTreeSet::new(),
            },
        ],
    );

    // +/- 50,000 FxDelta hedge trigger
    let hedge = Trigger::new(
        TriggerCondition::Aggregate {
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
        },
        vec![
            Action::ExitTrade {
                name: "hedge_fwd".into(),
            },
            Action::Hedge {
                risk: RiskMeasure::FxDelta,
                hedge_instrument: Instrument::fx_forward(
                    "hedge_fwd",
                    "EURUSD",
                    1.0,
                    ExpirySpec::Relative(Tenor::Months(1)),
                ),
                duration: TradeDuration::HoldUntilExit,
                name: Some("hedge_fwd".into()),
                entry_cost: TransactionCostModel::Constant(25.0),
                exit_cost: TransactionCostModel::Constant(10.0),
            },
        ],
    );

    let mut cfg = BacktestConfig::daily(start, end);
    cfg.risks = vec![RiskMeasure::FxDelta];
    cfg.pnl_explain = Some(fx_pnl_definition());

    let res = GenericEngine
        .run(
            &Strategy::new(Vec::new(), vec![entry, hedge]),
            &cfg,
            &history,
            &LocalPricer,
        )
        .unwrap();

    // Verify OTM call received larger notional than ATM call to hit same 75k FxDelta
    assert!(res.trade_ledger[1].signed_size > res.trade_ledger[0].signed_size);
    // Verify hedge brought portfolio FxDelta within +/- 1.0 on day 0
    let day0_delta = *res.result_summary[0].risks.get("FXDelta").unwrap();
    assert!(day0_delta.abs() < 1e-4, "day0 hedged delta = {day0_delta}");

    // Verify PnlExplain additive identity: sum(attrs) + unexplained == total_pnl_change
    let explain = res.pnl_explain.as_ref().unwrap();
    for row in explain {
        let sum_attrs: f64 = row.attributes.values().sum();
        assert!(
            (sum_attrs + row.unexplained - row.total_pnl_change).abs() < 1e-7,
            "explain identity mismatch on {}",
            row.date
        );
    }
    // Theta/cross-terms leave non-zero unexplained on later days
    assert!(explain.last().unwrap().unexplained.abs() > 1e-4);
}

#[test]
fn mean_reversion_on_sample_eurusd_csv() {
    let series = DataSeries::load_csv(sample_csv(), "date", "close").unwrap();
    let dates = series.dates();
    let start = *dates.first().unwrap();
    let end = *dates.last().unwrap();

    let mut history = MarketHistory::new();
    for (d, px) in series.iter() {
        history.insert_snapshot(MarketSnapshot::new(d).with_eq_spot("EURUSD", px));
    }
    history.insert_series("EURUSD", series);

    let trig = Trigger::new(
        TriggerCondition::MeanReversion {
            series_name: "EURUSD".into(),
            z_score_bound: 2.0,
            rolling_mean_window: 30,
            rolling_std_window: 30,
        },
        vec![Action::AddTrade {
            instrument: Instrument::spot("EURUSD_MR", "EURUSD", 100_000.0),
            duration: TradeDuration::HoldUntilExit,
            name: None,
            entry_cost: TransactionCostModel::Constant(25.0),
            exit_cost: TransactionCostModel::Constant(15.0),
            calendar: BTreeSet::new(),
        }],
    );

    let mut cfg = BacktestConfig::daily(start, end);
    cfg.states = Some(dates);
    let res = GenericEngine
        .run(
            &Strategy::new(Vec::new(), vec![trig]),
            &cfg,
            &history,
            &LocalPricer,
        )
        .unwrap();
    let stats = res.summary_stats().unwrap();

    assert!(
        stats.total_trades >= 5,
        "expected multiple MR trades, got {}",
        stats.total_trades
    );
    assert!(stats.closed_trades >= 4);
    assert!(stats.win_rate_pct.unwrap() > 0.0);
    // Verify entry (25) + exit (15) accounting matches total_transaction_costs
    let expected_costs: f64 = res
        .trade_ledger
        .iter()
        .map(|t| t.entry_cost + t.exit_cost.unwrap_or(0.0))
        .sum();
    assert!((stats.total_transaction_costs + expected_costs).abs() < 1e-9);

    let _ = Duration::days(1);
}
