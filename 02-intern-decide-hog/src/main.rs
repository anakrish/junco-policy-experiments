use history_semantics::{PolicyDecision, TracePolicy, TraceRule};
use history_store::{History, Store};
use history_syntax::{ComponentPattern, Head, RegexExpr, Rule, RuleMode};
use junco_policy::{Action, LockComponent, LockEvent};
use std::fs;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

type E = LockEvent<[u64; 32], [u64; 32]>;

fn trivial_rule(denied: bool) -> TraceRule<E> {
    TraceRule::new(
        Rule::new(
            RuleMode::Forbid,
            Head::new([
                ComponentPattern::variable("p"),
                ComponentPattern::constant(LockComponent::Action(Action::Edit)),
                ComponentPattern::variable("r"),
            ]),
            if denied {
                RegexExpr::epsilon()
            } else {
                RegexExpr::empty()
            },
        ),
        |_: &E| ("forbid".to_string(), Vec::new()),
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

fn run(n: usize, rule_count: usize, keep_unique: bool, denied: bool) {
    assert!(n > 0 && rule_count > 0, "counts must be positive");
    let store = Arc::new(Store::<E>::new().expect("store"));
    let history = History::new(store.clone()).expect("history");
    let rules = (0..rule_count)
        .map(|_| trivial_rule(denied))
        .collect::<Vec<_>>();
    let mut policy = TracePolicy::new(store.clone(), rules).expect("policy");

    let before = rss_kib();
    let t0 = Instant::now();
    for i in 0..n {
        let mut id = [0u64; 32];
        if keep_unique {
            id[0] = i as u64;
        }
        let candidate = LockEvent::new(id, Action::Edit, id);
        assert_eq!(
            matches!(
                policy.decide(&history, &candidate).expect("decide"),
                PolicyDecision::Deny { .. }
            ),
            denied
        );
    }
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    assert_eq!(history.len(), 0);
    let after = rss_kib();
    drop(policy);
    drop(history);
    let store_live = rss_kib();
    println!(
        "{n:>9} {rule_count:>5} {:>7} {keep_unique:>6} {ms:>10.2} {:>10.2} {:>10.2} {:>14.2}",
        if denied { "deny" } else { "grant" },
        before as f64 / 1024.0,
        after as f64 / 1024.0,
        store_live as f64 / 1024.0
    );
    drop(store);
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("--case") {
        assert_eq!(
            raw.len(),
            5,
            "expected --case decisions rules unique denied"
        );
        run(
            raw[1].parse().expect("invalid decisions"),
            raw[2].parse().expect("invalid rules"),
            raw[3].parse().expect("invalid unique flag"),
            raw[4].parse().expect("invalid denied flag"),
        );
        return;
    }
    let args: Vec<usize> = raw
        .iter()
        .map(|s| s.parse().expect("invalid count"))
        .collect();
    let decisions = args.first().copied().unwrap_or(50_000);
    let rule_counts: Vec<usize> = if args.len() > 1 {
        args[1..].to_vec()
    } else {
        vec![1, 32]
    };
    println!("empty history; 256-byte principal and resource IDs; fresh process per row");
    println!("decisions rules verdict unique    loop_ms before_MiB  after_MiB store_live_MiB");
    let executable = std::env::current_exe().expect("current executable");
    for rule_count in rule_counts {
        for denied in [false, true] {
            for unique in [false, true] {
                let status = Command::new(&executable)
                    .arg("--case")
                    .arg(decisions.to_string())
                    .arg(rule_count.to_string())
                    .arg(unique.to_string())
                    .arg(denied.to_string())
                    .status()
                    .expect("run isolated case");
                assert!(status.success(), "isolated case failed: {status}");
            }
        }
    }
}
