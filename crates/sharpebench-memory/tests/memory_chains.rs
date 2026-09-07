use sharpebench_memory::{multi_session_report, SessionScores};

fn session(id: u64, lift: f64, dependencies: &[u64]) -> SessionScores {
    SessionScores::new(id, vec![0.0; 3], vec![lift; 3], dependencies.to_vec())
}

#[test]
fn failed_root_blocks_every_descendant_not_only_its_child() {
    let report = multi_session_report(
        &[
            session(3, 0.8, &[2]),
            session(1, -0.1, &[]),
            session(2, 0.6, &[1]),
        ],
        0.05,
    )
    .unwrap();
    // Merely checking the child would pass the original one-hop implementation.
    for id in [2, 3] {
        let row = report
            .per_session
            .iter()
            .find(|row| row.session_id == id)
            .unwrap();
        assert!(row.lift > 0.0);
        assert!(
            !row.dependencies_satisfied,
            "session {id} revived a broken chain"
        );
        assert_eq!(row.conditioned_lift, 0.0);
    }
    assert_eq!(report.dependency_satisfaction_rate, 0.0);
}

#[test]
fn cycles_between_distinct_sessions_are_refused() {
    for sessions in [
        vec![session(1, 0.5, &[2]), session(2, 0.5, &[1])],
        vec![
            session(1, 0.5, &[3]),
            session(2, 0.5, &[1]),
            session(3, 0.5, &[2]),
        ],
    ] {
        assert!(multi_session_report(&sessions, 0.05).is_err());
    }
}

#[test]
fn duplicate_edges_cannot_change_the_dependency_denominator() {
    assert!(multi_session_report(&[session(1, 0.5, &[]), session(2, 0.5, &[1, 1])], 0.05).is_err());
}

#[test]
fn significance_threshold_must_be_finite_and_strictly_between_zero_and_one() {
    for alpha in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.1,
        0.0,
        1.0,
        1.1,
    ] {
        assert!(
            multi_session_report(&[session(1, 0.5, &[])], alpha).is_err(),
            "alpha {alpha}"
        );
    }
    assert!(multi_session_report(&[session(1, 0.5, &[])], 0.05).is_ok());
}

#[test]
fn descriptive_scoring_does_not_need_two_tasks_to_invent_replication() {
    let report =
        multi_session_report(&[SessionScores::new(7, vec![1.0], vec![1.5], vec![])], 0.05).unwrap();
    assert_eq!(report.per_session[0].conditioned_lift, 0.5);
}

#[test]
fn successful_chain_and_zero_lift_boundary_remain_distinct() {
    let positive =
        multi_session_report(&[session(9, 0.5, &[7]), session(7, 0.1, &[])], 0.05).unwrap();
    assert!(positive.per_session[0].dependencies_satisfied);
    let zero = multi_session_report(&[session(9, 0.5, &[7]), session(7, 0.0, &[])], 0.05).unwrap();
    assert!(!zero.per_session[0].dependencies_satisfied);
}

#[test]
fn all_prerequisites_must_qualify_in_a_diamond() {
    let report = multi_session_report(
        &[
            session(4, 1.0, &[2, 3]),
            session(1, 1.0, &[]),
            session(2, 1.0, &[1]),
            session(3, -1.0, &[1]),
        ],
        0.05,
    )
    .unwrap();
    assert!(!report.per_session[0].qualified_retention);
    assert_eq!(report.per_session[0].conditioned_lift, 0.0);
    assert_eq!(report.dependency_satisfaction_rate, 0.75);
    assert_eq!(report.raw_mean_lift, 0.5);
    assert_eq!(report.conditioned_mean_lift, 0.25); // loss from session 3 stays counted
}

#[test]
fn long_reverse_order_chain_is_iterative_and_transitive() {
    let sessions: Vec<_> = (0..10_000)
        .rev()
        .map(|id| {
            session(
                id,
                if id == 0 { -1.0 } else { 1.0 },
                &if id == 0 { vec![] } else { vec![id - 1] },
            )
        })
        .collect();
    let report = multi_session_report(&sessions, 0.05).unwrap();
    assert_eq!(report.per_session.len(), 10_000);
    assert_eq!(report.per_session[0].session_id, 9999);
    assert!(report
        .per_session
        .iter()
        .all(|row| !row.qualified_retention));
    assert_eq!(report.conditioned_mean_lift, -1.0 / 10_000.0);
}

#[test]
fn nonfinite_inputs_and_unrepresentable_differences_or_reductions_are_refused() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for (baseline, retrieval) in [(bad, 0.0), (0.0, bad)] {
            assert!(multi_session_report(
                &[SessionScores::new(
                    1,
                    vec![baseline],
                    vec![retrieval],
                    vec![]
                )],
                0.05
            )
            .is_err());
        }
    }
    assert!(multi_session_report(
        &[SessionScores::new(
            1,
            vec![-f64::MAX],
            vec![f64::MAX],
            vec![]
        )],
        0.05
    )
    .is_err());
    assert!(multi_session_report(
        &[SessionScores::new(
            1,
            vec![0.0; 2],
            vec![f64::MAX; 2],
            vec![]
        )],
        0.05
    )
    .is_err());
    assert!(multi_session_report(
        &[
            SessionScores::new(1, vec![0.0], vec![f64::MAX], vec![]),
            SessionScores::new(2, vec![0.0], vec![f64::MAX], vec![]),
        ],
        0.05
    )
    .is_err());
}
