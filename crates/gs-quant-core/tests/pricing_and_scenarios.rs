//! Integration tests for analytical pricing, Greeks, shock scenarios, and forward rolling.

use std::collections::BTreeSet;

use chrono::{Duration, NaiveDate};
use gs_quant_core::{
    ExpirySpec, ForwardRealisation, Instrument, LocalPricer, MarketDataPattern, MarketDataShock,
    MarketDataShockBasedScenario, MarketSnapshot, OptionType, Pricer, RiskMeasure, RollFwd,
    StrikeSpec, Tenor,
};

#[test]
fn fx_option_put_call_parity_and_finite_diff_greeks() {
    let d = NaiveDate::from_ymd_opt(2025, 1, 2).unwrap();
    let exp = d + Duration::days(180);
    let spot = 1.10;
    let strike = 1.08;
    let vol = 0.095;
    let r_usd = 0.045;
    let r_eur = 0.028;
    let notional = 1_000_000.0;

    let snap = MarketSnapshot::new(d)
        .with_fx_spot("EURUSD", spot)
        .with_fx_vol("EURUSD", "ATM", vol)
        .with_risk_free_rate("USD", r_usd)
        .with_risk_free_rate("EUR", r_eur);

    let call = Instrument::fx_option(
        "C",
        "EURUSD",
        notional,
        OptionType::Call,
        StrikeSpec::Absolute(strike),
        ExpirySpec::FixedDate(exp),
    );
    let put = Instrument::fx_option(
        "P",
        "EURUSD",
        notional,
        OptionType::Put,
        StrikeSpec::Absolute(strike),
        ExpirySpec::FixedDate(exp),
    );

    let pricer = LocalPricer::new();
    let c_pv = pricer
        .calc(&call, &snap, &RiskMeasure::DollarPrice)
        .unwrap()
        .value;
    let p_pv = pricer
        .calc(&put, &snap, &RiskMeasure::DollarPrice)
        .unwrap()
        .value;
    let t = 180.0 / 365.0;
    let parity_rhs = notional * (spot * (-r_eur * t).exp() - strike * (-r_usd * t).exp());
    assert!(
        (c_pv - p_pv - parity_rhs).abs() < 1e-4,
        "FX put-call parity mismatch: lhs={}, rhs={parity_rhs}",
        c_pv - p_pv
    );

    // Finite-difference checks for FxDelta, FxGamma, FxVega
    let eps_s = 1e-5;
    let snap_up = MarketDataShockBasedScenario::new()
        .with_shock(
            MarketDataPattern::any()
                .with_type("FX")
                .with_asset("EURUSD"),
            MarketDataShock::Absolute(eps_s),
        )
        .apply(&snap)
        .snapshot;
    let snap_dn = MarketDataShockBasedScenario::new()
        .with_shock(
            MarketDataPattern::any()
                .with_type("FX")
                .with_asset("EURUSD"),
            MarketDataShock::Absolute(-eps_s),
        )
        .apply(&snap)
        .snapshot;

    let pv_up = pricer
        .calc(&call, &snap_up, &RiskMeasure::DollarPrice)
        .unwrap()
        .value;
    let pv_dn = pricer
        .calc(&call, &snap_dn, &RiskMeasure::DollarPrice)
        .unwrap()
        .value;
    let fd_delta = (pv_up - pv_dn) / (2.0 * eps_s);
    let fd_gamma = (pv_up - 2.0 * c_pv + pv_dn) / (eps_s * eps_s);

    let ana_delta = pricer
        .calc(&call, &snap, &RiskMeasure::FxDelta)
        .unwrap()
        .value;
    let ana_gamma = pricer
        .calc(&call, &snap, &RiskMeasure::FxGamma)
        .unwrap()
        .value;
    assert!(
        (ana_delta - fd_delta).abs() < 1e-1,
        "delta {ana_delta} vs {fd_delta}"
    );
    assert!(
        (ana_gamma - fd_gamma).abs() < 5.0,
        "gamma {ana_gamma} vs {fd_gamma}"
    );

    let eps_v = 1e-5;
    let snap_v_up = MarketDataShockBasedScenario::new()
        .with_shock(
            MarketDataPattern::any()
                .with_type("FX VOL")
                .with_asset("EURUSD"),
            MarketDataShock::Absolute(eps_v),
        )
        .apply(&snap)
        .snapshot;
    let pv_v_up = pricer
        .calc(&call, &snap_v_up, &RiskMeasure::DollarPrice)
        .unwrap()
        .value;
    let fd_vega_per_pt = (pv_v_up - c_pv) / eps_v * 0.01;
    let ana_vega = pricer
        .calc(&call, &snap, &RiskMeasure::FxVega)
        .unwrap()
        .value;
    assert!(
        (ana_vega - fd_vega_per_pt).abs() < 5.0,
        "vega {ana_vega} vs {fd_vega_per_pt}"
    );
}

#[test]
fn eq_option_delta_gamma_and_scale_shock() {
    let d = NaiveDate::from_ymd_opt(2025, 3, 1).unwrap();
    let exp = d + Duration::days(90);
    let snap = MarketSnapshot::new(d)
        .with_eq_spot(".SPX", 5000.0)
        .with_eq_vol(".SPX", "ATM", 0.16)
        .with_risk_free_rate("USD", 0.04);

    let call = Instrument::eq_option(
        "SPX_Call",
        ".SPX",
        10.0,
        100.0,
        OptionType::Call,
        StrikeSpec::AtmSpot,
        ExpirySpec::FixedDate(exp),
    )
    .resolve(d, &snap, &BTreeSet::new())
    .unwrap();
    let pricer = LocalPricer::new();
    let base_pv = pricer
        .calc(&call, &snap, &RiskMeasure::DollarPrice)
        .unwrap()
        .value;
    let eq_delta = pricer
        .calc(&call, &snap, &RiskMeasure::EqDelta)
        .unwrap()
        .value;
    let eq_gamma = pricer
        .calc(&call, &snap, &RiskMeasure::EqGamma)
        .unwrap()
        .value;

    // 1% spot bump: Taylor ≈ EqDelta * 0.01 + 50 * EqGamma * 0.01^2
    let x = 0.01;
    let bumped = MarketDataShockBasedScenario::new()
        .with_shock(
            MarketDataPattern::any().with_type("EQ").with_asset(".SPX"),
            MarketDataShock::Proportional(x),
        )
        .apply(&snap);
    assert_eq!(bumped.matched_points, 1);
    let bump_pv = pricer
        .calc(&call, &bumped.snapshot, &RiskMeasure::DollarPrice)
        .unwrap()
        .value;
    let actual_diff = bump_pv - base_pv;
    let taylor_diff = eq_delta * x + 50.0 * eq_gamma * x * x;
    assert!(
        (actual_diff - taylor_diff).abs() / actual_diff.abs() < 0.01,
        "Eq Taylor mismatch: actual={actual_diff}, taylor={taylor_diff}"
    );

    // EQ VOL Scale shock (+10% multiplier via Absolute(0.1))
    let vol_shocked = MarketDataShockBasedScenario::new()
        .with_shock(
            MarketDataPattern::any()
                .with_type("EQ VOL")
                .with_asset(".SPX")
                .with_class("Scale")
                .with_quoting_style("Value"),
            MarketDataShock::Absolute(0.1),
        )
        .apply(&snap);
    assert_eq!(vol_shocked.matched_points, 1);
    let new_vol = vol_shocked.snapshot.eq_vol(".SPX", "ATM").unwrap();
    assert!((new_vol - 0.176).abs() < 1e-12);

    // Misspelled asset matches 0 points cleanly instead of inventing matches
    let noop = MarketDataShockBasedScenario::new()
        .with_shock(
            MarketDataPattern::any()
                .with_type("EQ VOL")
                .with_asset(".NOPE")
                .with_class("Scale"),
            MarketDataShock::Absolute(0.1),
        )
        .apply(&snap);
    assert_eq!(noop.matched_points, 0);
}

#[test]
fn roll_fwd_spot_modes_and_resolved_decay() {
    let d0 = NaiveDate::from_ymd_opt(2025, 1, 2).unwrap();
    let d1 = NaiveDate::from_ymd_opt(2025, 2, 2).unwrap();
    let snap = MarketSnapshot::new(d0)
        .with_fx_spot("EURUSD", 1.10)
        .with_fx_vol("EURUSD", "ATM", 0.09)
        .with_risk_free_rate("USD", 0.05)
        .with_risk_free_rate("EUR", 0.02);

    let resolved = Instrument::fx_option(
        "EURUSD_3m",
        "EURUSD",
        1_000_000.0,
        OptionType::Call,
        StrikeSpec::AtmSpot,
        ExpirySpec::Relative(Tenor::Months(3)),
    )
    .resolve(d0, &snap, &BTreeSet::new())
    .unwrap();

    let roll_fixed = RollFwd::new(d1, ForwardRealisation::KeepSpotFixed)
        .apply(&snap, std::slice::from_ref(&resolved))
        .unwrap();
    assert!((roll_fixed.fx_spot("EURUSD").unwrap() - 1.10).abs() < 1e-12);

    let roll_fwd = RollFwd::new(d1, ForwardRealisation::SpotMovesToForward)
        .apply(&snap, std::slice::from_ref(&resolved))
        .unwrap();
    let expected_fwd = 1.10 * ((0.05_f64 - 0.02) * (31.0 / 365.0)).exp();
    assert!((roll_fwd.fx_spot("EURUSD").unwrap() - expected_fwd).abs() < 1e-9);

    let pricer = LocalPricer::new();
    let pv_t0 = pricer
        .calc(&resolved, &snap, &RiskMeasure::DollarPrice)
        .unwrap()
        .value;
    let pv_fixed = pricer
        .calc(&resolved, &roll_fixed, &RiskMeasure::DollarPrice)
        .unwrap()
        .value;
    assert!(pv_fixed < pv_t0, "rolled option must experience time decay");
}
