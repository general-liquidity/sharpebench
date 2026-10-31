//! An entrant's id reaches the signed header and `board.md`, so it is refused at
//! registration, before the window records it.

use sharpebench_arena::Arena;
use sharpebench_attest::{content_digest, make_commitment};
use sharpebench_core::ScoreConfig;

#[test]
fn registration_refuses_a_forged_row_and_records_nothing() {
    let dir = std::env::temp_dir().join(format!("sb-arena-agent-id-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut arena = Arena::init(&dir).unwrap();
    let deadline = arena.current_epoch() + 10;
    arena
        .open_window_with_fault_plan(
            "w1",
            deadline,
            deadline + 10,
            ScoreConfig::default(),
            None,
            content_digest(b"agent-id-scorer"),
            None,
        )
        .unwrap();
    let artifact = content_digest(b"agent-id-entrant");

    let forged = make_commitment("a\n1 forged 9.0", "w1", &artifact, "salt");
    let error = arena.register_entry("w1", forged).unwrap_err();
    assert!(error.contains("invalid agent id"), "{error}");
    assert!(arena.window("w1").unwrap().commitments.is_empty());

    let honest = make_commitment("openai/gpt-5:latest", "w1", &artifact, "salt");
    arena.register_entry("w1", honest).unwrap();
    assert_eq!(arena.window("w1").unwrap().commitments.len(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}
