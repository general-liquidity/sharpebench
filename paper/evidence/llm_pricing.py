"""The rate card the LLM field is priced by, and the rule that selects it.

One table, one matching rule, one acceptance decision, imported by both files
that price a call: `examples/llm-agent/llm_agent.py`, which meters a running
policy and refuses a model it cannot price, and
`paper/evidence/assemble_llm_field.py`, which writes the `cost_usd` the paper
publishes. They each carried a literal copy of the table and a restatement of
the rule, recorded as a limit at the time: two tables and two copies of one
rule that a future edit could desynchronize, so a model could be priced by one
side and refused by the other, or priced differently by each. There is now one
of each, and `paper/src/test_llm_pricing.py` fails if either side grows a
second.

Nothing is imported here, deliberately. The reason the rule was restated rather
than shared was that importing the shim would pull the Anthropic SDK into an
assembler that reads only files; a module with no imports at all cannot carry
anything into either side. It lives under `paper/evidence/` because
`paper/src/provenance_common.py`'s `SOURCE_SCOPE` hashes `paper/evidence/*.py`
into the manifest and does not cover `examples/`, so the table that decides a
published number is inside the source snapshot rather than beside it.

The rates are USD per token, `(input, output)`, first-party API pricing. They
are unverified against a published price list; what is established here is
which card is selected and what happens when none is.
"""

# The separator and the width of a dated snapshot the provider expands a
# requested alias into. Every alias/pinned pair the SDK's own `Message.model`
# literal enumerates has this shape: claude-haiku-4-5-20251001,
# claude-opus-4-5-20251101, claude-sonnet-4-5-20250929, claude-opus-4-1-20250805.
SNAPSHOT_SEPARATOR = "-"
SNAPSHOT_DIGITS = 8

# First-party API pricing, USD per token (input, output).
PRICING = {
    "claude-fable-5": (10.00e-6, 50.00e-6),
    "claude-opus-5": (5.00e-6, 25.00e-6),
    "claude-haiku-4-5": (1.00e-6, 5.00e-6),
}


def is_dated_snapshot_of(requested, served):
    """Whether `served` is `requested` pinned to a dated snapshot of itself.

    The rule is taken from what the provider returns, not from what a served id
    happens to start with. An alias expands into the same alias followed by one
    hyphen and an eight-digit date, and that is the only remainder the API
    appends: `claude-haiku-4-5` -> `claude-haiku-4-5-20251001`,
    `claude-opus-4-5` -> `claude-opus-4-5-20251101`, `claude-sonnet-4-5` ->
    `claude-sonnet-4-5-20250929`, `claude-opus-4-1` -> `claude-opus-4-1-20250805`.
    Those four pairs are the alias/pinned pairs the installed SDK's own
    `Message.model` literal enumerates.

    So any other continuation is a different model, not a more precise name for
    the requested one: `-mini` is not a date, and neither is a truncated or
    padded one. The rule is deliberately narrower than the provider's whole
    namespace. Two deprecated aliases rebind rather than expand
    (`claude-sonnet-4-0` is served as `claude-sonnet-4-20250514`), and this
    refuses those; refusing a policy that is arguably the requested one costs a
    run, while accepting one that is not publishes the wrong identity.

    The shim decides model identity by this function and prices by it too, so
    narrowing the identity rule narrows the pricing match with it.
    """
    prefix = requested + SNAPSHOT_SEPARATOR
    if not served.startswith(prefix):
        return False
    snapshot = served[len(prefix):]
    return (
        len(snapshot) == SNAPSHOT_DIGITS and snapshot.isascii() and snapshot.isdigit()
    )


def lookup_price(model):
    """The rate card for `model`, or `None` when the table does not name it.

    The single acceptance decision. Matched exactly, or as a dated snapshot of a
    priced alias, which is the one expansion the provider makes. A prefix walk
    took any continuation, so a model whose name extends a priced one was billed
    at the other model's card: `claude-opus-5-1` would have been priced as
    `claude-opus-5`, and a table gaining a `claude-haiku-4` would price every
    `claude-haiku-4-5` at whichever key the walk reached first.

    `None` rather than a refusal, because the two callers refuse differently and
    both refusals are right where they are: the shim raises `UnpricedModel` and
    stops the run before the first observation is read, the assembler raises
    `SystemExit`, which is how every other incompleteness in that script
    refuses. What must not differ is which models are priced, at what rates, and
    which served ids count as one of them, and that is what lives here.
    """
    for alias, rate in PRICING.items():
        if model == alias or is_dated_snapshot_of(alias, model):
            return rate
    return None
