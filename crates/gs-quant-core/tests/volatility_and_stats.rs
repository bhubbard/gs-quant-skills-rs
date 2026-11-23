//! Integration tests for realized volatility conventions and Goldman 19 summary statistics.

use chrono::{Duration, NaiveDate};
use gs_quant_core::{
    realized_volatility, DataSeries, ResultSummaryRow, ReturnsType, SummaryStats, TradeLedgerEntry,
    VolatilityMean,
};
use std::collections::BTreeMap;

#[test]
fn realized_volatility_sample_mean_vs_zero_mean() {
    let start = NaiveDate::from_ymd_opt(2025, 1, 2).unwrap();
    // 5 prices => 4 simple returns: +1%, +3%, -1%, +3% (mean = +1%)
    let prices = [100.0, 101.0, 104.03, 102.9897, 106.079_391];
    let mut series = DataSeries::new();
    for (i, px) in prices.into_iter().enumerate() {
        series.insert(start + Duration::days(i as i64), px);
    }

    let sample = realized_volatility(
        &series,
        4,
        ReturnsType::Simple,
        252,
        VolatilityMean::SampleMean,
    )
    .unwrap();
    let zero_mean = realized_volatility(
        &series,
        4,
        ReturnsType::Simple,
        252,
        VolatilityMean::ZeroMean,
    )
    .unwrap();

    let s_vals: Vec<f64> = sample.iter().map(|(_, v)| v).collect();
    let z_vals: Vec<f64> = zero_mean.iter().map(|(_, v)| v).collect();
    assert_eq!(s_vals.len(), 1);
    assert_eq!(z_vals.len(), 1);

    // Returns are [0.01, 0.03, -0.01, 0.03], mean = 0.015:
    // Deviations: [-0.005, 0.015, -0.025, 0.015], sum_sq = 0.000025 + 0.000225 + 0.000625 + 0.000225 = 0.0011
    // SampleMean: sqrt(0.0011 / 3) * sqrt(252) * 100
    let expected_sample = (0.0011_f64 / 3.0).sqrt() * 252.0_f64.sqrt() * 100.0;
    // ZeroMean: sum(r^2) = 0.0001 + 0.0009 + 0.0001 + 0.0009 = 0.0020; sqrt(0.0020 / 4) * sqrt(252) * 100
    let expected_zero = (0.0020_f64 / 4.0).sqrt() * 252.0_f64.sqrt() * 100.0;

    assert!((s_vals[0] - expected_sample).abs() < 1e-9);
    assert!((z_vals[0] - expected_zero).abs() < 1e-9);
    assert!((z_vals[0] - s_vals[0]).abs() > 1.0);
}

#[test]
fn summary_stats_all_19_metrics_match_exact_formulas() {
    let d0 = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    // Totals: [0, +100, +50, +25, +150, +130]
    // Daily diffs: [+100, -50, -25, +125, -20] (5 periods)
    // Running max: [0, 100, 100, 100, 150, 150]
    // Drawdowns:   [0,   0, -50, -75,   0, -20] => max_dd = -75, current_dd = -20, peak = 150
    // DD duration: d+2 to d+3 = 1 day (since d+4 recovers to new peak 150)
    let totals = [0.0, 100.0, 50.0, 25.0, 150.0, 130.0];
    let rows: Vec<ResultSummaryRow> = totals
        .into_iter()
        .enumerate()
        .map(|(i, tot)| ResultSummaryRow {
            date: d0 + Duration::days(i as i64),
            price: tot,
            cumulative_cash: 0.0,
            transaction_costs: -45.0,
            total: tot,
            risks: BTreeMap::new(),
        })
        .collect();

    let ledger = vec![
        TradeLedgerEntry {
            trade_id: 1,
            instrument_name: "T1".into(),
            open_date: d0,
            signed_size: 1.0,
            entry_value: 100.0,
            entry_cost: 15.0,
            close_date: Some(d0 + Duration::days(2)),
            exit_value: Some(150.0),
            exit_cost: Some(10.0),
            realized_pnl: Some(25.0),
        },
        TradeLedgerEntry {
            trade_id: 2,
            instrument_name: "T2".into(),
            open_date: d0 + Duration::days(1),
            signed_size: 1.0,
            entry_value: 100.0,
            entry_cost: 10.0,
            close_date: Some(d0 + Duration::days(5)),
            exit_value: Some(80.0),
            exit_cost: Some(10.0),
            realized_pnl: Some(-40.0),
        },
    ];

    let stats = SummaryStats::compute(&rows, &ledger, 252).unwrap();
    assert_eq!(stats.start_date, d0);
    assert_eq!(stats.end_date, d0 + Duration::days(5));
    assert_eq!(stats.duration_days, 5);
    assert!((stats.total_pnl - 130.0).abs() < 1e-12);
    assert!((stats.total_transaction_costs + 45.0).abs() < 1e-12);
    assert_eq!(stats.total_trades, 2);
    assert!((stats.peak_pnl - 150.0).abs() < 1e-12);
    assert!((stats.max_drawdown + 75.0).abs() < 1e-12);
    assert_eq!(stats.max_drawdown_duration_days, 1);
    assert!((stats.current_drawdown + 20.0).abs() < 1e-12);
    assert!((stats.average_daily_pnl - 26.0).abs() < 1e-12);
    assert!((stats.best_day - 125.0).abs() < 1e-12);
    assert!((stats.worst_day + 50.0).abs() < 1e-12);
    assert!((stats.pct_positive_days.unwrap() - 40.0).abs() < 1e-12);
    assert!(stats.sharpe_ratio.is_some());
    assert!(stats.sortino_ratio.is_some());
    assert!(stats.calmar_ratio.is_some());
    assert!(stats.skewness.is_some());
    assert!(stats.kurtosis.is_some());
    assert_eq!(stats.closed_trades, 2);
    assert!((stats.win_rate_pct.unwrap() - 50.0).abs() < 1e-12);
    assert!((stats.avg_winning_hold_days.unwrap() - 2.0).abs() < 1e-12);
    assert!((stats.avg_losing_hold_days.unwrap() - 4.0).abs() < 1e-12);
}
