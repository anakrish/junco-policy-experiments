use history_semantics::{TracePolicy, TraceRule};
use history_store::Store;
use history_syntax::{
    AtomPattern, Comparison, Component, ComponentPattern, Head, RegexExpr, Rule, RuleMode, TestExpr,
};
use junco_policy::{Action, LockComponent, LockEvent};
use std::sync::Arc;
use std::time::Instant;

type E = LockEvent<&'static str, &'static str>;
const KMAX: usize = 2048;

fn test(name: &'static str) -> RegexExpr<E> {
    RegexExpr::test(TestExpr::atom(
        Comparison::Eq,
        AtomPattern::new(
            Component(2),
            ComponentPattern::constant(LockComponent::Resource(name)),
        ),
    ))
}

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

fn rss_mib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        .unwrap_or(0)
        / 1024
}

fn compile(
    store: &Arc<Store<E>>,
    rules_n: usize,
    k: usize,
    names: &[&'static str],
) -> Result<(u128, u64), String> {
    let rules: Vec<TraceRule<E>> = (0..rules_n).map(|_| rule(tail(names, k))).collect();
    let t = Instant::now();
    let out = TracePolicy::new(Arc::clone(store), rules);
    let ms = t.elapsed().as_millis();
    let rss = rss_mib();
    out.map(|_| (ms, rss)).map_err(|e| format!("{e:?}"))
}

fn main() {
    let store = Arc::new(Store::new().expect("store"));
    let names: Vec<&'static str> = (0..KMAX)
        .map(|i| Box::leak(format!("res{i}").into_boxed_str()) as &'static str)
        .collect();

    println!("rule: Forbid p,Edit,r   tail = INTERSECT_{{i<k}} STAR(resource == res_i)\n");
    println!("{:>6} {:>9} {}", "k", "compile", "outcome");
    let mut ok = 0usize;
    let mut k = 32usize;
    while k <= KMAX {
        match compile(&store, 1, k, &names) {
            Ok((ms, _)) => {
                println!("{:>6} {:>7}ms Ok", k, ms);
                ok = k;
            }
            Err(e) => {
                println!(
                    "{:>6} {:>9} {}",
                    k,
                    "-",
                    e.split_whitespace().next().unwrap_or("Err")
                );
                break;
            }
        }
        k = if k < 256 { k * 2 } else { k + 128 };
    }
    println!("\nlargest swept k that compiles: {ok}");
    if ok == 0 {
        return;
    }

    println!(
        "\n{:>6} {:>10} {:>9} {:>11} {:>10}",
        "rules", "compile", "ms/rule", "RSS before", "RSS after"
    );
    for n in [1usize, 20, 60, 150] {
        let before = rss_mib();
        match compile(&store, n, ok, &names) {
            Ok((ms, rss)) => println!(
                "{:>6} {:>7}ms {:>9.1} {:>9} MiB {:>8} MiB",
                n,
                ms,
                ms as f64 / n as f64,
                before,
                rss
            ),
            Err(e) => {
                println!("{:>6} refused: {e}", n);
                break;
            }
        }
    }
}
