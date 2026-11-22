use history_semantics::{PolicyDecision, TraceRule};
use history_store::{History, Store};
use history_syntax::{ComponentPattern, Head, RegexExpr, Rule, RuleMode};
use junco_policy::{FlowAction, FlowComponent, FlowEvent, RulePolicy, RuleSet};
use std::fs;
use std::sync::Arc;
use std::time::Instant;

type E = FlowEvent<&'static str, &'static str, &'static str>;

fn intern(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn always_rule() -> TraceRule<E> {
    TraceRule::new(
        Rule::new(
            RuleMode::Forbid,
            Head::new([
                ComponentPattern::variable("p"),
                ComponentPattern::variable("v"),
                ComponentPattern::constant(FlowComponent::Action(FlowAction::Contact)),
                ComponentPattern::constant(FlowComponent::Label("bad")),
            ]),
            RegexExpr::all(),
        ),
        |event: &E| (format!("{} contact", event.principal), Vec::new()),
    )
}

fn rss_kb() -> usize {
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args
        .get(1)
        .map(|s| s.parse().expect("batch length must be an integer"))
        .unwrap_or(1_000);
    let rule_counts: Vec<usize> = if args.len() > 2 {
        args[2..].iter().map(|s| s.parse().unwrap()).collect()
    } else {
        vec![1, 4, 8]
    };
    assert!(n > 0, "batch length must be positive");
    assert!(
        rule_counts.iter().all(|&r| r > 0),
        "rule counts must be positive"
    );

    let principals: Vec<&'static str> = (0..n).map(|i| intern(format!("user{i}"))).collect();
    let resource: &'static str = "val0";
    let diagnostics: Vec<String> = principals.iter().map(|p| format!("{p} contact")).collect();

    println!("batch_len={n}; audit mode, all decisions denied and appended");
    println!("rules    distinct   check_ms  RSS before KiB  RSS during KiB");

    for rules in rule_counts {
        for unique in [false, true] {
            let store = Arc::new(Store::<E>::new().expect("store"));
            let policy = (0..rules).fold(RulePolicy::new().audit(), |policy, _| {
                policy.rule(RuleSet::new(vec![always_rule()]))
            });
            let batch: Vec<E> = principals
                .iter()
                .map(|&p| FlowEvent {
                    principal: if unique { p } else { principals[0] },
                    handle: resource,
                    action: FlowAction::Contact,
                    label: "bad",
                })
                .collect();
            let mut history = History::new(store.clone()).expect("history");
            let before = rss_kb();
            let mut during = 0;
            let mut seen = 0;
            let t0 = Instant::now();
            policy
                .check(&mut history, &batch, |_, decision| {
                    match decision {
                        PolicyDecision::Deny {
                            failed_precondition,
                            allowed_fixes,
                        } => {
                            assert_eq!(
                                failed_precondition,
                                &diagnostics[if unique { seen } else { 0 }]
                            );
                            assert!(allowed_fixes.is_empty());
                        }
                        PolicyDecision::Grant => panic!("expected audit denial"),
                    }
                    seen += 1;
                    if seen == n {
                        // Judges are still alive inside the last observation callback.
                        during = rss_kb();
                    }
                })
                .expect("check");
            let ms = t0.elapsed().as_secs_f64() * 1e3;
            assert_eq!(seen, n);
            assert_eq!(history.len(), n);
            println!(
                "{rules:>5} {:>11} {ms:>10.2} {before:>15} {during:>15}",
                if unique { n } else { 1 }
            );
        }
    }
}
