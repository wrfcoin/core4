# Pre-dispatch truth check

Before assigning a lane, put every mechanically testable premise in a small JSON
manifest and run:

```text
python3 scripts/pre_dispatch_truth_check.py path/to/manifest.json
```

The checker fails closed for four waste patterns seen in the cut sprint:

- a required file or named symbol is absent from the tree;
- a branch-protection assumption is false;
- a linked PR is not merged as claimed; and
- a linked PR is merged while its issue is still open.

It cannot infer a premise from natural-language issue prose. The dispatcher
must declare the path, symbol, branch rule, and linked PRs it is relying on.
That explicit manifest is the reviewable contract.

```json
{
  "repo": "wrfcoin/core4",
  "checks": [{
    "label": "station-registration dispatch",
    "issue": 2537,
    "branch": "main",
    "branch_protection": false,
    "paths_exist": ["shared-protocol/src/blockchain/transaction.rs"],
    "symbols": [{
      "path": "shared-protocol/src/blockchain/transaction.rs",
      "regex": "StationRegistrationPayload"
    }],
    "linked_prs": []
  }]
}
```

For an offline review, pass `--facts facts.json`; production use reads the
declared GitHub facts through `gh`. Run `--self-test` to exercise its positive
case plus RED cases for missing path, missing symbol, absent/present branch-rule
disagreement, and a merged PR with a stale open issue.
