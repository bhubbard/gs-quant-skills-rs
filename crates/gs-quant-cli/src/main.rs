//! Pure Rust CLI (`gs-quant-skills`) for AI skill installation and offline quant workflows.

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use chrono::{Duration, NaiveDate};
use clap::{Args, Parser, Subcommand, ValueEnum};
use gs_quant_core::{
    realized_volatility, Action, AggregateMode, BacktestConfig, CostScaling, DataSeries,
    ExpirySpec, GenericEngine, Instrument, LocalPricer, MarketDataPattern, MarketDataShock,
    MarketDataShockBasedScenario, MarketHistory, MarketSnapshot, OptionType, Pricer, ReturnsType,
    RiskMeasure, Strategy, StrikeSpec, TradeDuration, TransactionAggType, TransactionCostModel,
    Trigger, TriggerCondition, TriggerDirection, VolatilityMean,
};
use gs_quant_skills::{
    install_skills, list_skills, uninstall_skills, InstallMode, InstallScope, SkillError,
    SkillStatus, UninstallOutcome,
};
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(
    name = "gs-quant-skills",
    version,
    about = "Install gs-quant-rs agent skills and run offline Pure Rust backtests, scenarios, and volatility."
)]
struct Cli {
    #[command(subcommand)]
    command: TopCommand,
}

#[derive(Subcommand, Debug)]
enum TopCommand {
    /// List bundled skills and show whether each is installed globally, locally, or in Antigravity.
    List(ListArgs),
    /// Install one or more skills (defaults to all).
    Install(InstallArgs),
    /// Uninstall one or more skills (defaults to all).
    Uninstall(UninstallArgs),
    /// Run offline backtests against local CSV market data.
    Backtest {
        #[command(subcommand)]
        command: BacktestCommand,
    },
    /// Price an FX or Equity option base vs shocked market data scenario.
    Scenario(ScenarioArgs),
    /// Calculate rolling annualized realized volatility on a CSV series.
    Volatility(VolatilityArgs),
}

#[derive(Args, Debug)]
struct ScopeFlags {
    /// Target ~/.claude/skills (available in any project).
    #[arg(long = "global", group = "scope")]
    global_scope: bool,
    /// Target ./.claude/skills (current working directory).
    #[arg(long = "project", group = "scope")]
    project_scope: bool,
    /// Target ~/.gemini/config/skills (Antigravity global skills).
    #[arg(long = "antigravity", group = "scope")]
    antigravity_scope: bool,
    /// Target an explicit custom skills directory path.
    #[arg(long = "target-dir", value_name = "PATH", group = "scope")]
    target_dir: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct ListArgs {
    /// Optional path to an on-disk skills directory instead of local/embedded defaults.
    #[arg(long, value_name = "PATH")]
    skills_dir: Option<PathBuf>,
    /// Output JSON instead of aligned text.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct InstallArgs {
    /// Skill name(s) to install. Defaults to all bundled skills.
    skills: Vec<String>,
    #[command(flatten)]
    scope: ScopeFlags,
    /// Copy skill files even on Unix.
    #[arg(long, group = "mode")]
    copy: bool,
    /// Create a directory symlink (requires an on-disk skills directory on Unix).
    #[arg(long, group = "mode")]
    symlink: bool,
    /// Optional path to an on-disk skills directory.
    #[arg(long, value_name = "PATH")]
    skills_dir: Option<PathBuf>,
    /// Output JSON instead of text lines.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct UninstallArgs {
    /// Skill name(s) to uninstall. Defaults to all bundled skills.
    skills: Vec<String>,
    #[command(flatten)]
    scope: ScopeFlags,
    /// Optional path to an on-disk skills directory.
    #[arg(long, value_name = "PATH")]
    skills_dir: Option<PathBuf>,
    /// Output JSON instead of text lines.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum BacktestCommand {
    /// Run Goldman's z-score mean-reversion rule on a local CSV price feed.
    MeanReversion(MeanReversionArgs),
}

#[derive(Args, Debug)]
struct MeanReversionArgs {
    /// Path to CSV file containing date and price columns.
    #[arg(long, value_name = "PATH")]
    csv: PathBuf,
    /// Date column header (`YYYY-MM-DD`).
    #[arg(long, default_value = "date")]
    date_col: String,
    /// Price/close column header.
    #[arg(long, default_value = "close")]
    price_col: String,
    /// Entry z-score bound (triggers when |(price - mean) / std| > z_score).
    #[arg(long, default_value_t = 2.0)]
    z_score: f64,
    /// Trailing observation count for rolling mean.
    #[arg(long, default_value_t = 30)]
    mean_window: usize,
    /// Trailing observation count for rolling sample std dev.
    #[arg(long, default_value_t = 30)]
    std_window: usize,
    /// Base position notional/quantity size per trade.
    #[arg(long, default_value_t = 100_000.0)]
    notional: f64,
    /// Maximum open positions allowed (`0` blocks all entries).
    #[arg(long, default_value_t = 1)]
    max_open: usize,
    /// Fixed entry fee per trade.
    #[arg(long, default_value_t = 0.0)]
    entry_fee: f64,
    /// Fixed exit fee per trade.
    #[arg(long, default_value_t = 0.0)]
    exit_fee: f64,
    /// Additional proportional transaction fee in basis points of notional (`1.0` = 1bp = 0.0001).
    #[arg(long, default_value_t = 0.0)]
    bps: f64,
    /// Initial cash balance.
    #[arg(long, default_value_t = 0.0)]
    initial_cash: f64,
    /// Emit structured JSON summary and trade ledger.
    #[arg(long)]
    json: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum InstrumentKindArg {
    Fx,
    Eq,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum OptionTypeArg {
    Call,
    Put,
}

#[derive(Args, Debug)]
struct ScenarioArgs {
    /// Option asset class (`fx` or `eq`).
    #[arg(long, value_enum, default_value_t = InstrumentKindArg::Fx)]
    kind: InstrumentKindArg,
    /// Current underlying spot rate/price.
    #[arg(long)]
    spot: f64,
    /// Option strike level.
    #[arg(long)]
    strike: f64,
    /// Annualized implied volatility as decimal (`0.08` = 8%).
    #[arg(long)]
    vol: f64,
    /// Time to expiration in years (e.g. `0.25` = 3 months).
    #[arg(long)]
    expiry_years: f64,
    /// Domestic/USD continuous risk-free rate as decimal.
    #[arg(long, default_value_t = 0.0)]
    rate: f64,
    /// Foreign currency rate (FX) or dividend yield (EQ) as decimal.
    #[arg(long, default_value_t = 0.0)]
    foreign_rate: f64,
    /// Option notional (FX) or share quantity (EQ).
    #[arg(long, default_value_t = 100_000.0)]
    notional: f64,
    /// Option payoff direction (`call` or `put`).
    #[arg(long, value_enum, default_value_t = OptionTypeArg::Call)]
    option_type: OptionTypeArg,
    /// Proportional spot shock as decimal (`0.02` = +2%).
    #[arg(long, default_value_t = 0.0)]
    spot_shock_pct: f64,
    /// Absolute implied volatility shock as decimal (`0.01` = +1 vol point).
    #[arg(long, default_value_t = 0.0)]
    vol_shock_abs: f64,
    /// Emit structured JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum ReturnsArg {
    Simple,
    Log,
    Abs,
}

#[derive(Args, Debug)]
struct VolatilityArgs {
    /// Path to CSV file containing date and close/price columns.
    #[arg(long, value_name = "PATH")]
    csv: PathBuf,
    /// Date column header.
    #[arg(long, default_value = "date")]
    date_col: String,
    /// Price column header.
    #[arg(long, default_value = "close")]
    price_col: String,
    /// Rolling return count window.
    #[arg(long, default_value_t = 22)]
    window: usize,
    /// Return convention (`simple`, `log`, `abs`).
    #[arg(long, value_enum, default_value_t = ReturnsArg::Simple)]
    returns: ReturnsArg,
    /// Use zero-mean RMS (`N` denominator) instead of sample mean (`N - 1`).
    #[arg(long)]
    zero_mean: bool,
    /// Annualisation factor (`252` for daily observations).
    #[arg(long, default_value_t = 252)]
    ann_factor: u32,
    /// Number of trailing observations to display.
    #[arg(long, default_value_t = 10)]
    tail: usize,
    /// Emit structured JSON.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            if let Some(skill_err) = err.downcast_ref::<SkillError>() {
                match skill_err {
                    SkillError::UnknownSkills { .. } | SkillError::MissingScope => {
                        eprintln!("{skill_err}");
                        return ExitCode::from(2);
                    }
                    _ => {}
                }
            }
            eprintln!("error: {err:#}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        TopCommand::List(args) => cmd_list(args)?,
        TopCommand::Install(args) => cmd_install(args)?,
        TopCommand::Uninstall(args) => cmd_uninstall(args)?,
        TopCommand::Backtest { command } => match command {
            BacktestCommand::MeanReversion(args) => cmd_backtest_mean_reversion(args)?,
        },
        TopCommand::Scenario(args) => cmd_scenario(args)?,
        TopCommand::Volatility(args) => cmd_volatility(args)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_list(args: ListArgs) -> Result<()> {
    let (source, rows) = list_skills(args.skills_dir.as_deref())?;
    if args.json {
        #[derive(Serialize)]
        struct ListJson<'a> {
            source: &'a str,
            skills: &'a [gs_quant_skills::ListedSkill],
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&ListJson {
                source: &source,
                skills: &rows
            })?
        );
        return Ok(());
    }

    if rows.is_empty() {
        println!("No skills bundled with gs-quant-skills.");
        return Ok(());
    }
    println!("Bundled skills (source: {source}):");
    for row in rows {
        let mut active = Vec::new();
        if row.global != SkillStatus::NotInstalled {
            active.push(format!("global={}", row.global));
        }
        if row.project != SkillStatus::NotInstalled {
            active.push(format!("project={}", row.project));
        }
        if row.antigravity != SkillStatus::NotInstalled {
            active.push(format!("antigravity={}", row.antigravity));
        }
        let suffix = if active.is_empty() {
            "not installed".to_owned()
        } else {
            active.join(", ")
        };
        println!("  {}  [{suffix}]", row.name);
    }
    Ok(())
}

fn resolve_scope_or_prompt(flags: &ScopeFlags, action: &str) -> Result<InstallScope> {
    if flags.global_scope {
        return Ok(InstallScope::Global);
    }
    if flags.project_scope {
        return Ok(InstallScope::Project);
    }
    if flags.antigravity_scope {
        return Ok(InstallScope::Antigravity);
    }
    if let Some(ref custom) = flags.target_dir {
        return Ok(InstallScope::Custom(custom.clone()));
    }

    if !io::stdin().is_terminal() {
        return Err(SkillError::MissingScope.into());
    }

    loop {
        print!(
            "{action} (g)lobal (~/.claude/skills), (p)roject (./.claude/skills), or (a)ntigravity (~/.gemini/config/skills)? [g/p/a] "
        );
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        match line.trim().to_ascii_lowercase().as_str() {
            "g" | "global" => return Ok(InstallScope::Global),
            "p" | "project" => return Ok(InstallScope::Project),
            "a" | "antigravity" | "agy" => return Ok(InstallScope::Antigravity),
            _ => {}
        }
    }
}

fn cmd_install(args: InstallArgs) -> Result<()> {
    let scope = resolve_scope_or_prompt(&args.scope, "Install to")?;
    let mode = if args.copy {
        InstallMode::Copy
    } else if args.symlink {
        InstallMode::Symlink
    } else {
        InstallMode::Auto
    };

    let records = install_skills(&args.skills, &scope, mode, args.skills_dir.as_deref())?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&records)?);
        return Ok(());
    }
    for rec in records {
        println!(
            "{} {} -> {}",
            rec.method.verb(),
            rec.name,
            rec.target.display()
        );
    }
    Ok(())
}

fn cmd_uninstall(args: UninstallArgs) -> Result<()> {
    let scope = resolve_scope_or_prompt(&args.scope, "Uninstall from")?;
    let records = uninstall_skills(&args.skills, &scope, args.skills_dir.as_deref())?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&records)?);
        return Ok(());
    }
    for rec in records {
        match rec.outcome {
            UninstallOutcome::Removed => {
                println!("removed {} from {}", rec.name, rec.target.display());
            }
            UninstallOutcome::Skipped | _ => {
                println!(
                    "skip    {} (not installed at {})",
                    rec.name,
                    rec.target.display()
                );
            }
        }
    }
    Ok(())
}

fn build_cost_model(flat: f64, bps: f64) -> TransactionCostModel {
    let mut parts = Vec::new();
    if flat > 0.0 {
        parts.push(TransactionCostModel::Constant(flat));
    }
    if bps > 0.0 {
        parts.push(TransactionCostModel::Scaled {
            scaling: CostScaling::Notional,
            rate: bps * 0.0001,
        });
    }
    match parts.len() {
        0 => TransactionCostModel::Zero,
        1 => parts.remove(0),
        _ => TransactionCostModel::aggregate(parts, TransactionAggType::Sum),
    }
}

fn cmd_backtest_mean_reversion(args: MeanReversionArgs) -> Result<()> {
    let series = DataSeries::load_csv(&args.csv, &args.date_col, &args.price_col)
        .with_context(|| format!("failed to load `{}`", args.csv.display()))?;
    let dates = series.dates();
    let start = *dates.first().context("CSV series has no dates")?;
    let end = *dates.last().context("CSV series has no dates")?;

    let mut history = MarketHistory::new();
    for (date, px) in series.iter() {
        history.insert_snapshot(MarketSnapshot::new(date).with_eq_spot("ASSET", px));
    }
    history.insert_series("ASSET", series);

    let entry_cost = build_cost_model(args.entry_fee, args.bps);
    let exit_cost = build_cost_model(args.exit_fee, args.bps);

    let mr_cond = TriggerCondition::MeanReversion {
        series_name: "ASSET".to_owned(),
        z_score_bound: args.z_score,
        rolling_mean_window: args.mean_window,
        rolling_std_window: args.std_window,
    };
    let condition = if args.max_open == 0 {
        TriggerCondition::Aggregate {
            conditions: vec![
                TriggerCondition::OpenPositionCount {
                    limit: 0,
                    direction: TriggerDirection::Below,
                },
                mr_cond,
            ],
            mode: AggregateMode::AllOf,
        }
    } else {
        mr_cond
    };

    let trigger = Trigger::new(
        condition,
        vec![Action::AddTrade {
            instrument: Instrument::spot("MR_Asset", "ASSET", args.notional),
            duration: TradeDuration::HoldUntilExit,
            name: Some("MR_Asset".to_owned()),
            entry_cost,
            exit_cost,
            calendar: Default::default(),
        }],
    );

    let strategy = Strategy::new(Vec::new(), vec![trigger]);
    let mut cfg = BacktestConfig::daily(start, end);
    cfg.initial_value = args.initial_cash;
    cfg.states = Some(dates);

    let result = GenericEngine.run(&strategy, &cfg, &history, &LocalPricer::new())?;
    let stats = result.summary_stats()?;

    if args.json {
        #[derive(Serialize)]
        struct Output<'a> {
            stats: &'a gs_quant_core::SummaryStats,
            trade_ledger: &'a [gs_quant_core::TradeLedgerEntry],
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Output {
                stats: &stats,
                trade_ledger: &result.trade_ledger,
            })?
        );
        return Ok(());
    }

    println!("{stats}");
    Ok(())
}

fn cmd_scenario(args: ScenarioArgs) -> Result<()> {
    let base_date = NaiveDate::from_ymd_opt(2025, 1, 2).context("valid anchor date")?;
    let days = (args.expiry_years * 365.0).round().max(1.0) as i64;
    let expiry_date = base_date + Duration::days(days);
    let opt_type = match args.option_type {
        OptionTypeArg::Call => OptionType::Call,
        OptionTypeArg::Put => OptionType::Put,
    };

    let (
        snap,
        inst,
        scenario,
        spot_measure,
        delta_measure,
        gamma_measure,
        vega_measure,
        theta_measure,
    ) = match args.kind {
        InstrumentKindArg::Fx => {
            let s = MarketSnapshot::new(base_date)
                .with_fx_spot("EURUSD", args.spot)
                .with_fx_vol("EURUSD", "ATM", args.vol)
                .with_risk_free_rate("USD", args.rate)
                .with_risk_free_rate("EUR", args.foreign_rate);
            let i = Instrument::fx_option(
                "FX_Option",
                "EURUSD",
                args.notional,
                opt_type,
                StrikeSpec::Absolute(args.strike),
                ExpirySpec::FixedDate(expiry_date),
            );
            let sc = MarketDataShockBasedScenario::new()
                .with_shock(
                    MarketDataPattern::any()
                        .with_type("FX")
                        .with_asset("EURUSD")
                        .with_class("SPOT"),
                    MarketDataShock::Proportional(args.spot_shock_pct),
                )
                .with_shock(
                    MarketDataPattern::any()
                        .with_type("FX VOL")
                        .with_asset("EURUSD"),
                    MarketDataShock::Absolute(args.vol_shock_abs),
                );
            (
                s,
                i,
                sc,
                RiskMeasure::FxSpot,
                RiskMeasure::FxDelta,
                RiskMeasure::FxGamma,
                RiskMeasure::FxVega,
                RiskMeasure::FxTheta,
            )
        }
        InstrumentKindArg::Eq => {
            let mut s = MarketSnapshot::new(base_date)
                .with_eq_spot("ASSET", args.spot)
                .with_eq_vol("ASSET", "ATM", args.vol)
                .with_risk_free_rate("USD", args.rate);
            if args.foreign_rate != 0.0 {
                s.insert(
                    gs_quant_core::MarketDataKey::eq_dividend_yield("ASSET"),
                    args.foreign_rate,
                );
            }
            let i = Instrument::eq_option(
                "EQ_Option",
                "ASSET",
                args.notional,
                1.0,
                opt_type,
                StrikeSpec::Absolute(args.strike),
                ExpirySpec::FixedDate(expiry_date),
            );
            let sc = MarketDataShockBasedScenario::new()
                .with_shock(
                    MarketDataPattern::any()
                        .with_type("EQ")
                        .with_asset("ASSET")
                        .with_class("SPOT"),
                    MarketDataShock::Proportional(args.spot_shock_pct),
                )
                .with_shock(
                    MarketDataPattern::any()
                        .with_type("EQ VOL")
                        .with_asset("ASSET")
                        .with_class("ATM VOL"),
                    MarketDataShock::Absolute(args.vol_shock_abs),
                );
            (
                s,
                i,
                sc,
                RiskMeasure::EqSpot,
                RiskMeasure::EqDelta,
                RiskMeasure::EqGamma,
                RiskMeasure::EqVega,
                RiskMeasure::EqTheta,
            )
        }
    };

    let pricer = LocalPricer::new();
    let shocked_res = scenario.apply(&snap);

    let base_pv = pricer.calc(&inst, &snap, &RiskMeasure::DollarPrice)?.value;
    let base_delta = pricer.calc(&inst, &snap, &delta_measure)?.value;
    let base_gamma = pricer.calc(&inst, &snap, &gamma_measure)?.value;
    let base_vega = pricer.calc(&inst, &snap, &vega_measure)?.value;
    let base_theta = pricer.calc(&inst, &snap, &theta_measure)?.value;

    let shocked_pv = pricer
        .calc(&inst, &shocked_res.snapshot, &RiskMeasure::DollarPrice)?
        .value;
    let shocked_delta = pricer
        .calc(&inst, &shocked_res.snapshot, &delta_measure)?
        .value;
    let shocked_spot = pricer
        .calc(&inst, &shocked_res.snapshot, &spot_measure)?
        .value;

    #[derive(Serialize)]
    struct ScenarioOutput {
        matched_points: usize,
        base_spot: f64,
        shocked_spot: f64,
        base_pv: f64,
        shocked_pv: f64,
        pnl_diff: f64,
        base_delta: f64,
        shocked_delta: f64,
        base_gamma: f64,
        base_vega: f64,
        base_theta: f64,
    }

    let out = ScenarioOutput {
        matched_points: shocked_res.matched_points,
        base_spot: args.spot,
        shocked_spot,
        base_pv,
        shocked_pv,
        pnl_diff: shocked_pv - base_pv,
        base_delta,
        shocked_delta,
        base_gamma,
        base_vega,
        base_theta,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("Scenario Shock Comparison ({:?})", args.kind);
    println!("  Matched market points : {}", out.matched_points);
    println!(
        "  Spot                  : {:.5} -> {:.5}",
        out.base_spot, out.shocked_spot
    );
    println!(
        "  DollarPrice           : {:.2} -> {:.2}  (PnL diff: {:+.2})",
        out.base_pv, out.shocked_pv, out.pnl_diff
    );
    println!(
        "  Delta                 : {:.2} -> {:.2}",
        out.base_delta, out.shocked_delta
    );
    println!("  Gamma                 : {:.2}", out.base_gamma);
    println!("  Vega (+1 vol pt)      : {:.2}", out.base_vega);
    println!("  Theta (1 day)         : {:.2}", out.base_theta);
    Ok(())
}

fn cmd_volatility(args: VolatilityArgs) -> Result<()> {
    let series = DataSeries::load_csv(&args.csv, &args.date_col, &args.price_col)
        .with_context(|| format!("failed to load `{}`", args.csv.display()))?;
    let ret_type = match args.returns {
        ReturnsArg::Simple => ReturnsType::Simple,
        ReturnsArg::Log => ReturnsType::Logarithmic,
        ReturnsArg::Abs => ReturnsType::Absolute,
    };
    let mean_mode = if args.zero_mean {
        VolatilityMean::ZeroMean
    } else {
        VolatilityMean::SampleMean
    };
    let vol = realized_volatility(&series, args.window, ret_type, args.ann_factor, mean_mode)?;
    let rows: Vec<(NaiveDate, f64)> = vol.iter().collect();

    if args.json {
        #[derive(Serialize)]
        struct VolPoint {
            date: NaiveDate,
            volatility_pct: f64,
        }
        let tail_start = rows.len().saturating_sub(args.tail);
        let payload: Vec<VolPoint> = rows[tail_start..]
            .iter()
            .map(|&(date, volatility_pct)| VolPoint {
                date,
                volatility_pct,
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    let mode_label = match mean_mode {
        VolatilityMean::SampleMean => "SampleMean (N-1)",
        VolatilityMean::ZeroMean => "ZeroMean (N RMS)",
        _ => "Custom",
    };
    println!(
        "Realized Volatility (window={}, returns={:?}, mode={}, ann={}):",
        args.window, ret_type, mode_label, args.ann_factor
    );
    let tail_start = rows.len().saturating_sub(args.tail);
    for (date, v) in &rows[tail_start..] {
        println!("  {date}  {v:.4}%");
    }
    Ok(())
}
