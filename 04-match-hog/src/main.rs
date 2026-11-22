use history_semantics::{PolicyDecision, TracePolicy, TraceRule};
use history_store::{History, Store};
use history_syntax::{
    AtomPattern, Comparison, Component, ComponentPattern, Head, RegexExpr, Rule, RuleMode, TestExpr,
};
use junco_policy::{Action, LockComponent, LockEvent};
use std::sync::Arc;
use std::time::Instant;

type E = LockEvent<&'static str, &'static str>;
const KMAX: usize = 1024;
const HISTORY: usize = 5_000;
const DECISIONS: usize = 10;

fn test(name: &'static str) -> RegexExpr<E> {
    RegexExpr::test(TestExpr::atom(
        Comparison::Eq,
        AtomPattern::new(
            Component(2),
            ComponentPattern::constant(LockComponent::Resource(name)),
        ),
    ))
}

// For k >= 2, these mutually exclusive resource predicates accept only empty history.
fn tail(names: &[&'static str], k: usize) -> RegexExpr<E> {
    RegexExpr::intersect(names[..k].iter().map(|n| RegexExpr::star(test(n))))
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
        |e: &E| (format!("{} edit", e.principal), Vec::new()),
    )
}

fn run(store: &Arc<Store<E>>, h: &History<E>, label: &str, tail: RegexExpr<E>) {
    let rules = vec![rule(tail)];
    let tc = Instant::now();
    let built = TracePolicy::new(Arc::clone(store), rules);
    let compile = tc.elapsed().as_secs_f64() * 1e3;
    let mut p = match built {
        Ok(p) => p,
        Err(e) => {
            println!("{label:>8} {compile:>10.2} refused: {e:?}");
            return;
        }
    };
    let cand = LockEvent::new("alice", Action::Edit, "doc");
    let mut grants = 0;
    let mut errors = 0;
    let mut first_error = None;
    let mut first_us = 0.0;
    let mut repeat_us = 0.0;
    for i in 0..DECISIONS {
        let td = Instant::now();
        let result = p.decide(h, &cand);
        let us = td.elapsed().as_secs_f64() * 1e6;
        if i == 0 {
            first_us = us;
        } else {
            repeat_us += us;
        }
        match result {
            Ok(decision) => {
                assert!(matches!(decision, PolicyDecision::Grant));
                grants += 1;
            }
            Err(e) => {
                errors += 1;
                let error = format!("{e:?}");
                if let Some(first) = &first_error {
                    assert_eq!(first, &error, "decision error changed between requests");
                } else {
                    first_error = Some(error);
                }
            }
        }
    }
    println!(
        "{label:>8} {compile:>10.2} {grants:>8} {errors:>8} {first_us:>11.2} {:>11.2}",
        repeat_us / (DECISIONS - 1) as f64
    );
    if let Some(error) = first_error {
        println!("         first error: {error}");
    }
}

fn main() {
    let store = Arc::new(Store::new().expect("store"));
    let names: Vec<&'static str> = (0..KMAX)
        .map(|i| Box::leak(format!("res{i}").into_boxed_str()) as &'static str)
        .collect();
    let mut h = History::new(Arc::clone(&store)).expect("history");
    for i in 0..HISTORY {
        let a = match i % 3 {
            0 => Action::Read,
            1 => Action::Edit,
            _ => Action::Release,
        };
        h.push(LockEvent::new("alice", a, "doc")).expect("push");
    }

    println!("rule: Forbid p,Edit,r   tail = INTERSECT_{{i<k}} STAR(resource == res_i)");
    println!("all tested tails are equivalent to epsilon (empty history only)");
    println!(
        "history: {HISTORY} events   decisions: {DECISIONS}, same matching candidate each time"
    );
    println!("expected verdict: Grant; first/repeat timings exclude outcome checks\n");
    println!(
        "{:>8} {:>10} {:>8} {:>8} {:>11} {:>11}",
        "tail/k", "compile ms", "grants", "errors", "first us", "repeat us"
    );
    run(&store, &h, "epsilon", RegexExpr::epsilon());
    for k in [256, 512, 1024] {
        run(&store, &h, &k.to_string(), tail(&names, k));
    }
}
