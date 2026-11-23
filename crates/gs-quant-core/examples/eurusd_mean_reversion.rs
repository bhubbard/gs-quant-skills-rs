//! EURUSD z-score mean-reversion backtest example inspired by Unbiased Trading's test of `gs_quant`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use gs_quant_core::{
    Action, BacktestConfig, DataSeries, GenericEngine, Instrument, LocalPricer, MarketHistory,
    MarketSnapshot, Strategy, TradeDuration, TransactionCostModel, Trigger, TriggerCondition,
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
    let csv_path = sample_csv_path();
    let series = DataSeries::load_csv(&csv_path, "date", "close")?;
    let dates = series.dates();
    let start = *dates.first().ok_or("empty sample CSV")?;
    let end = *dates.last().ok_or("empty sample CSV")?;

    let mut history = MarketHistory::new();
    for (date, spot) in series.iter() {
        history.insert_snapshot(MarketSnapshot::new(date).with_eq_spot("EURUSD", spot));
    }
    history.insert_series("EURUSD", series);

    // Goldman's default MeanReversionTrigger parameters:
    // 30-day rolling mean/std, +/- 2.0 z-score entry, close when price crosses rolling mean.
    // Demonstrates asymmetric entry ($25) vs exit ($15) fees.
    let trigger = Trigger::new(
        TriggerCondition::MeanReversion {
            series_name: "EURUSD".to_owned(),
            z_score_bound: 2.0,
            rolling_mean_window: 30,
            rolling_std_window: 30,
        },
        vec![Action::AddTrade {
            instrument: Instrument::spot("EURUSD_MR", "EURUSD", 100_000.0),
            duration: TradeDuration::HoldUntilExit,
            name: Some("EURUSD_MR".to_owned()),
            entry_cost: TransactionCostModel::constant(25.0),
            exit_cost: TransactionCostModel::constant(15.0),
            calendar: BTreeSet::new(),
        }],
    );

    let strategy = Strategy::new(Vec::new(), vec![trigger]);
    let mut cfg = BacktestConfig::daily(start, end);
    cfg.states = Some(dates);

    let result = GenericEngine.run(&strategy, &cfg, &history, &LocalPricer::new())?;
    let stats = result.summary_stats()?;

    println!("=== EURUSD Mean-Reversion Backtest (2024-2025 Sample) ===");
    println!("{stats}\n");
    println!("Trade Ledger ({} trades):", result.trade_ledger.len());
    for entry in &result.trade_ledger {
        println!(
            "  #{:<2} {} -> {:?}  size={:>10.0}  net_pnl={:?}",
            entry.trade_id,
            entry.open_date,
            entry.close_date,
            entry.signed_size,
            entry.realized_pnl.map(|p| format!("{p:+.2}"))
        );
    }

    Ok(())
}
