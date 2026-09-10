//! A commitment for a faulted window binds the window's fault plan digest as
//! one more framed field, the way it binds the target window. A window without
//! a plan adds no field, so its commitment bytes are the ones `v2` always
//! produced.

use sharpebench_attest::registry::Registry;
use sharpebench_attest::{
    content_digest, make_commitment, make_commitment_under_fault_plan, verify_commitment,
    verify_commitment_under_fault_plan,
};

fn artifact() -> String {
    content_digest(b"artifact")
}

fn plan(tag: &str) -> String {
    content_digest(format!("fault-plan-{tag}").as_bytes())
}

#[test]
fn an_unfaulted_commitment_keeps_its_bytes() {
    // Hashes printed by `sharpebench commit` built from origin/main (0dcc4b8),
    // before the plan could be bound.
    let pinned = [
        (
            "alpha",
            "w1",
            "salt-a",
            "c2ea1c6a8a266debd60d1fd120b80b95fc464d3671cddd6429f68858198c0671",
        ),
        (
            "a|b",
            "",
            "s|t",
            "a3e6f9c6d3f8ab7120e6782f01284b63cf3ff052383026c038b0a0fc21c3809e",
        ),
        (
            "gamma",
            "window-003",
            "salt with spaces",
            "cf48fd948cce3112ff48918de4f96408994991be40e868622890f2503af425dd",
        ),
    ];
    for (agent, window, salt, hash) in pinned {
        let plain = make_commitment(agent, window, &artifact(), salt);
        assert_eq!(plain.commit_hash, hash, "{agent}/{window}");
        assert_eq!(
            make_commitment_under_fault_plan(agent, window, &artifact(), salt, None),
            plain
        );
        assert_eq!(
            serde_json::to_string(&plain).unwrap(),
            format!(
                "{{\"agent_id\":{},\"target_window\":{},\"commit_hash\":\"{hash}\"}}",
                serde_json::to_string(agent).unwrap(),
                serde_json::to_string(window).unwrap()
            ),
            "the commitment gained a field"
        );
    }
}

#[test]
fn a_commitment_verifies_only_under_the_plan_it_bound() {
    let a = plan("a");
    let b = plan("b");
    let faulted = make_commitment_under_fault_plan("alpha", "w1", &artifact(), "salt", Some(&a));
    let plain = make_commitment("alpha", "w1", &artifact(), "salt");
    assert_ne!(faulted.commit_hash, plain.commit_hash);

    let under = |c, p: Option<&str>| {
        verify_commitment_under_fault_plan(c, "alpha", "w1", &artifact(), "salt", p)
    };
    assert!(under(&faulted, Some(&a)));
    assert!(
        !under(&faulted, Some(&b)),
        "committed under a, revealed under b"
    );
    assert!(
        !under(&faulted, None),
        "committed under a, revealed under none"
    );
    assert!(!verify_commitment(
        &faulted,
        "alpha",
        "w1",
        &artifact(),
        "salt"
    ));
    assert!(under(&plain, None));
    assert!(
        !under(&plain, Some(&a)),
        "committed under none, revealed under a"
    );
}

#[test]
fn the_plan_cannot_be_folded_into_the_salt() {
    // Five framed fields never equal four, whatever the fourth contains.
    let a = plan("a");
    let faulted = make_commitment_under_fault_plan("alpha", "w1", &artifact(), "salt", Some(&a));
    for salt in [
        format!("salt{a}"),
        format!("salt|{a}"),
        format!("salt\0{a}"),
    ] {
        assert_ne!(
            make_commitment("alpha", "w1", &artifact(), &salt).commit_hash,
            faulted.commit_hash
        );
    }
}

#[test]
fn the_registry_refuses_a_reveal_under_another_plan() {
    let a = plan("a");
    let b = plan("b");
    let cases: [(&str, Option<&str>, Option<&str>, bool); 5] = [
        ("same", Some(&a), Some(&a), true),
        ("none", None, None, true),
        ("different", Some(&a), Some(&b), false),
        ("dropped", Some(&a), None, false),
        ("invented", None, Some(&a), false),
    ];
    for (tag, committed, revealed, accepted) in cases {
        let mut registry = Registry::new();
        registry
            .register(
                make_commitment_under_fault_plan("alpha", "w1", &artifact(), "salt", committed),
                10,
            )
            .unwrap();
        registry.set_epoch(10);
        let result = registry.reveal_under_fault_plan("alpha", "w1", &artifact(), "salt", revealed);
        assert_eq!(result.is_ok(), accepted, "{tag}: {result:?}");
        if !accepted {
            assert_eq!(
                result.unwrap_err(),
                "reveal does not match commitment",
                "{tag}"
            );
        }
    }
}
