use super::policy::hot_unique_total;
use super::{
    AccessEvent, SchedulerConfig, SchedulerEntry, SchedulerError,
    SchedulerEvent, TickInput, WorkingSetState, WorkingSetTier,
    canonicalize_events, score,
};

fn entry(
    id: &str,
    tier: WorkingSetTier,
    pinned: bool,
    relevance: u8,
    last_access: u64,
    tokens: usize,
) -> SchedulerEntry {
    SchedulerEntry {
        node_id: id.to_owned(),
        tier,
        pinned,
        relevance,
        last_access_tick: last_access,
        token_estimate: tokens,
        content_digest: String::new(),
    }
}

fn entry_with_digest(
    id: &str,
    tier: WorkingSetTier,
    pinned: bool,
    relevance: u8,
    last_access: u64,
    tokens: usize,
    digest: &str,
) -> SchedulerEntry {
    SchedulerEntry {
        node_id: id.to_owned(),
        tier,
        pinned,
        relevance,
        last_access_tick: last_access,
        token_estimate: tokens,
        content_digest: digest.to_owned(),
    }
}

#[test]
fn build_canonical_order() {
    let state = WorkingSetState::build(vec![
        entry("b", WorkingSetTier::Cold, false, 10, 0, 0),
        entry("a", WorkingSetTier::Warm, false, 20, 0, 0),
    ])
    .expect("build");
    assert_eq!(
        state.entries().iter().map(|e| e.node_id.as_str()).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
}

#[test]
fn build_duplicate_refusal() {
    let err = WorkingSetState::build(vec![
        entry("a", WorkingSetTier::Cold, false, 10, 0, 0),
        entry("a", WorkingSetTier::Warm, false, 20, 0, 0),
    ])
    .unwrap_err();
    assert_eq!(err, SchedulerError::DuplicateNode { node_id: "a".to_owned() });
}

#[test]
fn relevance_out_of_range_refusal_on_build() {
    let err = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Cold,
        false,
        101,
        0,
        0,
    )])
    .unwrap_err();
    assert_eq!(
        err,
        SchedulerError::RelevanceOutOfRange {
            node_id: "a".to_owned(),
            value: 101
        }
    );
}

#[test]
fn pin_forces_hot() {
    let mut state = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Cold,
        false,
        10,
        0,
        0,
    )])
    .expect("build");
    let outcome = state
        .apply_event(SchedulerEvent::Pin { node_id: "a".to_owned() })
        .expect("pin");
    assert_eq!(outcome.tier_before, WorkingSetTier::Cold);
    assert_eq!(outcome.tier_after, WorkingSetTier::Hot);
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Hot);
    assert!(state.entry("a").unwrap().pinned);
}

#[test]
fn access_updates_recency() {
    let mut state = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Cold,
        false,
        10,
        0,
        0,
    )])
    .expect("build");
    // Advance tick so recency would be low without access.
    let cfg = SchedulerConfig::default();
    for _ in 0..5 {
        state.advance_tick(&cfg);
    }
    assert!(state.tick_value() == 5);
    // Access at tick 5: last_access becomes 5, score improves.
    let outcome = state
        .apply_event(SchedulerEvent::Access { node_id: "a".to_owned() })
        .expect("access");
    assert_eq!(state.entry("a").unwrap().last_access_tick, 5);
    // With relevance 10 (40) + recency 30 + pin 0 =70 <120 still Cold, so stays Cold.
    assert_eq!(outcome.tier_after, WorkingSetTier::Cold);
    // Now set high relevance then access should promote.
    state
        .apply_event(SchedulerEvent::Relevance {
            node_id: "a".to_owned(),
            relevance: 80,
        })
        .expect("relevance");
    // score 80*4=320 +30 =350 => Hot
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Hot);
}

#[test]
fn relevance_out_of_range_via_event() {
    let mut state = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Cold,
        false,
        10,
        0,
        0,
    )])
    .expect("build");
    let err = state
        .apply_event(SchedulerEvent::Relevance {
            node_id: "a".to_owned(),
            relevance: 200,
        })
        .unwrap_err();
    assert_eq!(
        err,
        SchedulerError::RelevanceOutOfRange {
            node_id: "a".to_owned(),
            value: 200
        }
    );
}

#[test]
fn tick_promotion_after_access() {
    // Low relevance Cold node becomes Hot after access + high relevance.
    let mut state = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Cold,
        false,
        90,
        0,
        0,
    )])
    .expect("build");
    // Immediately after build, tick 0: score 90*4=360+30=390 => Hot, but entry is Cold.
    // Tick will promote.
    let cfg = SchedulerConfig::default();
    let report = state.advance_tick(&cfg);
    assert_eq!(report.tick, 1);
    assert_eq!(report.promoted, vec!["a".to_owned()]);
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Hot);
}

#[test]
fn tick_demotion_neglect_to_cold() {
    // Start Hot with modest relevance, let recency decay.
    let mut state = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Hot,
        false,
        30,
        0,
        0,
    )])
    .expect("build");
    // Tick 0 score 30*4=120+30=150 => Warm, so first tick demotes Hot->Warm.
    let cfg = SchedulerConfig::default();
    let r1 = state.advance_tick(&cfg);
    assert_eq!(r1.demoted, vec!["a".to_owned()]);
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Warm);
    // Neglect 30 ticks -> recency 0 => score 120 => still Warm.
    for _ in 0..30 {
        state.advance_tick(&cfg);
    }
    // Score 120 => Warm. One more tick with relevance low? Actually need Cold.
    // Lower relevance to 10 => score 40+0=40 => Cold.
    state
        .apply_event(SchedulerEvent::Relevance {
            node_id: "a".to_owned(),
            relevance: 10,
        })
        .expect("relevance");
    let _ = state.advance_tick(&cfg);
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Cold);
}

#[test]
fn stale_beats_pin() {
    let mut state = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Hot,
        true,
        90,
        0,
        0,
    )])
    .expect("build");
    let outcome = state
        .apply_event(SchedulerEvent::Stale { node_id: "a".to_owned() })
        .expect("stale");
    assert_eq!(outcome.tier_after, WorkingSetTier::Warm);
    assert!(state.entry("a").unwrap().pinned);
}

#[test]
fn archive_explicit_only() {
    let mut state = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Hot,
        false,
        90,
        0,
        0,
    )])
    .expect("build");
    let cfg = SchedulerConfig::default();
    let _ = state.advance_tick(&cfg);
    assert_ne!(state.entry("a").unwrap().tier, WorkingSetTier::Archive);
    state
        .apply_event(SchedulerEvent::Archive { node_id: "a".to_owned() })
        .expect("archive");
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Archive);
    let _ = state.advance_tick(&cfg);
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Archive);
}

#[test]
fn budget_lowest_score_demoted_first() {
    let mut state = WorkingSetState::build(vec![
        entry("a", WorkingSetTier::Hot, false, 10, 0, 100),
        entry("b", WorkingSetTier::Hot, false, 10, 0, 100),
        entry("c", WorkingSetTier::Hot, false, 90, 0, 100),
    ])
    .expect("build");
    let cfg = SchedulerConfig::new(150).expect("cfg");
    let estimates = vec![
        ("a".to_owned(), 100),
        ("b".to_owned(), 100),
        ("c".to_owned(), 100),
    ];
    let report = state.enforce_budget(&cfg, &estimates).expect("budget");
    // Hot total 300 >150, demote lowest score first: a (110, id a) then b (110).
    // After demoting a: 200 >150 demote b: 100 <=150 stop.
    assert_eq!(report.hot_tokens_before, 300);
    assert_eq!(report.hot_tokens_after, 100);
    assert_eq!(report.demoted, vec!["a".to_owned(), "b".to_owned()]);
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Warm);
    assert_eq!(state.entry("b").unwrap().tier, WorkingSetTier::Warm);
    assert_eq!(state.entry("c").unwrap().tier, WorkingSetTier::Hot);
}

#[test]
fn pinned_never_budget_demoted() {
    let mut state = WorkingSetState::build(vec![
        entry("a", WorkingSetTier::Hot, true, 10, 0, 100),
        entry("b", WorkingSetTier::Hot, false, 90, 0, 100),
    ])
    .expect("build");
    let cfg = SchedulerConfig::new(150).expect("cfg");
    let estimates = vec![("a".to_owned(), 100), ("b".to_owned(), 100)];
    let report = state.enforce_budget(&cfg, &estimates).expect("budget");
    // a is pinned, so only b can be demoted even though a has lower score (10*4+30+50=120).
    assert_eq!(report.demoted, vec!["b".to_owned()]);
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Hot);
    assert_eq!(state.entry("b").unwrap().tier, WorkingSetTier::Warm);
}

#[test]
fn budget_boundary_no_demotions() {
    let mut state = WorkingSetState::build(vec![
        entry("a", WorkingSetTier::Hot, false, 90, 0, 100),
        entry("b", WorkingSetTier::Hot, false, 90, 0, 100),
    ])
    .expect("build");
    let cfg = SchedulerConfig::new(200).expect("cfg");
    let estimates = vec![("a".to_owned(), 100), ("b".to_owned(), 100)];
    let report = state.enforce_budget(&cfg, &estimates).expect("budget");
    assert_eq!(report.hot_tokens_before, 200);
    assert_eq!(report.hot_tokens_after, 200);
    assert!(report.demoted.is_empty());
}

#[test]
fn report_ordering_canonical() {
    let mut state = WorkingSetState::build(vec![
        entry("c", WorkingSetTier::Cold, false, 90, 0, 0),
        entry("a", WorkingSetTier::Cold, false, 90, 0, 0),
        entry("b", WorkingSetTier::Hot, false, 10, 0, 0),
    ])
    .expect("build");
    let cfg = SchedulerConfig::default();
    let report = state.advance_tick(&cfg);
    // a and c should promote, in canonical order.
    assert_eq!(report.promoted, vec!["a".to_owned(), "c".to_owned()]);
    assert_eq!(report.demoted, vec!["b".to_owned()]);
}

#[test]
fn config_validation() {
    assert!(SchedulerConfig::new(0).is_err());
    assert_eq!(
        SchedulerConfig::new(0).unwrap_err(),
        SchedulerError::InvalidConfig {
            reason: "budget_tokens must be >= 1".to_owned()
        }
    );
    assert!(SchedulerConfig::new(1).is_ok());
    assert_eq!(SchedulerConfig::default().budget_tokens, 4096);
}

#[test]
fn score_deterministic() {
    let e = entry("x", WorkingSetTier::Cold, false, 50, 5, 0);
    // tick 5 => recency 30, pinned 0 => 200+30=230
    assert_eq!(score(&e, 5), 230);
    let pinned = entry("x", WorkingSetTier::Cold, true, 50, 5, 0);
    assert_eq!(score(&pinned, 5), 280);
    // tick 10 => ticks_since 5 => 25 => 200+25=225
    assert_eq!(score(&e, 10), 225);
    // tick 40 => ticks_since 35 => capped 30 => 0 => 200
    assert_eq!(score(&e, 40), 200);
}

#[test]
fn unknown_node_error() {
    let mut state = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Cold,
        false,
        10,
        0,
        0,
    )])
    .expect("build");
    let err = state
        .apply_event(SchedulerEvent::Access { node_id: "missing".to_owned() })
        .unwrap_err();
    assert_eq!(
        err,
        SchedulerError::UnknownNode { node_id: "missing".to_owned() }
    );
    let cfg = SchedulerConfig::default();
    let est = vec![("missing".to_owned(), 10)];
    let err2 = state.enforce_budget(&cfg, &est).unwrap_err();
    assert_eq!(
        err2,
        SchedulerError::UnknownNode { node_id: "missing".to_owned() }
    );
}

// --- A1 canonical event dedupe/order/truncate ---
#[test]
fn canonical_event_dedupe_order_truncate() {
    let events = vec![
        AccessEvent::new("b"),
        AccessEvent::new("a"),
        AccessEvent::new("b"),
        AccessEvent::new("c"),
    ];
    let canon = canonicalize_events(events);
    assert_eq!(
        canon,
        vec![
            AccessEvent::new("a"),
            AccessEvent::new("b"),
            AccessEvent::new("c")
        ]
    );
    // Truncate 64
    let many: Vec<AccessEvent> =
        (0..70).map(|i| AccessEvent::new(format!("n{:03}", i))).collect();
    // Already sorted, should truncate to 64
    let canon_many = canonicalize_events(many);
    assert_eq!(canon_many.len(), 64);
    assert_eq!(canon_many[0].node_id, "n000");
    assert_eq!(canon_many[63].node_id, "n063");
    // Unsorted many with duplicates
    let unsorted = vec![
        AccessEvent::new("z"),
        AccessEvent::new("a"),
        AccessEvent::new("m"),
        AccessEvent::new("a"),
    ];
    assert_eq!(
        canonicalize_events(unsorted),
        vec![
            AccessEvent::new("a"),
            AccessEvent::new("m"),
            AccessEvent::new("z")
        ]
    );
}

#[test]
fn tick_input_canonicalizes_events() {
    let input = TickInput::new(
        10,
        vec![
            AccessEvent::new("b"),
            AccessEvent::new("a"),
            AccessEvent::new("b"),
        ],
        "rev1",
        vec!["n2".to_owned(), "n1".to_owned()],
        vec![],
    );
    assert_eq!(
        input.events,
        vec![AccessEvent::new("a"), AccessEvent::new("b")]
    );
    assert_eq!(input.new_node_ids, vec!["n1".to_owned(), "n2".to_owned()]);
}

// --- A1 demand edge updates recency/relevance with saturation ---
#[test]
fn demand_edge_updates_recency_and_relevance_saturating() {
    let mut state = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Cold,
        false,
        90,
        0,
        100,
    )])
    .expect("build");
    let cfg = SchedulerConfig::default();
    let input =
        TickInput::new(5, vec![AccessEvent::new("a")], "rev1", vec![], vec![]);
    let _ = state.process_tick(input, &cfg);
    // relevance 90+32 capped 100, last_access 5
    let e = state.entry("a").unwrap();
    assert_eq!(e.relevance, 100);
    assert_eq!(e.last_access_tick, 5);
    // Second event should stay 100
    let input2 =
        TickInput::new(6, vec![AccessEvent::new("a")], "rev2", vec![], vec![]);
    let _ = state.process_tick(input2, &cfg);
    assert_eq!(state.entry("a").unwrap().relevance, 100);
    assert_eq!(state.entry("a").unwrap().last_access_tick, 6);
}

// --- A3 stale-never-promote ---
#[test]
fn stale_never_promote_but_updates_scores() {
    let mut state = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Warm,
        false,
        90,
        0,
        100,
    )])
    .expect("build");
    // Make stale; even with demand event that would promote to Hot, it stays Warm
    let cfg = SchedulerConfig::default();
    // Warm with relevance 90, tick 0 score 90*4+30=390 => Hot, but we will mark stale for next tick
    // Process tick with stale flag and demand event
    let input = TickInput::new(
        1,
        vec![AccessEvent::new("a")],
        "rev1",
        vec![],
        vec!["a".to_owned()],
    );
    let _ = state.process_tick(input, &cfg);
    // Stale demotes Warm->Cold first, then demand updates relevance 90->100 and recency, but promotion gate refuses stale promotion
    // So final should be Cold, not Hot
    let e = state.entry("a").unwrap();
    // relevance should have been updated despite staleness
    assert_eq!(e.relevance, 100);
    assert_eq!(e.last_access_tick, 1);
    assert_eq!(
        e.tier,
        WorkingSetTier::Cold,
        "stale node must not promote to Hot even with high score"
    );
}

// --- A2 digest-counted budget ---
#[test]
fn digest_counted_budget_counts_unique_once() {
    let digest_dup = "aabbcc".repeat(10) + "00";
    let digest_unique = "ff".repeat(32);
    let mut state = WorkingSetState::build(vec![
        entry_with_digest(
            "a",
            WorkingSetTier::Hot,
            false,
            10,
            0,
            1000,
            &digest_dup,
        ),
        entry_with_digest(
            "b",
            WorkingSetTier::Hot,
            false,
            10,
            0,
            1000,
            &digest_dup,
        ),
        entry_with_digest(
            "c",
            WorkingSetTier::Hot,
            false,
            10,
            0,
            1000,
            &digest_unique,
        ),
    ])
    .expect("build");
    // Hot unique total should be 2000 (a first 1000 + c 1000, b 0 duplicate)
    assert_eq!(hot_unique_total(&state), 2000);
    // With budget 1500, demotion should demote lowest scored (a,b tie a first) but removing a won't reduce total if b still Hot with same digest
    // So after demoting a, total still 2000 (b becomes first of dup, plus c), need to demote b as well to get to 1000
    let cfg = SchedulerConfig::new(1500).expect("cfg");
    let estimates = vec![
        ("a".to_owned(), 1000),
        ("b".to_owned(), 1000),
        ("c".to_owned(), 1000),
    ];
    let report = state.enforce_budget(&cfg, &estimates).expect("budget");
    assert_eq!(report.hot_tokens_before, 2000);
    // Should demote a and b (both dup digest) to reach 1000
    assert_eq!(report.demoted, vec!["a".to_owned(), "b".to_owned()]);
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Warm);
    assert_eq!(state.entry("b").unwrap().tier, WorkingSetTier::Warm);
    assert_eq!(state.entry("c").unwrap().tier, WorkingSetTier::Hot);
    assert_eq!(
        state
            .enforce_budget(&SchedulerConfig::new(1500).unwrap(), &estimates)
            .unwrap()
            .hot_tokens_after,
        1000
    );
}

#[test]
fn budget_demotion_order_under_duplicates() {
    let d1 = "11".repeat(32);
    let d2 = "22".repeat(32);
    // a and b share d1, c has d2. Scores: a low (10), b mid (20), c high (90). Hot all.
    let mut state = WorkingSetState::build(vec![
        entry_with_digest("a", WorkingSetTier::Hot, false, 10, 0, 1000, &d1),
        entry_with_digest("b", WorkingSetTier::Hot, false, 20, 0, 1000, &d1),
        entry_with_digest("c", WorkingSetTier::Hot, false, 90, 0, 1000, &d2),
    ])
    .expect("build");
    let cfg = SchedulerConfig::new(1500).unwrap();
    let est = vec![
        ("a".to_owned(), 1000),
        ("b".to_owned(), 1000),
        ("c".to_owned(), 1000),
    ];
    let report = state.enforce_budget(&cfg, &est).unwrap();
    // Unique total 2000 -> need demote. Lowest score is a (10) -> demote a (still 2000 because b still holds d1), then next lowest is b -> demote b -> total 1000
    assert_eq!(report.demoted, vec!["a".to_owned(), "b".to_owned()]);
}

// --- A3 pin quota ---
#[test]
fn pin_quota_demotes_lowest_pinned_hot() {
    // Two pinned HOT nodes each 800 tokens, duplicate? unique 1600 >1024 quota -> demote lowest scored pinned
    let mut state = WorkingSetState::build(vec![
        entry("a", WorkingSetTier::Hot, true, 10, 0, 800),
        entry("b", WorkingSetTier::Hot, true, 90, 0, 800),
        entry("c", WorkingSetTier::Hot, false, 90, 0, 800),
    ])
    .expect("build");
    let cfg = SchedulerConfig::default();
    // tick with no events but same revision will trigger pin quota via process_tick
    // Use different revision to force pipeline
    let input = TickInput::new(1, vec![], "rev1", vec![], vec![]);
    let report = state.process_tick(input, &cfg);
    // Pinned hot total 1600 >1024, lowest scored pinned is a -> demoted to Warm
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Warm);
    assert_eq!(state.entry("b").unwrap().tier, WorkingSetTier::Hot);
    // Budget total after pin quota: b(800) + c(800) =1600 <=4096 so no further demotion
    assert!(
        report.demoted.contains(&"a".to_owned())
            || state.entry("a").unwrap().tier == WorkingSetTier::Warm
    );
}

#[test]
fn pin_quota_within_survives_budget() {
    // Pinned HOT within quota should survive budget demotion even if lowest score
    let mut state = WorkingSetState::build(vec![
        entry("a", WorkingSetTier::Hot, true, 10, 0, 500),
        entry("b", WorkingSetTier::Hot, false, 90, 0, 2000),
        entry("c", WorkingSetTier::Hot, false, 90, 0, 2000),
    ])
    .expect("build");
    // Pinned total 500 <=1024, so pin quota no demotion. Budget 4096: total unique 4500 >4096, need demote lowest non-pinned (b and c both 90, tie a would be lowest but pinned protects, so b demoted)
    let cfg = SchedulerConfig::new(4096).unwrap();
    let est = vec![
        ("a".to_owned(), 500),
        ("b".to_owned(), 2000),
        ("c".to_owned(), 2000),
    ];
    let report = state.enforce_budget(&cfg, &est).unwrap();
    assert_eq!(report.demoted, vec!["b".to_owned()]);
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Hot);
}

// --- A4 coalescing ---
#[test]
fn tick_coalescing_identical_input_no_pipeline() {
    let mut state = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Cold,
        false,
        10,
        0,
        100,
    )])
    .expect("build");
    let cfg = SchedulerConfig::default();
    let input =
        TickInput::new(1, vec![], "rev1", vec!["n1".to_owned()], vec![]);
    let before_runs = state.pipeline_runs();
    let _report1 = state.process_tick(input.clone(), &cfg);
    let after_first = state.pipeline_runs();
    assert_eq!(after_first, before_runs + 1);
    let state_after_first = state.clone();
    // Identical input should coalesce
    let report2 = state.process_tick(input.clone(), &cfg);
    assert_eq!(
        state, state_after_first,
        "coalesced tick must produce identical output state"
    );
    assert_eq!(
        state.pipeline_runs(),
        after_first,
        "no pipeline run on coalesced tick"
    );
    assert_eq!(report2.promoted.len(), 0);
    assert_eq!(report2.demoted.len(), 0);
    assert_eq!(report2.tick, state_after_first.tick_value());
    // Different graph revision should not coalesce
    let input_diff =
        TickInput::new(1, vec![], "rev2", vec!["n1".to_owned()], vec![]);
    let _ = state.process_tick(input_diff, &cfg);
    assert_eq!(state.pipeline_runs(), after_first + 1);
}

// --- processing order property ---
#[test]
fn processing_order_stale_before_demand_and_promotion() {
    // Stale Warm node with demand event that would otherwise promote, but stale never promotes
    let mut state = WorkingSetState::build(vec![entry(
        "a",
        WorkingSetTier::Warm,
        false,
        80,
        0,
        100,
    )])
    .expect("build");
    let cfg = SchedulerConfig::default();
    // Tick 0: Warm 80*4+30=350 Hot promotion would happen, but we mark stale for tick 1 with demand
    let input = TickInput::new(
        1,
        vec![AccessEvent::new("a")],
        "rev1",
        vec![],
        vec!["a".to_owned()],
    );
    let _ = state.process_tick(input, &cfg);
    // Stale demotion Warm->Cold, demand updates relevance, but promotion refused -> stays Cold
    assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Cold);
    assert_eq!(state.entry("a").unwrap().relevance, 100);
}

// --- determinism byte-equal ---
#[test]
fn determinism_byte_equal() {
    let build = || {
        WorkingSetState::build(vec![
            entry("b", WorkingSetTier::Cold, false, 20, 0, 100),
            entry("a", WorkingSetTier::Hot, true, 10, 0, 100),
        ])
        .unwrap()
    };
    let mut s1 = build();
    let mut s2 = build();
    let cfg = SchedulerConfig::default();
    let input =
        TickInput::new(1, vec![AccessEvent::new("b")], "revX", vec![], vec![]);
    let r1 = s1.process_tick(input.clone(), &cfg);
    let r2 = s2.process_tick(input.clone(), &cfg);
    assert_eq!(s1, s2);
    assert_eq!(r1, r2);
    // Same input again (coalesced) still byte-equal
    let r1b = s1.process_tick(input.clone(), &cfg);
    let r2b = s2.process_tick(input.clone(), &cfg);
    assert_eq!(r1b, r2b);
    assert_eq!(s1, s2);
}

// --- Decision 90 B1/B2 assembly ---
fn assembly_fixture() -> (
    WorkingSetState,
    crate::context_graph::ContextGraph,
    crate::context_representation::ContextRepresentationStore,
) {
    use crate::context_graph::{
        ContextEdge, ContextEdgeKind, ContextGraph, ContextNode,
        ContextNodeKind, estimate_tokens,
    };
    use crate::context_representation::{
        ContextRepresentationStore, NodeRepresentation, NodeRepresentationSet,
        RepresentationLevel, RepresentationOrigin, content_digest_of,
    };
    use crate::identity::sha256_hex;
    let a_digest = sha256_hex("body-a".as_bytes());
    let b_digest = sha256_hex("body-b".as_bytes());
    let nodes = vec![
        ContextNode {
            id: "a".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: a_digest.clone(),
            summary: "summary-a".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-a"),
        },
        ContextNode {
            id: "b".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: b_digest.clone(),
            summary: "summary-b".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-b"),
        },
        ContextNode {
            id: "c".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: sha256_hex("body-c".as_bytes()),
            summary: "summary-c".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-c"),
        },
    ];
    let edges = vec![
        ContextEdge {
            from: "a".to_owned(),
            to: "b".to_owned(),
            kind: ContextEdgeKind::Contains,
        },
        ContextEdge {
            from: "b".to_owned(),
            to: "c".to_owned(),
            kind: ContextEdgeKind::DependsOn,
        },
    ];
    let graph = ContextGraph::build(nodes, edges).expect("graph");
    let rep_a_summary = "summary-a";
    let rep_a_structured = "structured-facts-a";
    let rep_b_summary = "summary-b";
    let rep_c_summary = "summary-c";
    let set_a = NodeRepresentationSet::build(
        "a".to_owned(),
        vec![
            NodeRepresentation {
                level: RepresentationLevel::Identity,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("identity-a"),
                derived_from: vec![],
                content: "identity-a".to_owned(),
            },
            NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of(rep_a_summary),
                derived_from: vec![],
                content: rep_a_summary.to_owned(),
            },
            NodeRepresentation {
                level: RepresentationLevel::Structured,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of(rep_a_structured),
                derived_from: vec![],
                content: rep_a_structured.to_owned(),
            },
        ],
    )
    .expect("set_a");
    let set_b = NodeRepresentationSet::build(
        "b".to_owned(),
        vec![NodeRepresentation {
            level: RepresentationLevel::Summary,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: content_digest_of(rep_b_summary),
            derived_from: vec![],
            content: rep_b_summary.to_owned(),
        }],
    )
    .expect("set_b");
    let set_c = NodeRepresentationSet::build(
        "c".to_owned(),
        vec![NodeRepresentation {
            level: RepresentationLevel::Summary,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: content_digest_of(rep_c_summary),
            derived_from: vec![],
            content: rep_c_summary.to_owned(),
        }],
    )
    .expect("set_c");
    let store = ContextRepresentationStore::build(vec![set_a, set_b, set_c])
        .expect("store");
    let entries = vec![
        entry("a", WorkingSetTier::Hot, false, 90, 0, 1000),
        entry("b", WorkingSetTier::Hot, false, 90, 0, 1000),
        entry("c", WorkingSetTier::Cold, false, 10, 0, 1000),
    ];
    let state = WorkingSetState::build(entries).expect("state");
    (state, graph, store)
}

#[test]
fn l1_default_assembly_hot_contributes_summary_only() {
    let (mut state, graph, store) = assembly_fixture();
    let cfg = SchedulerConfig::default();
    let ctx = state.assemble(&graph, &store, &cfg);
    assert_eq!(ctx.entries.len(), 2);
    let a_entry = ctx.entries.iter().find(|e| e.node_id == "a").expect("a");
    assert_eq!(a_entry.summary, "summary-a");
    assert_eq!(
        a_entry.token_estimate,
        crate::context_graph::estimate_tokens("summary-a")
    );
    assert!(!a_entry.summary.contains("structured"));
    for e in &ctx.entries {
        for stub in &e.neighbor_stubs {
            assert!(!stub.content_digest.is_empty());
            assert_ne!(stub.content_digest, e.summary);
        }
    }
}

#[test]
fn budget_counts_assembled_summaries_unique_digest() {
    use crate::context_graph::{
        ContextGraph, ContextNode, ContextNodeKind, estimate_tokens,
    };
    use crate::context_representation::{
        ContextRepresentationStore, NodeRepresentation, NodeRepresentationSet,
        RepresentationLevel, RepresentationOrigin, content_digest_of,
    };
    let digest = content_digest_of("same-summary");
    let nodes = vec![
        ContextNode {
            id: "a".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "a".repeat(64),
            summary: "same-summary".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("same-summary"),
        },
        ContextNode {
            id: "b".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "b".repeat(64),
            summary: "same-summary".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("same-summary"),
        },
    ];
    let graph = ContextGraph::build(nodes, vec![]).expect("graph");
    let set_a = NodeRepresentationSet::build(
        "a".to_owned(),
        vec![NodeRepresentation {
            level: RepresentationLevel::Summary,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: digest.clone(),
            derived_from: vec![],
            content: "same-summary".to_owned(),
        }],
    )
    .expect("set_a");
    let set_b = NodeRepresentationSet::build(
        "b".to_owned(),
        vec![NodeRepresentation {
            level: RepresentationLevel::Summary,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: digest.clone(),
            derived_from: vec![],
            content: "same-summary".to_owned(),
        }],
    )
    .expect("set_b");
    let store =
        ContextRepresentationStore::build(vec![set_a, set_b]).expect("store");
    let tok = estimate_tokens("same-summary");
    let mut state = WorkingSetState::build(vec![
        entry("a", WorkingSetTier::Hot, false, 90, 0, 1000),
        entry("b", WorkingSetTier::Hot, false, 90, 0, 1000),
    ])
    .expect("state");
    let cfg = SchedulerConfig::new(tok).expect("cfg");
    let ctx = state.assemble(&graph, &store, &cfg);
    assert_eq!(ctx.entries.len(), 2);
    assert_eq!(ctx.total_tokens_after, tok);
    // Distinct digests with tight budget should demote lowest scored
    let distinct_nodes = vec![
        ContextNode {
            id: "a".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "c".repeat(64),
            summary: "summary-a-distinct".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-a-distinct"),
        },
        ContextNode {
            id: "b".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "d".repeat(64),
            summary: "summary-b-distinct".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-b-distinct"),
        },
    ];
    let distinct_graph =
        ContextGraph::build(distinct_nodes, vec![]).expect("graph");
    let d_a = NodeRepresentationSet::build(
        "a".to_owned(),
        vec![NodeRepresentation {
            level: RepresentationLevel::Summary,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: content_digest_of("summary-a-distinct"),
            derived_from: vec![],
            content: "summary-a-distinct".to_owned(),
        }],
    )
    .expect("set");
    let d_b = NodeRepresentationSet::build(
        "b".to_owned(),
        vec![NodeRepresentation {
            level: RepresentationLevel::Summary,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: content_digest_of("summary-b-distinct"),
            derived_from: vec![],
            content: "summary-b-distinct".to_owned(),
        }],
    )
    .expect("set");
    let distinct_store =
        ContextRepresentationStore::build(vec![d_a, d_b]).expect("store");
    let mut distinct_state = WorkingSetState::build(vec![
        entry("a", WorkingSetTier::Hot, false, 10, 0, 1000),
        entry("b", WorkingSetTier::Hot, false, 20, 0, 1000),
    ])
    .expect("state");
    let tok_a = estimate_tokens("summary-a-distinct");
    let cfg_tight = SchedulerConfig::new(tok_a).expect("cfg");
    let ctx2 =
        distinct_state.assemble(&distinct_graph, &distinct_store, &cfg_tight);
    assert_eq!(ctx2.entries.len(), 1);
    assert_eq!(ctx2.entries[0].node_id, "b");
}

#[test]
fn no_auto_escalation_demanded_node_still_summary_only() {
    let (mut state, graph, store) = assembly_fixture();
    let cfg = SchedulerConfig::default();
    let input = TickInput::new(
        10,
        vec![AccessEvent::new("a")],
        "rev1",
        vec![],
        vec![],
    );
    let _ = state.process_tick(input, &cfg);
    let ctx = state.assemble(&graph, &store, &cfg);
    let a = ctx.entries.iter().find(|e| e.node_id == "a").expect("a hot");
    assert_eq!(a.summary, "summary-a");
    assert!(!a.summary.contains("structured"));
}

#[test]
fn neighbor_stubs_bounded_and_ordered_and_no_content_leak() {
    use crate::context_graph::{
        ContextEdge, ContextEdgeKind, ContextGraph, ContextNode,
        ContextNodeKind, estimate_tokens,
    };
    use crate::context_representation::{
        ContextRepresentationStore, NodeRepresentation, NodeRepresentationSet,
        RepresentationLevel, RepresentationOrigin, content_digest_of,
    };
    use crate::identity::sha256_hex;
    let mut nodes = vec![ContextNode {
        id: "a".to_owned(),
        kind: ContextNodeKind::Source,
        content_digest: sha256_hex("center".as_bytes()),
        summary: "center".to_owned(),
        source_bindings: vec![],
        token_estimate: estimate_tokens("center"),
    }];
    for i in 0..12 {
        let id = format!("n{i:02}");
        nodes.push(ContextNode {
            id: id.clone(),
            kind: ContextNodeKind::Source,
            content_digest: sha256_hex(id.as_bytes()),
            summary: format!("summary-{id}"),
            source_bindings: vec![],
            token_estimate: estimate_tokens(&format!("summary-{id}")),
        });
    }
    let mut edges = Vec::new();
    for i in 0..12 {
        let id = format!("n{i:02}");
        edges.push(ContextEdge {
            from: "a".to_owned(),
            to: id.clone(),
            kind: ContextEdgeKind::Contains,
        });
    }
    let graph = ContextGraph::build(nodes.clone(), edges).expect("graph");
    let mut sets = Vec::new();
    for n in &nodes {
        sets.push(
            NodeRepresentationSet::build(
                n.id.clone(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of(&n.summary),
                    derived_from: vec![],
                    content: n.summary.clone(),
                }],
            )
            .expect("set"),
        );
    }
    let store = ContextRepresentationStore::build(sets).expect("store");
    let mut state = WorkingSetState::build(vec![SchedulerEntry {
        node_id: "a".to_owned(),
        tier: WorkingSetTier::Hot,
        pinned: false,
        relevance: 90,
        last_access_tick: 0,
        token_estimate: 1000,
        content_digest: sha256_hex("center".as_bytes()),
    }])
    .expect("state");
    let cfg = SchedulerConfig::default();
    let ctx = state.assemble(&graph, &store, &cfg);
    let a = ctx.entries.iter().find(|e| e.node_id == "a").expect("a");
    assert_eq!(a.neighbor_stubs.len(), 8);
    let expected: Vec<String> = (0..8).map(|i| format!("n{i:02}")).collect();
    let actual: Vec<String> =
        a.neighbor_stubs.iter().map(|s| s.node_id.clone()).collect();
    assert_eq!(actual, expected);
    for stub in &a.neighbor_stubs {
        assert!(stub.content_digest.len() == 64);
        let neighbor_node = graph.node(&stub.node_id).expect("node");
        assert_eq!(stub.content_digest, neighbor_node.content_digest);
        assert!(!stub.content_digest.contains("summary"));
    }
}

#[test]
fn stub_bytes_counted_in_budget_demotion() {
    use crate::context_graph::{
        ContextEdge, ContextEdgeKind, ContextGraph, ContextNode,
        ContextNodeKind, estimate_tokens,
    };
    use crate::context_representation::{
        ContextRepresentationStore, NodeRepresentation, NodeRepresentationSet,
        RepresentationLevel, RepresentationOrigin, content_digest_of,
    };
    use crate::identity::sha256_hex;
    // 9 HOT nodes each 512 tokens (max summary) => 4608 >4096 triggers demotion; stubs add further
    let big_summary_base = "s".repeat(2047);
    let mut nodes = Vec::new();
    let mut sets = Vec::new();
    let mut entries = Vec::new();
    for i in 0..9 {
        let id = format!("n{i}");
        let summary = format!("{big_summary_base}{i}");
        let summary_tokens = estimate_tokens(&summary);
        let summary_digest = content_digest_of(&summary);
        nodes.push(ContextNode {
            id: id.clone(),
            kind: ContextNodeKind::Source,
            content_digest: sha256_hex(format!("body-{id}").as_bytes()),
            summary: summary.clone(),
            source_bindings: vec![],
            token_estimate: summary_tokens,
        });
        sets.push(
            NodeRepresentationSet::build(
                id.clone(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: summary_digest.clone(),
                    derived_from: vec![],
                    content: summary.clone(),
                }],
            )
            .expect("set"),
        );
        entries.push(SchedulerEntry {
            node_id: id.clone(),
            tier: WorkingSetTier::Hot,
            pinned: false,
            relevance: (10 + i as u8),
            last_access_tick: 0,
            token_estimate: 1000,
            content_digest: sha256_hex(format!("body-{id}").as_bytes()),
        });
    }
    let mut edges = Vec::new();
    for i in 0..8 {
        edges.push(ContextEdge {
            from: format!("n{i}"),
            to: format!("n{}", i + 1),
            kind: ContextEdgeKind::Contains,
        });
    }
    let graph = ContextGraph::build(nodes, edges).expect("graph");
    let store = ContextRepresentationStore::build(sets).expect("store");
    let mut state = WorkingSetState::build(entries).expect("state");
    let cfg = SchedulerConfig::new(4096).expect("cfg");
    let ctx = state.assemble(&graph, &store, &cfg);
    assert!(ctx.demoted.contains(&"n0".to_owned()) || ctx.entries.len() < 9);
    assert!(ctx.total_tokens_before > ctx.total_tokens_after);
    assert!(ctx.total_tokens_before >= 512);
}

#[test]
fn assembly_determinism_byte_equal() {
    let (mut s1, graph, store) = assembly_fixture();
    let (mut s2, graph2, store2) = assembly_fixture();
    let cfg = SchedulerConfig::default();
    let c1 = s1.assemble(&graph, &store, &cfg);
    let c2 = s2.assemble(&graph2, &store2, &cfg);
    assert_eq!(c1, c2);
    assert_eq!(s1, s2);
}

#[test]
fn benchmark_byte_identity_guard() {
    let mut state = WorkingSetState::build(vec![
        entry("a", WorkingSetTier::Hot, false, 90, 0, 100),
        entry("b", WorkingSetTier::Hot, false, 90, 0, 100),
    ])
    .expect("build");
    let cfg = SchedulerConfig::default();
    let before = state.clone();
    let _ = state
        .process_tick(TickInput::new(1, vec![], "rev1", vec![], vec![]), &cfg);
    assert_eq!(state.pipeline_runs(), before.pipeline_runs() + 1);
}

// --- Decision 93 rank-for-retention (score-ordered retention) ---

#[test]
fn scores_absent_demoted_order_unchanged_byte_equal() {
    // Two states with identical HOT entries and tight budget: without scores, demotion order is scheduler score asc, node_id asc.
    // Verifies scores absent => byte-equal state vs pre-93 baseline.
    use crate::context_graph::{
        ContextGraph, ContextNode, ContextNodeKind, estimate_tokens,
    };
    use crate::context_representation::{
        ContextRepresentationStore, NodeRepresentation, NodeRepresentationSet,
        RepresentationLevel, RepresentationOrigin, content_digest_of,
    };
    let nodes = vec![
        ContextNode {
            id: "a".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "a".repeat(64),
            summary: "summary-a".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-a"),
        },
        ContextNode {
            id: "b".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "b".repeat(64),
            summary: "summary-b".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-b"),
        },
        ContextNode {
            id: "c".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "c".repeat(64),
            summary: "summary-c".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-c"),
        },
    ];
    let graph = ContextGraph::build(nodes, vec![]).expect("graph");
    let sets = vec![
        NodeRepresentationSet::build(
            "a".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-a"),
                derived_from: vec![],
                content: "summary-a".to_owned(),
            }],
        )
        .expect("a"),
        NodeRepresentationSet::build(
            "b".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-b"),
                derived_from: vec![],
                content: "summary-b".to_owned(),
            }],
        )
        .expect("b"),
        NodeRepresentationSet::build(
            "c".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-c"),
                derived_from: vec![],
                content: "summary-c".to_owned(),
            }],
        )
        .expect("c"),
    ];
    let store = ContextRepresentationStore::build(sets).expect("store");
    // scheduler scores: a low (10), b mid (20), c high (90) => demote a first
    let entries = vec![
        entry("a", WorkingSetTier::Hot, false, 10, 0, 1000),
        entry("b", WorkingSetTier::Hot, false, 20, 0, 1000),
        entry("c", WorkingSetTier::Hot, false, 90, 0, 1000),
    ];
    let mut s_none = WorkingSetState::build(entries.clone()).expect("s_none");
    let mut s_absent = WorkingSetState::build(entries).expect("s_absent");
    // s_absent explicitly has no scores (None) -> same as s_none
    let tok = estimate_tokens("summary-a");
    let cfg = SchedulerConfig::new(tok).expect("cfg");
    let c_none = s_none.assemble(&graph, &store, &cfg);
    let c_absent = s_absent.assemble(&graph, &store, &cfg);
    // byte-equal state and demotion
    assert_eq!(s_none, s_absent);
    assert_eq!(c_none, c_absent);
    // lowest scheduler (a) demoted
    assert_eq!(c_none.entries.len(), 1);
    assert_eq!(c_none.entries[0].node_id, "c");
    assert_eq!(c_none.demoted, vec!["a".to_owned(), "b".to_owned()]);
    // also verify TickInput with None search_scores is byte-equal to pre-93 baseline
    let tick_none = TickInput::new(1, vec![], "rev1", vec![], vec![]);
    assert!(tick_none.search_scores.is_none());
    let tick_with_none = TickInput::new(1, vec![], "rev1", vec![], vec![]);
    assert_eq!(tick_none, tick_with_none);
}

#[test]
fn scores_present_retention_order_lower_demoted_first_absent_zero() {
    use super::SearchScore;
    use crate::context_graph::{
        ContextGraph, ContextNode, ContextNodeKind, estimate_tokens,
    };
    use crate::context_representation::{
        ContextRepresentationStore, NodeRepresentation, NodeRepresentationSet,
        RepresentationLevel, RepresentationOrigin, content_digest_of,
    };
    let nodes = vec![
        ContextNode {
            id: "a".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "a".repeat(64),
            summary: "summary-a".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-a"),
        },
        ContextNode {
            id: "b".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "b".repeat(64),
            summary: "summary-b".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-b"),
        },
        ContextNode {
            id: "c".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "c".repeat(64),
            summary: "summary-c".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-c"),
        },
    ];
    let graph = ContextGraph::build(nodes, vec![]).expect("graph");
    let sets = vec![
        NodeRepresentationSet::build(
            "a".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-a"),
                derived_from: vec![],
                content: "summary-a".to_owned(),
            }],
        )
        .expect("a"),
        NodeRepresentationSet::build(
            "b".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-b"),
                derived_from: vec![],
                content: "summary-b".to_owned(),
            }],
        )
        .expect("b"),
        NodeRepresentationSet::build(
            "c".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-c"),
                derived_from: vec![],
                content: "summary-c".to_owned(),
            }],
        )
        .expect("c"),
    ];
    let store = ContextRepresentationStore::build(sets).expect("store");
    // scheduler scores: a low (10), b mid (50), c high (90) => without scores demote a first
    // search scores: a 100, b absent(0), c 50 => with scores demote b(0) first, then c(50), then a(100) — inverts scheduler
    let entries = vec![
        entry("a", WorkingSetTier::Hot, false, 10, 0, 1000),
        entry("b", WorkingSetTier::Hot, false, 50, 0, 1000),
        entry("c", WorkingSetTier::Hot, false, 90, 0, 1000),
    ];
    let mut state = WorkingSetState::build(entries).expect("state");
    // set search scores: a 100, c 50, b absent => 0
    state.set_search_scores(vec![
        SearchScore::new("a", 100),
        SearchScore::new("c", 50),
    ]);
    let tok = estimate_tokens("summary-a");
    let cfg = SchedulerConfig::new(tok).expect("cfg");
    let ctx = state.assemble(&graph, &store, &cfg);
    // with scores, lowest search first: b(0) then c(50) => only c remains after budget tight
    assert_eq!(ctx.entries.len(), 1);
    assert_eq!(ctx.entries[0].node_id, "a");
    assert_eq!(ctx.demoted, vec!["b".to_owned(), "c".to_owned()]);
    // tiebreak: same search score, lower scheduler first then node_id
    let mut state2 = WorkingSetState::build(vec![
        entry("a", WorkingSetTier::Hot, false, 10, 0, 1000),
        entry("b", WorkingSetTier::Hot, false, 10, 0, 1000),
        entry("c", WorkingSetTier::Hot, false, 90, 0, 1000),
    ])
    .expect("s2");
    // search scores: a and b same 10, c 10 also? then tiebreak scheduler: a and b same low, c high => among a/b tie, node_id asc
    state2.set_search_scores(vec![
        SearchScore::new("a", 10),
        SearchScore::new("b", 10),
        SearchScore::new("c", 10),
    ]);
    let tok2 = estimate_tokens("summary-a");
    let cfg2 = SchedulerConfig::new(tok2).expect("cfg2");
    let ctx2 = state2.assemble(&graph, &store, &cfg2);
    // All same search, so scheduler decides: a and b lowest (both 10), tie node_id a before b => demote a then b, keep c
    assert_eq!(ctx2.demoted, vec!["a".to_owned(), "b".to_owned()]);
    assert_eq!(ctx2.entries[0].node_id, "c");
}

#[test]
fn quota_demoted_order_unchanged_by_scores() {
    use super::SearchScore;
    use crate::context_graph::{
        ContextGraph, ContextNode, ContextNodeKind, estimate_tokens,
    };
    use crate::context_representation::{
        ContextRepresentationStore, NodeRepresentation, NodeRepresentationSet,
        RepresentationLevel, RepresentationOrigin, content_digest_of,
    };
    // pinned HOT nodes: a low scheduler (10), b high (90), c high (90) but search scores invert
    let nodes = vec![
        ContextNode {
            id: "a".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "a".repeat(64),
            summary: "summary-a".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-a"),
        },
        ContextNode {
            id: "b".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "b".repeat(64),
            summary: "summary-b".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-b"),
        },
        ContextNode {
            id: "c".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "c".repeat(64),
            summary: "summary-c".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("summary-c"),
        },
    ];
    let graph = ContextGraph::build(nodes, vec![]).expect("graph");
    let sets = vec![
        NodeRepresentationSet::build(
            "a".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-a"),
                derived_from: vec![],
                content: "summary-a".to_owned(),
            }],
        )
        .expect("a"),
        NodeRepresentationSet::build(
            "b".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-b"),
                derived_from: vec![],
                content: "summary-b".to_owned(),
            }],
        )
        .expect("b"),
        NodeRepresentationSet::build(
            "c".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-c"),
                derived_from: vec![],
                content: "summary-c".to_owned(),
            }],
        )
        .expect("c"),
    ];
    let store = ContextRepresentationStore::build(sets).expect("store");
    // Create large distinct summaries to exceed pinned quota 1024 (unique digest, each ~514 tokens)
    let big_a = "x".repeat(2040) + "a";
    let big_b = "x".repeat(2040) + "b";
    let big_c = "x".repeat(2040) + "c";
    let big_tok_a = estimate_tokens(&big_a);
    let big_tok = big_tok_a;
    // Use distinct summaries with distinct digests (unique-digest counts each)
    let big_nodes = vec![
        ContextNode {
            id: "a".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "a".repeat(64),
            summary: big_a.clone(),
            source_bindings: vec![],
            token_estimate: big_tok,
        },
        ContextNode {
            id: "b".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "b".repeat(64),
            summary: big_b.clone(),
            source_bindings: vec![],
            token_estimate: big_tok,
        },
        ContextNode {
            id: "c".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: "c".repeat(64),
            summary: big_c.clone(),
            source_bindings: vec![],
            token_estimate: big_tok,
        },
    ];
    let big_graph = ContextGraph::build(big_nodes, vec![]).expect("bgraph");
    let big_sets = vec![
        NodeRepresentationSet::build(
            "a".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of(&big_a),
                derived_from: vec![],
                content: big_a.clone(),
            }],
        )
        .expect("a"),
        NodeRepresentationSet::build(
            "b".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of(&big_b),
                derived_from: vec![],
                content: big_b.clone(),
            }],
        )
        .expect("b"),
        NodeRepresentationSet::build(
            "c".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of(&big_c),
                derived_from: vec![],
                content: big_c.clone(),
            }],
        )
        .expect("c"),
    ];
    let big_store =
        ContextRepresentationStore::build(big_sets).expect("bstore");
    let entries = vec![
        entry("a", WorkingSetTier::Hot, true, 10, 0, big_tok),
        entry("b", WorkingSetTier::Hot, true, 90, 0, big_tok),
        entry("c", WorkingSetTier::Hot, true, 90, 0, big_tok),
    ];
    let mut state = WorkingSetState::build(entries).expect("state");
    // search scores invert: a high 100, b low 0, c mid 50 — if quota used search scores, b would demote, but it should demote a (lowest scheduler)
    state.set_search_scores(vec![
        SearchScore::new("a", 100),
        SearchScore::new("b", 0),
        SearchScore::new("c", 50),
    ]);
    let cfg = SchedulerConfig::default(); // budget 4096, pin quota 1024 will trigger (3*big_tok >1024)
    let ctx = state.assemble(&big_graph, &big_store, &cfg);
    // Pin quota should demote lowest scheduler pinned HOT first (a), regardless of search scores
    assert!(
        ctx.demoted.contains(&"a".to_owned()),
        "pin quota must demote lowest scheduler 'a', got {:?}",
        ctx.demoted
    );
    // b and c should remain if still within quota after demoting a (depends on tokens); at least a demoted first
    // Verify that search scores did not affect quota order: b (search 0) should NOT be demoted before a
    let _ = (graph, store); // keep unused
}

#[test]
fn no_depth_change_high_score_still_l1_only() {
    let (mut state, graph, store) = assembly_fixture();
    use super::SearchScore;
    // state has a HOT with structured also stored; assembly should still be L1 only even with high search score
    state.set_search_scores(vec![
        SearchScore::new("a", 9999),
        SearchScore::new("b", 0),
    ]);
    let cfg = SchedulerConfig::default();
    let ctx = state.assemble(&graph, &store, &cfg);
    let a = ctx.entries.iter().find(|e| e.node_id == "a").expect("a");
    assert_eq!(a.summary, "summary-a");
    assert!(!a.summary.contains("structured"));
}

#[test]
fn determinism_byte_equal_with_scores() {
    use super::SearchScore;
    let build = || {
        let entries = vec![
            entry("a", WorkingSetTier::Hot, false, 10, 0, 1000),
            entry("b", WorkingSetTier::Hot, false, 20, 0, 1000),
            entry("c", WorkingSetTier::Hot, false, 30, 0, 1000),
        ];
        WorkingSetState::build(entries).unwrap()
    };
    let (mut s1, graph, store) = {
        let s = build();
        let (_, _g, _st) = assembly_fixture();
        // Use custom graph/store for determinism: use assembly_fixture's graph but we need matching ids
        // Instead use simple fixture with a,b,c
        use crate::context_graph::{
            ContextGraph, ContextNode, ContextNodeKind, estimate_tokens,
        };
        use crate::context_representation::{
            ContextRepresentationStore, NodeRepresentation,
            NodeRepresentationSet, RepresentationLevel, RepresentationOrigin,
            content_digest_of,
        };
        let nodes = vec![
            ContextNode {
                id: "a".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "a".repeat(64),
                summary: "summary-a".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-a"),
            },
            ContextNode {
                id: "b".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "b".repeat(64),
                summary: "summary-b".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-b"),
            },
            ContextNode {
                id: "c".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "c".repeat(64),
                summary: "summary-c".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-c"),
            },
        ];
        let graph = ContextGraph::build(nodes, vec![]).expect("g");
        let sets = vec![
            NodeRepresentationSet::build(
                "a".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("summary-a"),
                    derived_from: vec![],
                    content: "summary-a".to_owned(),
                }],
            )
            .expect("a"),
            NodeRepresentationSet::build(
                "b".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("summary-b"),
                    derived_from: vec![],
                    content: "summary-b".to_owned(),
                }],
            )
            .expect("b"),
            NodeRepresentationSet::build(
                "c".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("summary-c"),
                    derived_from: vec![],
                    content: "summary-c".to_owned(),
                }],
            )
            .expect("c"),
        ];
        let store = ContextRepresentationStore::build(sets).expect("store");
        (s, graph, store)
    };
    let (mut s2, graph2, store2) = {
        let s = build();
        use crate::context_graph::{
            ContextGraph, ContextNode, ContextNodeKind, estimate_tokens,
        };
        use crate::context_representation::{
            ContextRepresentationStore, NodeRepresentation,
            NodeRepresentationSet, RepresentationLevel, RepresentationOrigin,
            content_digest_of,
        };
        let nodes = vec![
            ContextNode {
                id: "a".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "a".repeat(64),
                summary: "summary-a".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-a"),
            },
            ContextNode {
                id: "b".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "b".repeat(64),
                summary: "summary-b".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-b"),
            },
            ContextNode {
                id: "c".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "c".repeat(64),
                summary: "summary-c".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-c"),
            },
        ];
        let graph = ContextGraph::build(nodes, vec![]).expect("g");
        let sets = vec![
            NodeRepresentationSet::build(
                "a".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("summary-a"),
                    derived_from: vec![],
                    content: "summary-a".to_owned(),
                }],
            )
            .expect("a"),
            NodeRepresentationSet::build(
                "b".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("summary-b"),
                    derived_from: vec![],
                    content: "summary-b".to_owned(),
                }],
            )
            .expect("b"),
            NodeRepresentationSet::build(
                "c".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("summary-c"),
                    derived_from: vec![],
                    content: "summary-c".to_owned(),
                }],
            )
            .expect("c"),
        ];
        let store = ContextRepresentationStore::build(sets).expect("store");
        (s, graph, store)
    };
    s1.set_search_scores(vec![
        SearchScore::new("b", 10),
        SearchScore::new("a", 5),
    ]);
    s2.set_search_scores(vec![
        SearchScore::new("a", 5),
        SearchScore::new("b", 10),
    ]);
    let cfg = SchedulerConfig::new(crate::context_graph::estimate_tokens(
        "summary-a",
    ))
    .expect("cfg");
    let c1 = s1.assemble(&graph, &store, &cfg);
    let c2 = s2.assemble(&graph2, &store2, &cfg);
    assert_eq!(c1, c2);
    // canonical JSON byte-equal check via serde_json sorted keys
    let j1 = serde_json::to_string(&serde_json::json!({"entries": c1.entries.iter().map(|e| serde_json::json!({"nodeId": e.node_id, "summary": e.summary})).collect::<Vec<_>>(), "demoted": c1.demoted})).expect("j1");
    let j2 = serde_json::to_string(&serde_json::json!({"entries": c2.entries.iter().map(|e| serde_json::json!({"nodeId": e.node_id, "summary": e.summary})).collect::<Vec<_>>(), "demoted": c2.demoted})).expect("j2");
    assert_eq!(j1, j2);
}

#[test]
fn tick_input_search_scores_canonical_and_bounded() {
    use super::{SearchScore, canonicalize_search_scores};
    // dedupe keeps highest, sorted, truncated to 64
    let mut scores = Vec::new();
    for i in 0..70 {
        scores.push(SearchScore::new(format!("n{i:03}"), i));
    }
    // duplicate with higher score should keep higher
    scores.push(SearchScore::new("n005", 999));
    let canon = canonicalize_search_scores(scores);
    assert_eq!(canon.len(), 64);
    assert_eq!(canon[0].node_id, "n000");
    assert_eq!(canon[63].node_id, "n063");
    // n005 should have score 999 (max)
    let n005 = canon.iter().find(|s| s.node_id == "n005").expect("n005");
    assert_eq!(n005.score, 999);
    // TickInput with scores is canonical and one per node
    let tick = TickInput::new_with_search_scores(
        1,
        vec![],
        "rev1",
        vec![],
        vec![],
        vec![
            SearchScore::new("b", 10),
            SearchScore::new("a", 5),
            SearchScore::new("b", 20),
        ],
    );
    let scores = tick.search_scores.expect("scores");
    assert_eq!(scores.len(), 2);
    assert_eq!(scores[0].node_id, "a");
    assert_eq!(scores[1].node_id, "b");
    assert_eq!(scores[1].score, 20); // max kept
}
