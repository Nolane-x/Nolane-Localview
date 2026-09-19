use localview_network::{
    canonicalize_fault_plan, FaultMethod, FaultTransport, NetworkFaultEffect, NetworkFaultPlan,
    NetworkFaultPolicyError, NetworkFaultRule,
};

fn rule(
    id: &str,
    transport: FaultTransport,
    method: FaultMethod,
    path: &str,
    effect: NetworkFaultEffect,
    max_hits: u16,
) -> NetworkFaultRule {
    NetworkFaultRule {
        id: id.into(),
        transport,
        method,
        path: path.into(),
        effect,
        max_hits,
    }
}

fn plan(rules: Vec<NetworkFaultRule>, lease_ms: u64) -> NetworkFaultPlan {
    NetworkFaultPlan { rules, lease_ms }
}

#[test]
fn canonical_plan_accepts_bounded_loopback_relative_rules() {
    let canonical = canonicalize_fault_plan(&plan(
        vec![
            rule(
                "fail-projects",
                FaultTransport::Both,
                FaultMethod::Get,
                "/api/projects",
                NetworkFaultEffect::Fail,
                2,
            ),
            rule(
                "slow-save",
                FaultTransport::Fetch,
                FaultMethod::Post,
                "/api/save",
                NetworkFaultEffect::Delay { milliseconds: 450 },
                3,
            ),
            rule(
                "mock-auth",
                FaultTransport::Xhr,
                FaultMethod::Get,
                "/api/auth",
                NetworkFaultEffect::MockStatus { status: 503 },
                1,
            ),
        ],
        15_000,
    ))
    .expect("bounded plan should be valid");

    assert_eq!(canonical.lease_ms, 15_000);
    assert_eq!(canonical.rules.len(), 3);
    assert_eq!(canonical.rules[0].path, "/api/projects");
    assert_eq!(canonical.rules[1].path, "/api/save");
    assert_eq!(canonical.rules[2].path, "/api/auth");
    assert_eq!(canonical.fingerprint.len(), 16);
    assert!(canonical
        .fingerprint
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn fingerprint_is_deterministic_and_sensitive_to_authoritative_fields() {
    let base = plan(
        vec![rule(
            "slow",
            FaultTransport::Both,
            FaultMethod::Get,
            "/api/data",
            NetworkFaultEffect::Delay { milliseconds: 250 },
            4,
        )],
        10_000,
    );
    let same = base.clone();
    let mut changed = base.clone();
    changed.rules[0].max_hits = 5;

    let a = canonicalize_fault_plan(&base).unwrap();
    let b = canonicalize_fault_plan(&same).unwrap();
    let c = canonicalize_fault_plan(&changed).unwrap();

    assert_eq!(a.fingerprint, b.fingerprint);
    assert_ne!(a.fingerprint, c.fingerprint);
}

#[test]
fn empty_or_oversized_rule_sets_fail_closed() {
    assert_eq!(
        canonicalize_fault_plan(&plan(vec![], 1_000)).unwrap_err(),
        NetworkFaultPolicyError::EmptyRules
    );

    let rules = (0..17)
        .map(|index| {
            rule(
                &format!("r{index}"),
                FaultTransport::Fetch,
                FaultMethod::Get,
                &format!("/api/{index}"),
                NetworkFaultEffect::Fail,
                1,
            )
        })
        .collect();

    assert_eq!(
        canonicalize_fault_plan(&plan(rules, 1_000)).unwrap_err(),
        NetworkFaultPolicyError::TooManyRules { count: 17, max: 16 }
    );
}

#[test]
fn path_must_be_query_free_relative_and_bounded() {
    for path in [
        "api/data",
        "http://127.0.0.1:3000/api/data",
        "https://localhost/api/data",
        "//localhost/api/data",
        "/api/data?token=secret",
        "/api/data#private",
        "/api/\nsecret",
    ] {
        let error = canonicalize_fault_plan(&plan(
            vec![rule(
                "bad-path",
                FaultTransport::Fetch,
                FaultMethod::Get,
                path,
                NetworkFaultEffect::Fail,
                1,
            )],
            1_000,
        ))
        .unwrap_err();
        assert!(
            matches!(error, NetworkFaultPolicyError::InvalidPath { .. }),
            "{path:?} must fail as an invalid path, got {error:?}"
        );
    }

    let oversized = format!("/{}", "x".repeat(256));
    assert!(matches!(
        canonicalize_fault_plan(&plan(
            vec![rule(
                "oversized",
                FaultTransport::Fetch,
                FaultMethod::Get,
                &oversized,
                NetworkFaultEffect::Fail,
                1,
            )],
            1_000,
        )),
        Err(NetworkFaultPolicyError::InvalidPath { .. })
    ));
}

#[test]
fn selector_duplicates_are_rejected_even_when_effects_differ() {
    let error = canonicalize_fault_plan(&plan(
        vec![
            rule(
                "one",
                FaultTransport::Both,
                FaultMethod::Get,
                "/api/data",
                NetworkFaultEffect::Fail,
                1,
            ),
            rule(
                "two",
                FaultTransport::Both,
                FaultMethod::Get,
                "/api/data",
                NetworkFaultEffect::Delay { milliseconds: 10 },
                1,
            ),
        ],
        1_000,
    ))
    .unwrap_err();

    assert_eq!(
        error,
        NetworkFaultPolicyError::DuplicateSelector {
            transport: FaultTransport::Both,
            method: FaultMethod::Get,
            path: "/api/data".into(),
        }
    );
}

#[test]
fn lease_delay_status_and_hit_limits_are_enforced() {
    for lease_ms in [0, 99, 30_001] {
        assert!(matches!(
            canonicalize_fault_plan(&plan(
                vec![rule(
                    "lease",
                    FaultTransport::Fetch,
                    FaultMethod::Get,
                    "/api/data",
                    NetworkFaultEffect::Fail,
                    1,
                )],
                lease_ms,
            )),
            Err(NetworkFaultPolicyError::InvalidLease { .. })
        ));
    }

    for delay_ms in [0, 5_001] {
        assert!(matches!(
            canonicalize_fault_plan(&plan(
                vec![rule(
                    "delay",
                    FaultTransport::Fetch,
                    FaultMethod::Get,
                    "/api/data",
                    NetworkFaultEffect::Delay {
                        milliseconds: delay_ms,
                    },
                    1,
                )],
                1_000,
            )),
            Err(NetworkFaultPolicyError::InvalidDelay { .. })
        ));
    }

    for status in [199, 600] {
        assert!(matches!(
            canonicalize_fault_plan(&plan(
                vec![rule(
                    "status",
                    FaultTransport::Fetch,
                    FaultMethod::Get,
                    "/api/data",
                    NetworkFaultEffect::MockStatus { status },
                    1,
                )],
                1_000,
            )),
            Err(NetworkFaultPolicyError::InvalidStatus { .. })
        ));
    }

    for max_hits in [0, 65] {
        assert!(matches!(
            canonicalize_fault_plan(&plan(
                vec![rule(
                    "hits",
                    FaultTransport::Fetch,
                    FaultMethod::Get,
                    "/api/data",
                    NetworkFaultEffect::Fail,
                    max_hits,
                )],
                1_000,
            )),
            Err(NetworkFaultPolicyError::InvalidHitBudget { .. })
        ));
    }
}

#[test]
fn canonical_plan_never_contains_query_values() {
    let canonical = canonicalize_fault_plan(&plan(
        vec![rule(
            "safe",
            FaultTransport::Fetch,
            FaultMethod::Get,
            "/api/projects",
            NetworkFaultEffect::MockStatus { status: 429 },
            1,
        )],
        2_000,
    ))
    .unwrap();

    let serialized = serde_json::to_string(&canonical).unwrap();
    assert!(!serialized.contains("token="));
    assert!(!serialized.contains("password="));
    assert!(!serialized.contains("secret"));
}
