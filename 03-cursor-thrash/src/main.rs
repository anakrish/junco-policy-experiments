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
const POOL: usize = 40;
// Common multiples keep history growth and cycle boundaries identical across arms.
const WARMUP_DECISIONS: usize = FIT * THRASH;
const DECISIONS: usize = 4 * WARMUP_DECISIONS;

fn test(n: &'static str) -> RegexExpr<E> {
    RegexExpr::test(TestExpr::atom(
        Comparison::Eq,
        AtomPattern::new(
            Component(2),
            ComponentPattern::constant(LockComponent::Resource(n)),
        ),
    ))
}

// The history must start with v0 and end with v1; the middle may be empty.
fn rule() -> TraceRule<E> {
    let tail = RegexExpr::concat([
        test("v0"),
        RegexExpr::all(),
        RegexExpr::star(RegexExpr::all()),
        test("v1"),
    ]);
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
        |e: &E| (format!("{} edit", e.principal), Vec::new()),
    )
}

fn median(v: &[u128]) -> u128 {
    let mut v = v.to_vec();
    v.sort_unstable();
    v[v.len() / 2]
}

fn run_arm(l: usize, d: usize, names: &[&'static str]) -> Vec<u128> {
    let store = Arc::new(Store::new().expect("store"));
    let mut h = History::new(Arc::clone(&store)).expect("history");
    for i in 0..l {
        h.push(LockEvent::new("alice", Action::Read, names[i % POOL]))
            .expect("push");
    }
    let mut p = TracePolicy::new(Arc::clone(&store), vec![rule()]).expect("compile");
    let mut costs = Vec::with_capacity(DECISIONS);
    for i in 0..WARMUP_DECISIONS + DECISIONS {
        h.push(LockEvent::new("alice", Action::Read, names[i % POOL]))
            .expect("push");
        let cand = LockEvent::new("bob", Action::Edit, names[i % d]);
        let start = (i >= WARMUP_DECISIONS).then(Instant::now);
        let result = p.decide(&h, &cand);
        let elapsed = start.map(|t| t.elapsed().as_nanos());
        // Candidate resources change binding keys, not the tail's language.
        assert_eq!(
            matches!(result.expect("decide"), PolicyDecision::Deny { .. }),
            names[i % POOL] == "v1"
        );
        if let Some(elapsed) = elapsed {
            costs.push(elapsed);
        }
    }
    costs
}

fn run(l: usize, names: &[&'static str]) {
    println!("L = {l} initial events, equivalent fresh append-only histories, one rule");
    println!(
        "  {:>6}  {:>11}  {:>11}  {:>11}  role",
        "values", "median ns", "first half", "last half"
    );
    let mut medians = Vec::new();
    for (d, label) in [(1usize, "baseline"), (FIT, "fits"), (THRASH, "thrash")] {
        let costs = run_arm(l, d, names);
        let m = median(&costs);
        println!(
            "  {:>6}  {:>11}  {:>11}  {:>11}  {label}",
            d,
            m,
            median(&costs[..DECISIONS / 2]),
            median(&costs[DECISIONS / 2..])
        );
        medians.push(m);
    }
    let base = medians[0].max(1);
    let fit = medians[1].max(1);
    let thrash = medians[2];
    println!("  thrash / fits      {:>7.0}x", thrash as f64 / fit as f64);
    println!("  thrash / baseline  {:>7.0}x", thrash as f64 / base as f64);
}

fn main() {
    println!(
        "cursor cache: {FIT} entries keyed by (history, candidate bindings), evicts its last entry"
    );
    println!("per-decision cost: {FIT} candidate values fit the cache, {THRASH} do not");
    println!(
        "per arm: {WARMUP_DECISIONS} unmeasured warmup decisions, {DECISIONS} measured decisions"
    );
    println!("all timings are ns/decision medians; halves contain complete candidate cycles\n");
    let names: Vec<&'static str> = (0..POOL)
        .map(|i| Box::leak(format!("v{i}").into_boxed_str()) as &'static str)
        .collect();
    match std::env::args().nth(1).and_then(|s| s.parse().ok()) {
        Some(l) => run(l, &names),
        None => {
            for l in [1_000usize, 5_000, 20_000, 100_000] {
                run(l, &names);
                println!();
            }
        }
    }
}
