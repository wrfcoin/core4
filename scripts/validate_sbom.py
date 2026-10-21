#!/usr/bin/env python3
"""Fail-closed CycloneDX SBOM validator (#2317).

`sbom-generate.yml` accepted invalid inventories. The pinned `cyclonedx validate`
CLI returns 0 on a malformed document unless `--fail-on-errors` is passed, and the
workflow both omitted that flag *and* appended `|| echo "...warnings"`, so the
step could not fail. A workflow-shaped probe fed it two documents whose
`components` value was a JSON **string**: the CLI printed "Validation failed", the
summary counted the string's length as a component count, and the job exited 0.

This validator is the authoritative, fail-closed structural check the gate was
missing. It is deterministic and needs no network or external binary, so it can be
run locally and in CI and cannot pass vacuously. It exits **non-zero** on a
missing, empty, non-JSON, wrong-format, or empty/malformed inventory.

    python3 scripts/validate_sbom.py sbom/rust-sbom.cdx.json [more.cdx.json ...]
    python3 scripts/validate_sbom.py --min-components 1 <file>   # default min is 1
    python3 scripts/validate_sbom.py --self-test                 # hermetic probe

Exit 0 = every named SBOM is a well-formed, non-empty CycloneDX inventory.
Exit 1 = at least one SBOM is missing / empty / malformed — CI must go red.
Exit 2 = usage error.
"""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from pathlib import Path


def validate_doc(data: object, min_components: int) -> list[str]:
    """Structural errors in a parsed CycloneDX document (`[]` == valid).

    Deliberately strict about `components`: it must be a JSON **array**. The
    #2317 bypass was a document whose `components` was a string; `jq
    '.components | length'` then reported the string's character count as a
    component total. A string (or object, or number) here is a hard failure.
    """
    errors: list[str] = []
    if not isinstance(data, dict):
        return [f"top-level value is {type(data).__name__}, expected a JSON object"]

    fmt = data.get("bomFormat")
    if fmt != "CycloneDX":
        errors.append(f"bomFormat is {fmt!r}, expected 'CycloneDX'")

    if not isinstance(data.get("specVersion"), str) or not data.get("specVersion"):
        errors.append("specVersion missing, empty, or not a string")

    comps = data.get("components")
    if not isinstance(comps, list):
        # The exact malformed-inventory shape #2317 found green.
        errors.append(
            f"components is {type(comps).__name__}, expected a JSON array "
            "(a string/object here is the #2317 malformed-inventory bypass)"
        )
    else:
        if len(comps) < min_components:
            errors.append(
                f"{len(comps)} component(s), need >= {min_components} "
                "(an empty inventory is not a valid SBOM)"
            )
        for i, comp in enumerate(comps):
            if not isinstance(comp, dict):
                errors.append(f"components[{i}] is {type(comp).__name__}, expected an object")
            elif not (isinstance(comp.get("name"), str) and comp.get("name")):
                errors.append(f"components[{i}] has no non-empty 'name'")
    return errors


def validate_file(path: Path, min_components: int) -> list[str]:
    """Errors for one SBOM file, covering missing/empty/non-JSON up front."""
    if not path.exists():
        return [f"{path}: file does not exist"]
    raw = path.read_text(encoding="utf-8", errors="replace")
    if not raw.strip():
        return [f"{path}: file is empty"]
    try:
        data = json.loads(raw)
    except json.JSONDecodeError as exc:
        return [f"{path}: not valid JSON ({exc})"]
    return [f"{path}: {e}" for e in validate_doc(data, min_components)]


# --------------------------------------------------------------------------
# Hermetic self-test: prove a bad inventory is REJECTED and a good one passes.
# Reverting the fail-closed logic (e.g. dropping the `components` array check)
# turns the malformed-string probe below RED, which is the point.
# --------------------------------------------------------------------------
def self_test() -> int:
    good = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "components": [
            {"type": "library", "name": "serde", "version": "1.0.0"},
            {"type": "library", "name": "tokio", "version": "1.0.0"},
        ],
    }
    # Each bad fixture is a distinct real-world failure the old gate let through.
    bads: dict[str, object] = {
        "components-is-a-string (#2317 exact bypass)": {
            "bomFormat": "CycloneDX",
            "specVersion": "1.5",
            "components": "42",
        },
        "components-missing": {"bomFormat": "CycloneDX", "specVersion": "1.5"},
        "empty-components": {"bomFormat": "CycloneDX", "specVersion": "1.5", "components": []},
        "wrong-bomFormat": {"bomFormat": "SPDX", "specVersion": "1.5", "components": [{"name": "x"}]},
        "no-specVersion": {"bomFormat": "CycloneDX", "components": [{"name": "x"}]},
        "component-without-name": {
            "bomFormat": "CycloneDX",
            "specVersion": "1.5",
            "components": [{"type": "library", "version": "1.0"}],
        },
        "top-level-not-object": [1, 2, 3],
    }

    assert validate_doc(good, 1) == [], f"good SBOM rejected: {validate_doc(good, 1)}"
    for label, doc in bads.items():
        errs = validate_doc(doc, 1)
        assert errs, f"SELF-TEST FAILED: bad SBOM '{label}' was accepted — the gate is not fail-closed"

    # File-level probes on real files: missing, empty, non-JSON, and a good one.
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        missing = root / "missing.json"
        assert validate_file(missing, 1), "a missing file must be rejected"

        empty = root / "empty.json"
        empty.write_text("", encoding="utf-8")
        assert validate_file(empty, 1), "an empty file must be rejected"

        notjson = root / "notjson.json"
        notjson.write_text("this is not json {", encoding="utf-8")
        assert validate_file(notjson, 1), "a non-JSON file must be rejected"

        # A truncated document (the #2317 CLI-quirk shape) must also fail.
        strcomp = root / "strcomp.cdx.json"
        strcomp.write_text(json.dumps(bads["components-is-a-string (#2317 exact bypass)"]), encoding="utf-8")
        assert validate_file(strcomp, 1), "components-as-string must be rejected"

        ok = root / "good.cdx.json"
        ok.write_text(json.dumps(good), encoding="utf-8")
        assert validate_file(ok, 1) == [], "a well-formed SBOM must pass"

    print(f"validate_sbom.py self-test passed "
          f"({len(bads)} malformed shapes rejected, 3 bad files rejected, good passes)")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("files", nargs="*", help="CycloneDX SBOM JSON file(s) to validate")
    parser.add_argument("--min-components", type=int, default=1,
                        help="minimum component count for a non-empty inventory (default 1)")
    parser.add_argument("--self-test", action="store_true",
                        help="run the hermetic accept/reject probe and exit")
    args = parser.parse_args(argv[1:])

    if args.self_test:
        return self_test()

    if not args.files:
        parser.error("no SBOM files given (or use --self-test)")

    all_errors: list[str] = []
    for name in args.files:
        errs = validate_file(Path(name), args.min_components)
        if errs:
            all_errors.extend(errs)
        else:
            print(f"OK: {name} is a valid CycloneDX inventory")

    if all_errors:
        print("\nSBOM validation FAILED — refusing to accept an invalid inventory (#2317):",
              file=sys.stderr)
        for e in all_errors:
            print(f"  - {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
