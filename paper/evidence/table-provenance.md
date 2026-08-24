# Per-cell provenance for the comparison tables (tab:related, tab:related-inverse)

Protocol: a mark means the cited paper (or its public board) states the property
of itself. A blank means only that this audit found no positive documentation;
it is not evidence that the system lacks the property. This file records the
source for every mark and distinguishes exhaustively searched blanks from
abstract-only checks. "abs" = the paper's arXiv abstract page; "body" = full
text searched; "unchecked" = no claim was found at abstract level and the body
was not exhaustively searched. Checked 2026-08-23; rows added in revision
checked 2026-08-24.

## tab:related (gate axes)

| Row | Deflates | pass^k | Process gate | Costs | Deterministic | Forward commit |
|---|---|---|---|---|---|---|
| FinBen | blank: no deflation claim (abs+body, Table 4 reports raw SR with CI) | blank | blank | blank: unchecked (body not searched for fees) | blank | blank |
| StockBench | blank: single-window evaluation, no deflation (abs) | blank | blank | blank: body searched for "transaction cost"/"commission"/"fee", zero hits | blank | blank |
| QuantBench | blank: names overfitting as open problem (abs) | blank | blank | MARK: body p6, "Other costs such as commissions and transaction fees are also considered." | blank | blank |
| InvestorBench | blank: return-based metrics, no deflation claim (abs) | blank | blank | blank: unchecked | blank | blank |
| FinRL-Meta | blank: environment library, no agent gating claimed (abs) | blank | blank | blank: unchecked (envs may charge costs; not claimed in abs) | blank | blank |
| Open FinLLM | blank: knowledge axes only (board) | blank | blank | n/a: no trading axis | blank | blank |
| tau-bench | blank | MARK: pass^k is its construct (abs) | blank | n/a | blank | blank |
| SharpeBench | this paper, Sec. 3 | Sec. 3.1 | Sec. 3.1 | App. C cost model | Sec. 4 | Sec. 4 |

## tab:related-inverse (rival axes)

| Row | LLM agents | Rich info | Single names | Live board | Post-cutoff run |
|---|---|---|---|---|---|
| FinBen | MARK: evaluates 21 LLMs (abs) | MARK: 42 datasets, 24 tasks, multimodal (abs) | blank: trading task is 10 stocks but board axis unchecked | blank | blank |
| StockBench | MARK (abs) | MARK: prices+news+fundamentals (abs) | MARK: single-name equities (abs) | MARK: public board | MARK: post-cutoff window (abs) |
| QuantBench | MARK: AI methods incl. LLM pipelines (abs) | blank | blank | blank | blank |
| InvestorBench | MARK: 13 LLM backbones (abs) | blank: memory/news environment not claimed at abs level | MARK: "single equities like stocks, cryptocurrencies and ETFs" (abs) | blank | blank |
| FinRL-Meta | blank: RL agents, not LLM (abs) | blank: rich data pipeline exists but axis is about agent observation; not claimed | blank: unchecked | blank: "community-wise competitions" (abs) is not a live board claim | blank |
| Open FinLLM | MARK (board) | MARK (board tasks) | blank | MARK: live HF space | blank |
| tau-bench | MARK (abs) | blank | n/a | blank | blank |
| SharpeBench | blank: the stated gap (Sec. 7) | blank | blank | blank | blank |

## Sources

- FinBen: arXiv 2402.12659 / NeurIPS 2024 D&B proceedings; trading results Table 4
  (mean +/- 95% CI across 10 stocks).
- StockBench: arXiv 2510.02209 (body PDF searched 2026-08-24).
- QuantBench: arXiv 2504.18600 (body PDF searched 2026-08-24).
- InvestorBench: arXiv 2412.18174 (abstract).
- FinRL-Meta: arXiv 2211.03107 / NeurIPS 2022 D&B (abstract).
- Open Financial LLM Leaderboard: public Hugging Face space.
- tau-bench: arXiv 2406.12045 (abstract).
