use history_semantics::{PolicyDecision, TracePolicy, TraceRule};
use history_store::{cache::SUCCESSOR_CAPACITY, History, Store};
use history_syntax::{
    AtomPattern, Comparison, Component, ComponentPattern, Head, RegexExpr, Rule, RuleMode, TestExpr,
};
use junco_policy::{Action, LockComponent, LockEvent};
use std::sync::Arc;
use std::time::Instant;

type E = LockEvent<&'static str, &'static str>;

fn resource(name: &'static str) -> RegexExpr<E> {
    RegexExpr::test(TestExpr::atom(
        Comparison::Eq,
        AtomPattern::new(
            Component(2),
            ComponentPattern::constant(LockComponent::Resource(name)),
        ),
    ))
}

fn rule(names: &[&'static str], k: usize, missing: bool) -> TraceRule<E> {
    let repeated = RegexExpr::star(RegexExpr::union(names[..k].iter().map(|n| resource(n))));
    let tail = if missing {
        RegexExpr::concat([repeated, resource(names[k])])
    } else {
        repeated
    };
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

fn median(samples: &mut [u128]) -> u128 {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn decide_ns(
    policy: &mut TracePolicy<E>,
    history: &History<E>,
    candidate: &E,
    denied: bool,
) -> u128 {
    let t = Instant::now();
    let result = policy.decide(history, candidate);
    let elapsed = t.elapsed().as_nanos();
    assert_eq!(
        matches!(result.expect("decide"), PolicyDecision::Deny { .. }),
        denied
    );
    elapsed
}

fn run(
    store: &Arc<Store<E>>,
    names: &[&'static str],
    len: usize,
    reps: usize,
    arm: (usize, usize, bool, usize),
) {
    let (rules, k, missing, classes) = arm;
    assert!(
        rules > 0 && k > 0 && k < names.len(),
        "invalid rule count or width"
    );
    assert!(
        classes > 0 && classes <= k,
        "history classes must be within the union"
    );
    let source: Vec<_> = (0..rules).map(|_| rule(names, k, missing)).collect();
    let initial = source.clone();
    let t = Instant::now();
    let mut policy = TracePolicy::new(Arc::clone(store), initial).expect("compile");
    let compile_ms = t.elapsed().as_secs_f64() * 1e3;
    let history = History::from_events(
        Arc::clone(store),
        (0..len).map(|i| LockEvent::new("seed", Action::Read, names[i % classes])),
    )
    .expect("history");
    let candidate = LockEvent::new("candidate", Action::Edit, names[k]);

    // Exclude the first full-history pass from the cached measurements.
    decide_ns(&mut policy, &history, &candidate, !missing);
    let mut warm: Vec<_> = (0..reps)
        .map(|_| decide_ns(&mut policy, &history, &candidate, !missing))
        .collect();
    let mut fresh = Vec::with_capacity(reps);
    for _ in 0..reps {
        let mut policy = TracePolicy::new(Arc::clone(store), source.clone()).expect("compile");
        fresh.push(decide_ns(&mut policy, &history, &candidate, !missing));
    }
    let warm_ns = median(&mut warm);
    let fresh_ns = median(&mut fresh);
    println!(
        "{rules:>5} {k:>5} {classes:>7} {missing:>7} {compile_ms:>10.2} {warm_ns:>9} {:>12.2} {:>11.0}x",
        fresh_ns as f64 / 1000.0,
        fresh_ns as f64 / warm_ns.max(1) as f64
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let len: usize = args
        .get(1)
        .map(|s| s.parse().expect("invalid history length"))
        .unwrap_or(10_000);
    let reps: usize = args
        .get(2)
        .map(|s| s.parse().expect("invalid decision count"))
        .unwrap_or(3);
    assert!(
        len > 0 && reps > 0,
        "history length and decisions must be positive"
    );
    let arms: Vec<_> = if args.len() > 3 {
        assert_eq!(
            (args.len() - 3) % 4,
            0,
            "expected groups: rules k missing(0/1) classes"
        );
        args[3..]
            .chunks_exact(4)
            .map(|a| {
                (
                    a[0].parse().expect("invalid rule count"),
                    a[1].parse().expect("invalid union width"),
                    match a[2].as_str() {
                        "0" => false,
                        "1" => true,
                        _ => panic!("missing must be 0 or 1"),
                    },
                    a[3].parse().expect("invalid class count"),
                )
            })
            .collect()
    } else {
        vec![
            (1, 512, false, 128),
            (1, 512, false, 256),
            (1, 512, false, 257),
            (1, 512, false, 512),
            (4, 512, true, 512),
        ]
    };
    let names: Vec<&'static str> = (0..1025)
        .map(|i| Box::leak(format!("res{i}").into_boxed_str()) as &'static str)
        .collect();
    let store = Arc::new(Store::new().expect("store"));
    println!("history_len={len} decisions={reps} successor_capacity={SUCCESSOR_CAPACITY}");
    println!("rules     k classes missing compile_ms   warm_ns     fresh_us  fresh/warm");
    for arm in arms {
        run(&store, &names, len, reps, arm);
    }
}
