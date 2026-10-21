#!/usr/bin/env python3
"""#2924: `skip_serializing_if` on ANY type reachable across a bincode boundary.

## Why this exists

`#[serde(skip_serializing_if = "Option::is_none")]` on a type that crosses a
**bincode** boundary is undecodable on `None` -- bincode is positional and
non-self-describing, so omitting a field on serialize leaves the derived
`Deserialize` short a byte it unconditionally expects to read.

This defect has recurred once with zero systemic guard between occurrences:
#1053 fixed the instance on `State`'s own top-level fields and added only an
*instance* guard -- a `debug_assert!` canary plus one `State::new()`
round-trip test, which uses an EMPTY `Vec` and therefore could never exercise
a `None`-bearing element added later. #2922 found `TreasuryTransactionRecord`
(nested inside `State` via `governance.treasury_transactions`, added AFTER
#1053, never re-swept) and `ForecastVerificationReceipt` (a P2P gossip type)
independently, by accident, while fixing something else.

An instance guard cannot fail for a type it does not name. This script is the
class guard: it discovers bincode-boundary ROOT types mechanically (not from
a hand-maintained list), walks their field graph transitively across the
whole workspace, and flags every `skip_serializing_if` it reaches.

## How "crosses a bincode boundary" is decided -- read before trusting the output

Static, whole-program type inference for arbitrary Rust is not attempted
here (that is a type-checker, not a script). Root-type discovery instead
uses a small set of MECHANICAL patterns, each independently resolvable
without semantic inference, empirically covering every real
`bincode::serialize`/`bincode::deserialize`/`Codec` call site in this
workspace at the time this was written:

  1. Turbofish:              `bincode::deserialize::<Type>(...)`
  2. `Codec` trait impls:    `type Request = Type;` / `type Response = Type;`
  3. Typed `let` binding:    `let x: Type = ...bincode::deserialize(...)...`
                              (searched across the enclosing statement, which
                              may span a multi-line `match`/`.map_err()` chain)
  4. `self`/`&self`:         `bincode::serialize(self)` inside `impl Type { .. }`
  5. Function tail position: a bare `bincode::deserialize(...)` as the last
                              expression of a function whose signature
                              declares `-> Result<Type, _>` or `-> Type`
                              (`Self` resolved via the enclosing `impl`)
  6. Named argument:         `bincode::serialize(&x)` / `bincode::serialize(x)`
                              where `x` is a bare identifier, resolved via the
                              nearest preceding `let x: Type = ...` / `let x =
                              Type { .. }` in the same function, or a function
                              parameter `x: Type` / `x: &Type`
  7. Inline struct literal:  `bincode::serialize(&Type { .. })`

Anything that does not match one of these is reported as UNRESOLVED rather
than silently dropped -- this is the conservative-over-approximation choice
required by #2924: narrowing root discovery to "whatever is easy" is exactly
how the original defect survived undetected between #1053 and #2922. A
human reviewing this script's output sees both the flagged violations AND
the sites it could not pin a type to, and can verify the latter by hand.

Once root types are found, the field-graph WALK is fully automatic and
requires no hand-maintained list: every struct/enum definition in the
workspace is indexed once, and reachability from a root is transitive
closure over field types (stripping `Vec<T>`, `Option<T>`, `HashMap<_, T>`,
`Box<T>`, references, and lifetimes to reach the inner type name). This is
what makes the guard catch a field added to an ALREADY-reachable type
without anyone updating a list -- the #1053-to-#2922 gap, closed.

## Known scope limits, stated rather than hidden

- Tuple-struct and tuple-enum-variant fields (positional, not named) are not
  modeled -- `#[serde(skip_serializing_if = ...)]` on a positional field is
  not idiomatic serde and was not observed anywhere in this workspace.
- A type name that collides across two unrelated definitions in different
  crates is treated as BOTH being reachable if either is -- a false-positive
  risk accepted deliberately (over-approximation over narrowing).
- This script does not resolve generic type parameters to concrete types; a
  field of a bare generic type parameter (e.g. `T` in `struct Foo<T> { x: T
  }`) cannot be walked further and is left as a leaf.

Reviewed suppressions (a real false positive, confirmed by hand) live in
`scripts/bincode_boundary_exceptions.json`, same shape as this codebase's
other ratchet/ceremony manifests: an entry names the exact type and field
and requires a non-empty reason.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
EXCEPTIONS_PATH = REPO_ROOT / "scripts" / "bincode_boundary_exceptions.json"

# Directories never scanned: build output, and non-Rust-source vendor trees.
EXCLUDED_DIR_PARTS = {"target", "node_modules", ".git"}

# `sdks/generated` is machine-generated OpenAPI/JSON-RPC client code, JSON-only
# by construction (see #2922's sweep) -- not part of the bincode domain and
# would only add noise (and different-language type-name collisions) to the
# index.
EXCLUDED_PATH_PREFIXES = ("sdks/generated/",)


def _iter_rust_files(root: Path) -> list[Path]:
    out = []
    for path in root.rglob("*.rs"):
        rel = path.relative_to(root)
        parts = set(rel.parts)
        if parts & EXCLUDED_DIR_PARTS:
            continue
        relstr = rel.as_posix()
        if any(relstr.startswith(p) for p in EXCLUDED_PATH_PREFIXES):
            continue
        out.append(path)
    return out


# ---------------------------------------------------------------------------
# Pass 1: index every struct/enum definition and every field's serde attrs.
# ---------------------------------------------------------------------------


@dataclass
class Field:
    name: str
    type_text: str
    serde_attrs: str  # raw text of every #[serde(...)] attribute directly above the field
    line: int


@dataclass
class TypeDef:
    name: str
    file: str
    line: int
    kind: str  # "struct" | "enum"
    fields: list[Field] = field(default_factory=list)


_STRUCT_RE = re.compile(r"^\s*pub(?:\([^)]*\))?\s+struct\s+(\w+)")
_ENUM_RE = re.compile(r"^\s*pub(?:\([^)]*\))?\s+enum\s+(\w+)")
_FIELD_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(\w+)\s*:\s*([^,{}]+?),?\s*$")
_ATTR_RE = re.compile(r"^\s*#\[([^\]]*)\]\s*$")


def _split_top_level_braces(lines: list[str], start_idx: int) -> tuple[int, int]:
    """Given the line index of a `struct X {` / `enum X {` opener (which may
    not itself contain the brace, e.g. a derive-heavy multi-line header),
    find the matching close. Returns (open_line_idx, close_line_idx)."""
    depth = 0
    open_idx = None
    for i in range(start_idx, len(lines)):
        for ch in lines[i]:
            if ch == "{":
                if depth == 0:
                    open_idx = i
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0 and open_idx is not None:
                    return open_idx, i
    return start_idx, len(lines) - 1


def _parse_fields(body_lines: list[str]) -> list[Field]:
    """Parse NAMED fields of a struct/enum-variant body, one field per
    top-level comma-separated entry, tracking any #[serde(...)] attributes
    immediately preceding each field."""
    fields: list[Field] = []
    pending_attrs: list[str] = []
    depth = 0
    buf = ""
    buf_start_line = 0
    for lineno, raw in enumerate(body_lines):
        stripped = raw.strip()
        if not stripped or stripped.startswith("//"):
            continue
        m_attr = _ATTR_RE.match(raw)
        if m_attr and depth == 0 and not buf.strip():
            if "serde" in m_attr.group(1):
                pending_attrs.append(m_attr.group(1))
            continue
        buf += (" " if buf else "") + stripped
        if not buf_start_line:
            buf_start_line = lineno
        depth += raw.count("<") - raw.count(">")
        depth += raw.count("(") - raw.count(")")
        # A field entry ends at a top-level comma, or at the end of input.
        if depth <= 0 and (stripped.endswith(",") or lineno == len(body_lines) - 1):
            entry = buf.rstrip(",").strip()
            buf = ""
            depth = 0
            if entry:
                fm = _FIELD_RE.match(entry + ",")
                if fm:
                    fields.append(
                        Field(
                            name=fm.group(1),
                            type_text=fm.group(2).strip(),
                            serde_attrs=" ".join(pending_attrs),
                            line=buf_start_line,
                        )
                    )
            pending_attrs = []
            buf_start_line = 0
    return fields


def build_type_index(files: list[Path], root: Path) -> dict[str, list[TypeDef]]:
    index: dict[str, list[TypeDef]] = {}
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        lines = text.splitlines()
        rel = path.relative_to(root).as_posix()
        i = 0
        while i < len(lines):
            m_struct = _STRUCT_RE.match(lines[i])
            m_enum = _ENUM_RE.match(lines[i])
            if m_struct or m_enum:
                name = (m_struct or m_enum).group(1)
                kind = "struct" if m_struct else "enum"
                # Header may continue ("pub struct Foo\n<'a> {") before the brace.
                header_end = i
                while "{" not in lines[header_end] and ";" not in lines[header_end]:
                    header_end += 1
                    if header_end >= len(lines):
                        break
                if header_end < len(lines) and lines[header_end].rstrip().endswith(";"):
                    # Unit struct / tuple struct on one line -- no named fields.
                    index.setdefault(name, []).append(
                        TypeDef(name=name, file=rel, line=i + 1, kind=kind, fields=[])
                    )
                    i = header_end + 1
                    continue
                open_idx, close_idx = _split_top_level_braces(lines, header_end)
                body = lines[open_idx + 1 : close_idx]
                if kind == "enum":
                    fields = _parse_enum_variant_fields(body)
                else:
                    fields = _parse_fields(body)
                index.setdefault(name, []).append(
                    TypeDef(name=name, file=rel, line=i + 1, kind=kind, fields=fields)
                )
                i = close_idx + 1
                continue
            i += 1
    return index


def _parse_enum_variant_fields(body_lines: list[str]) -> list[Field]:
    """Enum variants that are struct-like (`Variant { field: Type }`) get
    their named fields indexed exactly like a struct's. Unit and tuple
    variants contribute nothing (no named fields to carry the attribute)."""
    fields: list[Field] = []
    i = 0
    while i < len(body_lines):
        line = body_lines[i]
        if "{" in line and "}" not in line[: line.index("{") + 1]:
            open_idx, close_idx = _split_top_level_braces(body_lines, i)
            variant_body = body_lines[open_idx + 1 : close_idx]
            fields.extend(_parse_fields(variant_body))
            i = close_idx + 1
            continue
        i += 1
    return fields


# ---------------------------------------------------------------------------
# Pass 2: find bincode call sites, resolve each to a root type name.
# ---------------------------------------------------------------------------


@dataclass
class ResolvedRoot:
    type_name: str
    file: str
    line: int
    method: str  # which heuristic resolved it


@dataclass
class UnresolvedSite:
    file: str
    line: int
    text: str


_TURBOFISH_RE = re.compile(r"bincode::(?:de)?serialize::<([A-Za-z_][\w:]*)")
_CODEC_TYPE_RE = re.compile(r"^\s*type\s+(Request|Response)\s*=\s*([A-Za-z_][\w:]*)\s*;")
_IMPL_RE = re.compile(r"^\s*impl(?:<[^>]*>)?\s+(?:[\w:]+\s+for\s+)?([A-Za-z_]\w*)")
_FN_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)\s*\(")
_LET_TYPED_RE = re.compile(r"let\s+(?:mut\s+)?(\w+)\s*:\s*([^=]+?)\s*=")
_LET_STRUCT_LIT_RE = re.compile(r"let\s+(?:mut\s+)?(\w+)\s*=\s*([A-Za-z_]\w*)\s*\{")
_FN_RETURN_RE = re.compile(r"->\s*(?:Result<\s*([A-Za-z_][\w:]*)\s*,|([A-Za-z_][\w:]*))")
_BINCODE_CALL_RE = re.compile(r"bincode::(serialize|deserialize)\(")
_STRUCT_LIT_ARG_RE = re.compile(r"^\s*&?\s*([A-Za-z_]\w*)\s*\{")
_BARE_IDENT_ARG_RE = re.compile(r"^\s*&?\s*(self|[A-Za-z_]\w*)\s*(?:[,)]|$)")


def _split_top_level_generic_args(inner: str) -> list[str]:
    """Split `A<B<C, D>>, E` into `['A<B<C, D>>', 'E']` -- commas nested
    inside angle brackets do not split."""
    parts: list[str] = []
    depth = 0
    buf = ""
    for ch in inner:
        if ch == "<":
            depth += 1
        elif ch == ">":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append(buf.strip())
            buf = ""
        else:
            buf += ch
    if buf.strip():
        parts.append(buf.strip())
    return parts


# Wrapper generics from which we take the FIRST (only, or Ok-side) argument.
_FIRST_ARG_WRAPPERS = ("Option", "Vec", "Box", "Arc", "Rc", "RefCell", "Cow", "Result")
# Wrapper generics from which we take the SECOND argument (the value type in
# a keyed container).
_SECOND_ARG_WRAPPERS = ("HashMap", "FxHashMap", "BTreeMap")


def _strip_generic(type_text: str) -> str:
    """Reduce a field/param/`let`-annotation type to its innermost concrete
    name. `Option<Vec<HashMap<u64, Foo>>>` -> `Foo`. `&'a Foo` -> `Foo`.
    `Result<ForecastVerificationReceipt, _>` -> `ForecastVerificationReceipt`
    (the `Ok` side is what a `bincode::deserialize` call actually produces)."""
    t = type_text.strip()
    t = re.sub(r"^&(?:'[a-zA-Z_]\w*\s+)?(?:mut\s+)?", "", t)
    changed = True
    while changed:
        changed = False
        m = re.match(r"^([A-Za-z_]\w*)<(.+)>$", t)
        if not m:
            break
        name, inner = m.group(1), m.group(2)
        args = _split_top_level_generic_args(inner)
        if name in _FIRST_ARG_WRAPPERS and args:
            t = args[0]
            changed = True
        elif name in _SECOND_ARG_WRAPPERS and len(args) >= 2:
            t = args[1]
            changed = True
    t = t.strip().rstrip(",")
    t = t.split("::")[-1]
    t = re.split(r"[<\s]", t, maxsplit=1)[0]
    return t


def _all_type_names(type_text: str) -> list[str]:
    """Every distinct type-name-looking token in a (possibly compound, e.g.
    `HashMap<String, Vec<Foo>>`) type annotation -- used when walking a
    field's type for reachability, where every generic argument matters,
    not just the innermost one `_strip_generic` returns for a `let` LHS."""
    return sorted(set(re.findall(r"[A-Za-z_]\w*", type_text)) - _RUST_KEYWORDS)


_RUST_KEYWORDS = {
    "Vec", "Option", "Box", "Arc", "Rc", "RefCell", "Cow", "HashMap", "FxHashMap",
    "BTreeMap", "HashSet", "BTreeSet", "FxHashSet", "String", "str", "bool", "char",
    "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128",
    "isize", "f32", "f64", "Result", "Ok", "Err", "Some", "None", "Self", "dyn",
    "impl", "for", "mut", "ref",
}


def find_roots(files: list[Path], root: Path) -> tuple[list[ResolvedRoot], list[UnresolvedSite]]:
    resolved: list[ResolvedRoot] = []
    unresolved: list[UnresolvedSite] = []
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        lines = text.splitlines()
        rel = path.relative_to(root).as_posix()

        # Codec associated types -- no inference needed.
        for i, line in enumerate(lines):
            m = _CODEC_TYPE_RE.match(line)
            if m:
                resolved.append(ResolvedRoot(m.group(2).split("::")[-1], rel, i + 1, "codec-assoc-type"))

        # Track the innermost enclosing `impl TYPE` and `fn NAME(...) -> RET`
        # for each line, computed once per file via a brace-depth walk, so
        # `self`, function-tail, and parameter resolution can look them up
        # cheaply. `signature_text` is the fn's FULL header text (which may
        # span many lines when each parameter is on its own line), so a
        # parameter can be found regardless of where `fn` itself sits.
        impl_stack: list[tuple[int, str]] = []  # (brace_depth_at_open, type_name)
        fn_stack: list[tuple[int, str, str | None, str]] = []  # (depth_at_open, name, return_type, signature_text)
        depth = 0
        impl_at_line: list[str | None] = [None] * len(lines)
        fn_return_at_line: list[str | None] = [None] * len(lines)
        fn_signature_at_line: list[str] = [""] * len(lines)
        for i, line in enumerate(lines):
            m_impl = _IMPL_RE.match(line)
            if m_impl and "{" in line:
                impl_stack.append((depth, m_impl.group(1)))
            m_fn = _FN_RE.match(line)
            if m_fn:
                header = line
                j = i
                while "{" not in header and ";" not in header and j + 1 < len(lines):
                    j += 1
                    header += " " + lines[j]
                rm = _FN_RETURN_RE.search(header)
                ret = None
                if rm:
                    ret = rm.group(1) or rm.group(2)
                    if ret == "Self" and impl_stack:
                        ret = impl_stack[-1][1]
                if "{" in header:
                    fn_stack.append((depth, m_fn.group(1), ret, header))
            impl_at_line[i] = impl_stack[-1][1] if impl_stack else None
            fn_return_at_line[i] = fn_stack[-1][2] if fn_stack else None
            fn_signature_at_line[i] = fn_stack[-1][3] if fn_stack else ""
            depth += line.count("{") - line.count("}")
            while impl_stack and depth <= impl_stack[-1][0]:
                impl_stack.pop()
            while fn_stack and depth <= fn_stack[-1][0]:
                fn_stack.pop()

        for i, line in enumerate(lines):
            if line.lstrip().startswith("//"):
                continue  # a call site mentioned in a comment is not a call site
            for m in _BINCODE_CALL_RE.finditer(line):
                if _TURBOFISH_RE.search(line):
                    continue  # already resolved above via the turbofish branch's own scan
                call_start = m.end()
                arg_text = line[call_start:]
                # Pull a bounded forward window if the call's argument list
                # is still open at end of line (multi-line call).
                window_lines = [line]
                j = i
                depth2 = arg_text.count("(") - arg_text.count(")") + 1
                while depth2 > 0 and j + 1 < len(lines) and j - i < 8:
                    j += 1
                    window_lines.append(lines[j])
                    depth2 += lines[j].count("(") - lines[j].count(")")
                arg_window = " ".join(window_lines)
                arg_start_local = arg_window.find("bincode::")
                open_paren = arg_window.find("(", arg_start_local)
                # Bound arg_str to THIS call's own matching close paren --
                # otherwise trailing code on the same line (e.g. a `let ... =
                # bincode::serialize(x) else { ... }` guard) gets scanned as
                # if it were part of the argument list.
                depth3 = 0
                close_paren = len(arg_window)
                for k in range(open_paren, len(arg_window)):
                    if arg_window[k] == "(":
                        depth3 += 1
                    elif arg_window[k] == ")":
                        depth3 -= 1
                        if depth3 == 0:
                            close_paren = k
                            break
                arg_str = arg_window[open_paren + 1 : close_paren]

                site_text = line.strip()
                loc = (rel, i + 1)

                # Heuristic 3: typed `let` on this line or immediately before
                # a `match bincode::deserialize(...) {` block that this line
                # opens (scan a small backward window for the nearest typed
                # `let` whose RHS reaches this line without an intervening
                # unmatched close-brace).
                let_m = _LET_TYPED_RE.search(line)
                resolved_type = None
                method = None
                if let_m and "bincode::" in line[let_m.end() :]:
                    resolved_type = _strip_generic(let_m.group(2))
                    method = "same-line-let"
                if resolved_type is None:
                    back_window = lines[max(0, i - 6) : i]
                    depth3 = 0
                    for back_line in reversed(back_window):
                        depth3 += back_line.count("}") - back_line.count("{")
                        if depth3 > 0:
                            break
                        lm = _LET_TYPED_RE.search(back_line)
                        if lm:
                            resolved_type = _strip_generic(lm.group(2))
                            method = "enclosing-let"
                            break

                # Heuristic 4: self/&self inside impl TYPE.
                if resolved_type is None:
                    sm = _STRUCT_LIT_ARG_RE.search(arg_str) or _BARE_IDENT_ARG_RE.search(arg_str)
                    ident = sm.group(1) if sm else None
                    if ident == "self" and impl_at_line[i]:
                        resolved_type = impl_at_line[i]
                        method = "self-in-impl"

                # Heuristic 7: inline struct literal argument.
                if resolved_type is None:
                    sm = _STRUCT_LIT_ARG_RE.search(arg_str)
                    if sm and sm.group(1) not in ("self",):
                        resolved_type = sm.group(1)
                        method = "inline-struct-literal"

                # Heuristic 6: bare identifier argument, resolved via a
                # preceding typed `let` / struct-literal `let` in the same
                # function (bounded backward window), or a parameter in the
                # enclosing function's OWN signature (search the whole
                # signature text, not just the current line, since Rust
                # commonly puts one parameter per line).
                if resolved_type is None:
                    idm = _BARE_IDENT_ARG_RE.search(arg_str)
                    if idm:
                        ident = idm.group(1)
                        for back_line in reversed(lines[max(0, i - 60) : i]):
                            lm = re.search(rf"\blet\s+(?:mut\s+)?{re.escape(ident)}\s*:\s*([^=]+?)\s*=", back_line)
                            if lm:
                                resolved_type = _strip_generic(lm.group(1))
                                method = "backward-let"
                                break
                            slm = re.search(rf"\blet\s+(?:mut\s+)?{re.escape(ident)}\s*=\s*([A-Za-z_]\w*)\s*\{{", back_line)
                            if slm:
                                resolved_type = slm.group(1)
                                method = "backward-struct-literal"
                                break
                            # `let VAR = Type::associated_fn(...)` -- an
                            # associated-constructor call, the other common
                            # shape besides a bare struct literal. Rust type
                            # names are UpperCamelCase by convention while
                            # module/crate paths (`bincode::`, `std::`) are
                            # snake_case, so requiring an uppercase first
                            # letter rejects `let bytes = bincode::serialize(...)`
                            # matching "bincode" as if it were the type.
                            cm = re.search(rf"\blet\s+(?:mut\s+)?{re.escape(ident)}\s*=\s*([A-Z]\w*)::", back_line)
                            if cm:
                                resolved_type = cm.group(1)
                                method = "backward-associated-fn"
                                break
                        if resolved_type is None and fn_signature_at_line[i]:
                            pm = re.search(
                                rf"[(,]\s*{re.escape(ident)}\s*:\s*&?(?:mut\s+)?(?:'[a-zA-Z_]\w*\s+)?([A-Za-z_][\w:<>]*)",
                                fn_signature_at_line[i],
                            )
                            if pm:
                                resolved_type = _strip_generic(pm.group(1))
                                method = "fn-param-signature"

                # Heuristic 5: function-tail position, no local let at all --
                # only reached if this bincode call is not itself inside a
                # nested `let` context already handled above. DESERIALIZE
                # only: serialize's return is always bytes, never the
                # payload type, so the function's return type says nothing
                # about what was serialized.
                if resolved_type is None and fn_return_at_line[i] and m.group(1) == "deserialize":
                    # Only trust this when the call is (close to) the sole
                    # content of the line, i.e. genuinely the tail expression
                    # rather than a nested sub-call inside a bigger statement.
                    if re.match(r"^\s*bincode::deserialize\(", line) and line.rstrip().endswith((")", ";")):
                        resolved_type = fn_return_at_line[i]
                        method = "fn-tail-return-type"

                if resolved_type:
                    resolved.append(ResolvedRoot(resolved_type, rel, i + 1, method))
                else:
                    unresolved.append(UnresolvedSite(rel, i + 1, site_text))

        for i, line in enumerate(lines):
            tm = _TURBOFISH_RE.search(line)
            if tm:
                resolved.append(ResolvedRoot(tm.group(1).split("::")[-1], rel, i + 1, "turbofish"))

    return resolved, unresolved


# ---------------------------------------------------------------------------
# Pass 3: transitive field-graph walk from every resolved root type.
# ---------------------------------------------------------------------------


@dataclass
class Violation:
    type_name: str
    field_name: str
    file: str
    line: int
    reached_via: list[str]  # root -> ... -> this type, for a readable trail


def walk_and_flag(
    index: dict[str, list[TypeDef]], roots: set[str]
) -> tuple[list[Violation], set[str]]:
    violations: list[Violation] = []
    visited: set[str] = set()
    queue: list[tuple[str, list[str]]] = [(r, [r]) for r in roots]
    while queue:
        name, trail = queue.pop()
        if name in visited:
            continue
        visited.add(name)
        for typedef in index.get(name, []):
            for f in typedef.fields:
                # A bare `#[serde(skip)]` field (distinct from
                # `skip_serializing_if`) is symmetric: bincode writes
                # nothing for it on encode AND reads nothing for it on
                # decode, in both directions, always. There is no
                # positional-byte hazard through a field the wire format
                # never touches at all, so it must not propagate
                # reachability -- confirmed against a real case:
                # `Transaction.parsed_reveal: OnceLock<Option<RevealPayload>>`
                # is `#[serde(skip)]`'d (a lazy in-memory parse cache, #1378),
                # and RevealPayload's own `skip_serializing_if` fields are
                # genuinely safe JSON-only code reached only via
                # `parse_reveal_payload`'s `serde_json::from_str` -- never
                # bincode. Following this field would flag 15+ fields that
                # are not the defect this guard exists to catch.
                if re.search(r"\bskip\b", f.serde_attrs):
                    continue
                if "skip_serializing_if" in f.serde_attrs:
                    violations.append(
                        Violation(
                            type_name=name,
                            field_name=f.name,
                            file=typedef.file,
                            line=typedef.line + f.line,
                            reached_via=trail,
                        )
                    )
                for nested in _all_type_names(f.type_text):
                    if nested in index and nested not in visited:
                        queue.append((nested, trail + [nested]))
    return violations, visited


# ---------------------------------------------------------------------------
# Exceptions manifest.
# ---------------------------------------------------------------------------


def load_exceptions(path: Path) -> set[tuple[str, str]]:
    if not path.is_file():
        return set()
    data = json.loads(path.read_text(encoding="utf-8"))
    out = set()
    for entry in data.get("exceptions", []):
        type_name = entry.get("type")
        field_name = entry.get("field")
        reason = entry.get("reason", "").strip()
        if not type_name or not field_name or not reason:
            raise ValueError(f"malformed exception entry (needs type, field, non-empty reason): {entry}")
        out.add((type_name, field_name))
    return out


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def run(root: Path) -> tuple[list[Violation], list[UnresolvedSite], set[str], int]:
    files = _iter_rust_files(root)
    index = build_type_index(files, root)
    resolved_roots, unresolved = find_roots(files, root)
    root_names = {r.type_name for r in resolved_roots}
    violations, visited = walk_and_flag(index, root_names)
    exceptions = load_exceptions(root / "scripts" / "bincode_boundary_exceptions.json")
    real_violations = [v for v in violations if (v.type_name, v.field_name) not in exceptions]
    return real_violations, unresolved, root_names, len(visited)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", default=str(REPO_ROOT))
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        print("check_bincode_boundary_attrs.py self-test passed")
        return 0

    root = Path(args.root).resolve()
    violations, unresolved, root_names, reached_count = run(root)

    print(f"BINCODE_ROOT_TYPES resolved={len(root_names)}: {', '.join(sorted(root_names))}")
    print(f"BINCODE_REACHABLE_TYPES count={reached_count}")
    print(f"BINCODE_UNRESOLVED_CALL_SITES count={len(unresolved)}")
    for site in unresolved:
        print(f"  UNRESOLVED {site.file}:{site.line}: {site.text}")

    if not violations:
        print("BINCODE_BOUNDARY_GUARD_OK: no skip_serializing_if reachable from a bincode boundary")
        return 0

    print(f"BINCODE_BOUNDARY_VIOLATION count={len(violations)}")
    for v in violations:
        trail = " -> ".join(v.reached_via)
        print(
            f"::error file={v.file},line={v.line}::"
            f"{v.type_name}.{v.field_name} carries skip_serializing_if and is reachable "
            f"across a bincode boundary ({trail}). bincode is positional and "
            f"non-self-describing: omitting this field on serialize leaves the "
            f"derived Deserialize short a byte it unconditionally expects. "
            f"Drop skip_serializing_if, or add a reviewed entry to "
            f"scripts/bincode_boundary_exceptions.json with a reason."
        )
    return 1


def self_test() -> None:
    from tempfile import TemporaryDirectory

    with TemporaryDirectory() as d:
        root = Path(d)
        (root / "scripts").mkdir()
        crate = root / "crate_a" / "src"
        crate.mkdir(parents=True)

        (crate / "wire.rs").write_text(
            "#[derive(Serialize, Deserialize)]\n"
            "pub struct Envelope {\n"
            "    pub id: u64,\n"
            "    pub payload: Payload,\n"
            "}\n"
            "\n"
            "#[derive(Serialize, Deserialize)]\n"
            "pub struct Payload {\n"
            "    pub name: String,\n"
            "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n"
            "    pub note: Option<String>,\n"
            "}\n"
            "\n"
            "#[derive(Serialize, Deserialize)]\n"
            "pub struct NotReachable {\n"
            "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n"
            "    pub memo: Option<String>,\n"
            "}\n"
            "\n"
            "pub fn send(e: &Envelope) -> Vec<u8> {\n"
            "    bincode::serialize(e).unwrap()\n"
            "}\n",
            encoding="utf-8",
        )
        (root / "scripts" / "bincode_boundary_exceptions.json").write_text("{}", encoding="utf-8")

        violations, unresolved, root_names, reached = run(root)
        names = {(v.type_name, v.field_name) for v in violations}
        assert ("Payload", "note") in names, (
            f"Payload.note is nested inside Envelope, a real bincode root -- must be flagged: {names}"
        )
        assert not any(t == "NotReachable" for t, _ in names), (
            f"NotReachable is never serialized with bincode and must not be flagged: {names}"
        )
        assert "Envelope" in root_names, root_names


if __name__ == "__main__":
    raise SystemExit(main())
