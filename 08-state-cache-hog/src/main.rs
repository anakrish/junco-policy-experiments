use history_semantics::{TracePolicy, TraceRule};
use history_store::Store;
use history_syntax::{
    AtomPattern, Comparison, Component, ComponentPattern, Head, RegexExpr, Rule, RuleMode, TestExpr,
};
use junco_policy::{Action, LockComponent, LockEvent};
use std::fs;
use std::sync::Arc;
use std::time::Instant;

type E = LockEvent<&'static str, &'static str>;

fn intern(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn class(name: &'static str) -> RegexExpr<E> {
    RegexExpr::test(TestExpr::atom(
        Comparison::Eq,
        AtomPattern::new(
            Component(2),
            ComponentPattern::constant(LockComponent::Resource(name)),
        ),
    ))
}

fn tail(names: &[&'static str], k: usize, repeats: usize) -> RegexExpr<E> {
    let wide = RegexExpr::union(names[..k].iter().copied().map(class));
    RegexExpr::concat((0..repeats).map(|_| RegexExpr::star(wide.clone())))
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

fn rss_kib() -> u64 {
    fs::read_to_string("/proc/self/status")
        .expect("read process RSS")
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .expect("VmRSS missing")
        .split_whitespace()
        .next()
        .expect("VmRSS value missing")
        .parse()
        .expect("invalid VmRSS")
}

fn run(store: &Arc<Store<E>>, names: &[&'static str], repeats: usize, n: usize) {
    let before = rss_kib();
    let t = Instant::now();
    let rules: Vec<_> = (0..n)
        .map(|_| rule(tail(names, names.len(), repeats)))
        .collect();
    let build_ms = t.elapsed().as_secs_f64() * 1e3;
    let input = rss_kib();
    let t = Instant::now();
    let policy = match TracePolicy::new(Arc::clone(store), rules) {
        Ok(policy) => policy,
        Err(error) => {
            println!(
                "{n:>6} {repeats:>8} refused after {:.2}ms: {error:?}",
                t.elapsed().as_secs_f64() * 1e3
            );
            return;
        }
    };
    let compile_ms = t.elapsed().as_secs_f64() * 1e3;
    let compiled = rss_kib();
    println!(
        "{n:>6} {repeats:>8} {build_ms:>10.2} {compile_ms:>10.2} {:>10.2} {:>10.2} {:>12.2}",
        before as f64 / 1024.0,
        input as f64 / 1024.0,
        compiled as f64 / 1024.0
    );
    drop(policy);
}

fn main() {
    let args: Vec<usize> = std::env::args()
        .skip(1)
        .map(|s| s.parse().unwrap())
        .collect();
    let k = args.first().copied().unwrap_or(2);
    let repeats = args.get(1).copied().unwrap_or(20_000);
    let rule_counts: Vec<usize> = if args.len() > 2 {
        args[2..].to_vec()
    } else {
        vec![1, 16]
    };
    assert!(
        k > 0 && repeats > 0 && rule_counts.iter().all(|&n| n > 0),
        "arguments must be positive"
    );

    let store = Arc::new(Store::<E>::new().expect("store"));
    let names: Vec<&'static str> = (0..k).map(|i| intern(format!("res{i}"))).collect();

    println!("atoms={k}; repeated stars are equivalent to one star");
    println!(" rules  repeats   build_ms compile_ms before_MiB  input_MiB compiled_MiB");
    run(&store, &names, 1, 1);
    for n in rule_counts {
        run(&store, &names, repeats, n);
    }
}
