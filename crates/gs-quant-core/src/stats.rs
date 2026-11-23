//! Summary statistics (all 19 `gs_quant` metrics + trade-level diagnostics).

use std::fmt;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::backtest::{ResultSummaryRow, TradeLedgerEntry};
use crate::error::QuantError;

/// Backtest performance summary containing Goldman's 3 date-range metadata fields,
/// all 19 performance statistics (`gs_quant.backtests.backtest_objects.summary_stats`),
/// and closed-trade diagnostics (win rate and average holding duration).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryStats {
    /// First valuation date in the backtest summary.
    pub start_date: NaiveDate,
    /// Final valuation date in the backtest summary.
    pub end_date: NaiveDate,
    /// Calendar days between `start_date` and `end_date`.
    pub duration_days: i64,

    // ── 19 Goldman Sachs summary_stats() metrics ──────────────────────
    /// 1. Final value of the `Total` series.
    pub total_pnl: f64,
    /// 2. Final cumulative transaction costs (negative or zero).
    pub total_transaction_costs: f64,
    /// 3. Total trades entered during the backtest.
    pub total_trades: usize,
    /// 4. Peak cumulative `Total` value reached.
    pub peak_pnl: f64,
    /// 5. Mean daily P&L change multiplied by `annualisation_factor`.
    pub annualised_return: f64,
    /// 6. Sample standard deviation of daily P&L changes multiplied by `sqrt(annualisation_factor)`.
    pub annualised_volatility: f64,
    /// 7. `annualised_return / annualised_volatility` (`None` if volatility is zero).
    pub sharpe_ratio: Option<f64>,
    /// 8. `annualised_return / annualised_downside_deviation` (`None` if downside deviation is zero).
    pub sortino_ratio: Option<f64>,
    /// 9. Largest peak-to-trough drop in `Total` (zero or negative).
    pub max_drawdown: f64,
    /// 10. Longest calendar-day streak spent below the running `Total` peak.
    pub max_drawdown_duration_days: i64,
    /// 11. `annualised_return / |max_drawdown|` (`None` if `max_drawdown == 0.0`).
    pub calmar_ratio: Option<f64>,
    /// 12. Drawdown (`Total - running_max`) at `end_date`.
    pub current_drawdown: f64,
    /// 13. Mean daily P&L change.
    pub average_daily_pnl: f64,
    /// 14. Sample standard deviation (`N - 1`) of daily P&L changes.
    pub daily_pnl_std_dev: f64,
    /// 15. Largest single-day positive P&L change.
    pub best_day: f64,
    /// 16. Worst single-day P&L change.
    pub worst_day: f64,
    /// 17. Percentage (`0..=100`) of daily periods with strictly positive P&L change.
    pub pct_positive_days: Option<f64>,
    /// 18. Adjusted Fisher-Pearson sample skewness of daily P&L changes (`None` if `< 3` diffs or zero std).
    pub skewness: Option<f64>,
    /// 19. Sample excess kurtosis of daily P&L changes (`None` if `< 4` diffs or zero std).
    pub kurtosis: Option<f64>,

    // ── Closed-trade diagnostics (from Unbiased Trading analysis) ──────
    /// Number of closed round-trip trades in the ledger.
    pub closed_trades: usize,
    /// Percentage (`0..=100`) of closed trades with positive net P&L.
    pub win_rate_pct: Option<f64>,
    /// Average holding duration (calendar days) across all closed trades.
    pub avg_holding_days: Option<f64>,
    /// Average holding duration (calendar days) for winning closed trades.
    pub avg_winning_hold_days: Option<f64>,
    /// Average holding duration (calendar days) for losing closed trades.
    pub avg_losing_hold_days: Option<f64>,
}

impl SummaryStats {
    /// Computes [`SummaryStats`] from the daily summary rows and trade ledger.
    ///
    /// # Errors
    ///
    /// Returns [`QuantError::EmptySeries`] if `summary` is empty, or [`QuantError::InvalidParameter`]
    /// if `annualisation_factor == 0`.
    pub fn compute(
        summary: &[ResultSummaryRow],
        ledger: &[TradeLedgerEntry],
        annualisation_factor: u32,
    ) -> Result<Self, QuantError> {
        if summary.is_empty() {
            return Err(QuantError::empty_series(
                "cannot compute summary statistics on an empty backtest",
            ));
        }
        if annualisation_factor == 0 {
            return Err(QuantError::invalid_parameter(
                "annualisation_factor must be positive",
            ));
        }

        let start_date = summary[0].date;
        let end_date = summary[summary.len() - 1].date;
        let duration_days = (end_date - start_date).num_days();

        let total_pnl = summary[summary.len() - 1].total;
        let total_transaction_costs = summary[summary.len() - 1].transaction_costs;
        let total_trades = ledger.len();

        let daily_pnl: Vec<f64> = summary
            .windows(2)
            .map(|w| w[1].total - w[0].total)
            .collect();
        let num_periods = daily_pnl.len();

        let avg_daily = if num_periods > 0 {
            daily_pnl.iter().sum::<f64>() / (num_periods as f64)
        } else {
            0.0
        };
        let std_daily = sample_std(&daily_pnl);

        let ann_f = f64::from(annualisation_factor);
        let annualised_return = avg_daily * ann_f;
        let annualised_volatility = std_daily * ann_f.sqrt();

        let sharpe_ratio =
            (annualised_volatility != 0.0).then_some(annualised_return / annualised_volatility);

        let downside: Vec<f64> = daily_pnl.iter().copied().filter(|&x| x < 0.0).collect();
        let downside_rms = if downside.is_empty() {
            0.0
        } else {
            (downside.iter().map(|x| x * x).sum::<f64>() / (downside.len() as f64)).sqrt()
        };
        let ann_downside = downside_rms * ann_f.sqrt();
        let sortino_ratio = (ann_downside != 0.0).then_some(annualised_return / ann_downside);

        // Drawdown series
        let mut running_max = f64::NEG_INFINITY;
        let mut max_drawdown = 0.0_f64;
        let mut current_drawdown = 0.0_f64;
        let mut max_dd_duration_days = 0_i64;
        let mut dd_streak_start: Option<NaiveDate> = None;

        for row in summary {
            if row.total > running_max {
                running_max = row.total;
            }
            let dd = row.total - running_max;
            if dd < max_drawdown {
                max_drawdown = dd;
            }
            current_drawdown = dd;

            if dd < 0.0 {
                let start_d = *dd_streak_start.get_or_insert(row.date);
                let span = (row.date - start_d).num_days();
                if span > max_dd_duration_days {
                    max_dd_duration_days = span;
                }
            } else {
                dd_streak_start = None;
            }
        }
        let peak_pnl = running_max;
        let calmar_ratio = (max_drawdown != 0.0).then_some(annualised_return / max_drawdown.abs());

        let best_day = daily_pnl.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let worst_day = daily_pnl.iter().copied().fold(f64::INFINITY, f64::min);
        let best_day = if num_periods == 0 { 0.0 } else { best_day };
        let worst_day = if num_periods == 0 { 0.0 } else { worst_day };

        let pct_positive_days = if num_periods > 0 {
            let pos = daily_pnl.iter().filter(|&&x| x > 0.0).count();
            Some((pos as f64) / (num_periods as f64) * 100.0)
        } else {
            None
        };

        let skewness = sample_skewness(&daily_pnl, avg_daily, std_daily);
        let kurtosis = sample_excess_kurtosis(&daily_pnl, avg_daily, std_daily);

        // Trade-level diagnostics
        let closed: Vec<&TradeLedgerEntry> =
            ledger.iter().filter(|e| e.close_date.is_some()).collect();
        let closed_trades = closed.len();
        let (win_rate_pct, avg_holding_days, avg_winning_hold_days, avg_losing_hold_days) =
            if closed.is_empty() {
                (None, None, None, None)
            } else {
                let mut wins = 0usize;
                let mut total_hold = 0_i64;
                let mut win_hold = 0_i64;
                let mut loss_hold = 0_i64;
                let mut loss_cnt = 0usize;
                for t in &closed {
                    let pnl = t.realized_pnl.unwrap_or(0.0);
                    let hold = t.close_date.map_or(0, |c| (c - t.open_date).num_days());
                    total_hold += hold;
                    if pnl > 0.0 {
                        wins += 1;
                        win_hold += hold;
                    } else if pnl < 0.0 {
                        loss_cnt += 1;
                        loss_hold += hold;
                    }
                }
                let win_rate = (wins as f64) / (closed_trades as f64) * 100.0;
                let avg_hold = (total_hold as f64) / (closed_trades as f64);
                let avg_win_hold = (wins > 0).then(|| (win_hold as f64) / (wins as f64));
                let avg_loss_hold = (loss_cnt > 0).then(|| (loss_hold as f64) / (loss_cnt as f64));
                (Some(win_rate), Some(avg_hold), avg_win_hold, avg_loss_hold)
            };

        Ok(Self {
            start_date,
            end_date,
            duration_days,
            total_pnl,
            total_transaction_costs,
            total_trades,
            peak_pnl,
            annualised_return,
            annualised_volatility,
            sharpe_ratio,
            sortino_ratio,
            max_drawdown,
            max_drawdown_duration_days: max_dd_duration_days,
            calmar_ratio,
            current_drawdown,
            average_daily_pnl: avg_daily,
            daily_pnl_std_dev: std_daily,
            best_day,
            worst_day,
            pct_positive_days,
            skewness,
            kurtosis,
            closed_trades,
            win_rate_pct,
            avg_holding_days,
            avg_winning_hold_days,
            avg_losing_hold_days,
        })
    }
}

impl fmt::Display for SummaryStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let opt_f = |v: Option<f64>| -> String {
            v.map_or_else(|| "N/A".to_owned(), |x| format!("{x:.4}"))
        };
        let opt_pct = |v: Option<f64>| -> String {
            v.map_or_else(|| "N/A".to_owned(), |x| format!("{x:.2}%"))
        };
        let opt_days = |v: Option<f64>| -> String {
            v.map_or_else(|| "N/A".to_owned(), |x| format!("{x:.1}d"))
        };

        writeln!(f, "{:<30} {:>16}", "Metric", "Value")?;
        writeln!(f, "{:-<48}", "")?;
        writeln!(f, "{:<30} {:>16}", "Start Date", self.start_date)?;
        writeln!(f, "{:<30} {:>16}", "End Date", self.end_date)?;
        writeln!(f, "{:<30} {:>16}", "Duration (days)", self.duration_days)?;
        writeln!(f, "{:<30} {:>16.2}", "Total PnL", self.total_pnl)?;
        writeln!(
            f,
            "{:<30} {:>16.2}",
            "Total Transaction Costs", self.total_transaction_costs
        )?;
        writeln!(f, "{:<30} {:>16}", "Total Trades", self.total_trades)?;
        writeln!(f, "{:<30} {:>16.2}", "Peak PnL", self.peak_pnl)?;
        writeln!(
            f,
            "{:<30} {:>16.2}",
            "Annualised Return", self.annualised_return
        )?;
        writeln!(
            f,
            "{:<30} {:>16.2}",
            "Annualised Volatility", self.annualised_volatility
        )?;
        writeln!(f, "{:<30} {:>16}", "Sharpe Ratio", opt_f(self.sharpe_ratio))?;
        writeln!(
            f,
            "{:<30} {:>16}",
            "Sortino Ratio",
            opt_f(self.sortino_ratio)
        )?;
        writeln!(f, "{:<30} {:>16.2}", "Max Drawdown", self.max_drawdown)?;
        writeln!(
            f,
            "{:<30} {:>16}",
            "Max Drawdown Duration (days)", self.max_drawdown_duration_days
        )?;
        writeln!(f, "{:<30} {:>16}", "Calmar Ratio", opt_f(self.calmar_ratio))?;
        writeln!(
            f,
            "{:<30} {:>16.2}",
            "Current Drawdown", self.current_drawdown
        )?;
        writeln!(
            f,
            "{:<30} {:>16.4}",
            "Average Daily PnL", self.average_daily_pnl
        )?;
        writeln!(
            f,
            "{:<30} {:>16.4}",
            "Daily PnL Std Dev", self.daily_pnl_std_dev
        )?;
        writeln!(f, "{:<30} {:>16.2}", "Best Day", self.best_day)?;
        writeln!(f, "{:<30} {:>16.2}", "Worst Day", self.worst_day)?;
        writeln!(
            f,
            "{:<30} {:>16}",
            "% Positive Days",
            opt_pct(self.pct_positive_days)
        )?;
        writeln!(f, "{:<30} {:>16}", "Skewness", opt_f(self.skewness))?;
        writeln!(f, "{:<30} {:>16}", "Kurtosis", opt_f(self.kurtosis))?;
        writeln!(f, "{:-<48}", "")?;
        writeln!(f, "{:<30} {:>16}", "Closed Trades", self.closed_trades)?;
        writeln!(
            f,
            "{:<30} {:>16}",
            "Closed Trade Win Rate",
            opt_pct(self.win_rate_pct)
        )?;
        writeln!(
            f,
            "{:<30} {:>16}",
            "Avg Holding Duration",
            opt_days(self.avg_holding_days)
        )?;
        writeln!(
            f,
            "{:<30} {:>16}",
            "Avg Winning Hold Duration",
            opt_days(self.avg_winning_hold_days)
        )?;
        write!(
            f,
            "{:<30} {:>16}",
            "Avg Losing Hold Duration",
            opt_days(self.avg_losing_hold_days)
        )
    }
}

fn sample_std(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let sum_sq = values.iter().map(|x| (x - mean).powi(2)).sum::<f64>();
    (sum_sq / (n - 1.0)).sqrt()
}

fn sample_skewness(values: &[f64], mean: f64, std: f64) -> Option<f64> {
    if values.len() < 3 || std == 0.0 {
        return None;
    }
    let n = values.len() as f64;
    let m3 = values
        .iter()
        .map(|x| ((x - mean) / std).powi(3))
        .sum::<f64>();
    Some((n / ((n - 1.0) * (n - 2.0))) * m3)
}

fn sample_excess_kurtosis(values: &[f64], mean: f64, std: f64) -> Option<f64> {
    if values.len() < 4 || std == 0.0 {
        return None;
    }
    let n = values.len() as f64;
    let m4 = values
        .iter()
        .map(|x| ((x - mean) / std).powi(4))
        .sum::<f64>();
    let term1 = (n * (n + 1.0)) / ((n - 1.0) * (n - 2.0) * (n - 3.0)) * m4;
    let term2 = 3.0 * (n - 1.0).powi(2) / ((n - 2.0) * (n - 3.0));
    Some(term1 - term2)
}
