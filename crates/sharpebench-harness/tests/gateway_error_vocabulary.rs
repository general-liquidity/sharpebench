//! The closed error vocabulary, held to the chapter that publishes it.
//!
//! `GatewayErrorKind::detail` is what an entrant reads and what evidence may
//! quote. Pinning each sentence here would restate them; the published table is
//! the record, so this compares the code with it. A sentence changed on one side
//! only fails, and the failure names the kind.

use std::collections::BTreeMap;

use sharpebench_harness::gateway::GatewayErrorKind::{self, *};

const CHAPTER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/book/src/model-gateway.md"
);

const KINDS: [GatewayErrorKind; 22] = [
    RequestTooLarge,
    MalformedRequest,
    ForbiddenField,
    UnknownProtocol,
    UnknownModelAlias,
    TooManyMessages,
    MessageTooLarge,
    TooManyTools,
    ToolPayloadTooLarge,
    OutputTokensOutOfRange,
    BudgetExhausted,
    CallLimitExhausted,
    ConcurrencyLimit,
    ShuttingDown,
    JournalUnwritable,
    JournalOwnershipLost,
    ProviderRateLimited,
    ProviderUnavailable,
    ProviderTimeout,
    ProviderResponseInvalid,
    DecisionRequestLimit,
    ResponseWithheld,
];

/// Each kind's position in [`KINDS`]. The match has no wildcard, so a variant
/// added to the enum stops this file compiling until it is listed.
fn position(kind: GatewayErrorKind) -> usize {
    match kind {
        RequestTooLarge => 0,
        MalformedRequest => 1,
        ForbiddenField => 2,
        UnknownProtocol => 3,
        UnknownModelAlias => 4,
        TooManyMessages => 5,
        MessageTooLarge => 6,
        TooManyTools => 7,
        ToolPayloadTooLarge => 8,
        OutputTokensOutOfRange => 9,
        BudgetExhausted => 10,
        CallLimitExhausted => 11,
        ConcurrencyLimit => 12,
        ShuttingDown => 13,
        JournalUnwritable => 14,
        JournalOwnershipLost => 15,
        ProviderRateLimited => 16,
        ProviderUnavailable => 17,
        ProviderTimeout => 18,
        ProviderResponseInvalid => 19,
        DecisionRequestLimit => 20,
        ResponseWithheld => 21,
    }
}

fn published() -> BTreeMap<String, String> {
    let chapter = std::fs::read_to_string(CHAPTER).expect("the gateway chapter");
    let mut rows = BTreeMap::new();
    for line in chapter
        .lines()
        .skip_while(|line| *line != "| Kind | Detail |")
        .skip(2)
        .take_while(|line| line.starts_with('|'))
    {
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        let [kind, detail] = cells[..] else {
            panic!("a row has two cells: {line}");
        };
        let previous = rows.insert(kind.trim_matches('`').to_string(), detail.to_string());
        assert_eq!(previous, None, "{kind} is published twice");
    }
    rows
}

#[test]
fn the_kind_list_here_is_every_kind_once() {
    for (index, kind) in KINDS.into_iter().enumerate() {
        assert_eq!(position(kind), index, "{kind:?}");
    }
}

#[test]
fn every_detail_sentence_is_the_one_the_chapter_publishes() {
    let code: BTreeMap<String, String> = KINDS
        .into_iter()
        .map(|kind| {
            let wire = serde_json::to_value(kind).expect("a kind serializes");
            let wire = wire.as_str().expect("a kind is a string").to_string();
            (wire, kind.detail().to_string())
        })
        .collect();
    assert_eq!(code, published());
}
