# import-example

Synthetic demonstration input for `sharpebench import csv` (directory mode).
The two files are NOT market data and NOT from any external benchmark: they are
deterministic sine/cosine series generated for the docs, so the import workflow
in `docs/book/src/importing.md` is runnable end to end:

```
sharpebench import csv suites/import-example --out /tmp/imported.json --trials 5
sharpebench score /tmp/imported.json
```

Shape: one `<agent_id>.csv` per agent, one run per column, per-period simple
returns down the rows, header row optional.

No StockBench (arXiv:2510.02209) data appears here. StockBench publishes no
per-agent per-period return series (only summary tables), so there is nothing
real to freeze into a `suites/imported-stockbench/` field. See
`docs/book/src/importing.md` for what its repository does publish.
