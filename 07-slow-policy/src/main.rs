use history_semantics::{PolicyDecision, TracePolicy, TraceRule};
use history_store::{cache::HISTORY_CACHE_CAPACITY, History, Store};
use history_syntax::{
    AtomPattern, Comparison, Component, ComponentPattern, Head, RegexExpr, Rule, RuleMode, TestExpr,
};
use junco_policy::{Action, LockComponent, LockEvent};
use std::sync::Arc;
use std::time::Instant;

type E = LockEvent<&'static str, &'static str>;
const FIT: usize = HISTORY_CACHE_CAPACITY;
const THRASH: usize = FIT + 1;
const WARMUP: usize = FIT * THRASH;
const DECISIONS: usize = WARMUP;

fn intern(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn resource(name: &'static str) -> RegexExpr<E> {
    RegexExpr::test(TestExpr::atom(
        Comparison::Eq,
        AtomPattern::new(
            Component(2),
            ComponentPattern::constant(LockComponent::Resource(name)),
        ),
    ))
}

fn rule(tail: RegexExpr<E>) -> TraceRule<E> {
    TraceRule::new(
        Rule::new(
            RuleMode::Forbid,
            Head::new([
                ComponentPattern::variable("p"),
                ComponentPattern::constant(LockComponent::Action(Action::Edit)),
                ComponentPattern::variable("r"),
            ]),
            tail,
        ),
        |event: &E| (format!("{} edit", event.principal), Vec::new()),
    )
}

// An alpha event occurs before the history's final beta event.
fn ordinary_rule(index: usize, pool: &[&'static str]) -> TraceRule<E> {
    rule(RegexExpr::concat([
        RegexExpr::all(),
        resource(pool[index % pool.len()]),
        RegexExpr::all(),
        resource(pool[(index + 100) % pool.len()]),
    ]))
}

fn median(v: &mut [u128]) -> u128 {
    v.sort_unstable();
    v[v.len() / 2]
}

fn main() {
    let l: usize = std::env::args()
        .nth(1)
        .map(|s| s.parse().expect("history length must be an integer"))
        .unwrap_or(20_000);
    let rule_counts: Vec<usize> = if std::env::args().count() > 2 {
        std::env::args()
            .skip(2)
            .map(|s| s.parse().unwrap())
            .collect()
    } else {
        vec![1, 4, 8, 16, 32]
    };
    assert!(l > 0, "history length must be positive");
    assert!(
        rule_counts.iter().all(|&r| r > 0),
        "rule counts must be positive"
    );

    let pool: Vec<&'static str> = (0..200).map(|i| intern(format!("q{i}"))).collect();
    let users: Vec<&'static str> = (0..200).map(|i| intern(format!("user{i}"))).collect();

    let store = Arc::new(Store::<E>::new().expect("store"));
    let mut history = History::new(store.clone()).expect("history");
    for i in 0..l {
        let action = match i % 3 {
            0 => Action::Read,
            1 => Action::Edit,
            _ => Action::Release,
        };
        history
            .push(LockEvent::new("seed", action, pool[i % pool.len()]))
            .expect("push");
    }

    println!("history_len={l}");
    println!("callers: 1 / {FIT} / {THRASH}; warmup={WARMUP}, measured={DECISIONS} per arm");
    println!("rules   hits_us   fits_us   thrash_us   thrash/fits  ns per rule-event");
    let run_t0 = Instant::now();
    let mut last_ratio = 0.0f64;
    let mut last_rules = 0usize;
    for &r in &rule_counts {
        let mut row = Vec::new();
        let denied = (0..r).any(|i| {
            pool[(l - 1) % pool.len()] == pool[(i + 100) % pool.len()] && i % pool.len() < l - 1
        });
        for width in [1usize, FIT, THRASH] {
            // Isolate cursor state between arms.
            let rules = (0..r).map(|i| ordinary_rule(i, &pool)).collect::<Vec<_>>();
            let mut policy = TracePolicy::new(store.clone(), rules).expect("policy");
            let mut times = Vec::new();
            for i in 0..WARMUP + DECISIONS {
                // Caller identity changes cursor keys, not the tail.
                let candidate = LockEvent::new(users[i % width], Action::Edit, pool[0]);
                let t0 = (i >= WARMUP).then(Instant::now);
                let result = policy.decide(&history, &candidate);
                let elapsed = t0.map(|t| t.elapsed().as_nanos());
                assert_eq!(
                    matches!(result.expect("decide"), PolicyDecision::Deny { .. }),
                    denied
                );
                if let Some(elapsed) = elapsed {
                    times.push(elapsed);
                }
            }
            let med_ns = median(&mut times);
            row.push(med_ns);
        }
        let (hits, fits, thrash) = (row[0], row[1], row[2]);
        let ratio = thrash as f64 / fits.max(1) as f64;
        last_ratio = ratio;
        last_rules = r;
        println!(
            "{:>5}  {:>8.1}  {:>8.1}  {:>10.1}  {:>10.0}x  {:>16.2}",
            r,
            hits as f64 / 1000.0,
            fits as f64 / 1000.0,
            thrash as f64 / 1000.0,
            ratio,
            thrash as f64 / (r as f64 * l as f64)
        );
    }
    println!(
        "thrash/fits = {:.0}x at {} rules over {} events   (wall {:.1} s)",
        last_ratio,
        last_rules,
        l,
        run_t0.elapsed().as_secs_f64()
    );
}
