#!/usr/bin/env python3
"""Fails on user-visible string literals in the web client (R-I18N-010, d-i18n-fluent).

Every text the client shows comes from the catalogue (`musicus_i18n::t::…`). This lint looks at
`crates/musicus-web/src/**/*.rs` (test modules skipped):

- inside `rsx! { … }`: every string literal with letters (after removing `{placeholders}`), except
  values of non-text attributes (`class`, `id`, `key`, `r#type`, …, see NON_TEXT);
- outside `rsx!`: literals passed to `confirm(` / `prompt(`, and literals turned into a `String`
  (`.into()`, `.to_owned()`, `.to_string()`, `format!(`) that look like prose (start with a capital
  letter, or contain "…", "?", "!", ". ").

Allowed: `ALLOW` (symbols, brand), literals starting with "/" (paths) or containing "://", and any
line with the marker `// i18n-ok` (data formats, CSS, JavaScript; say why next to it).

Usage: python3 scripts/check-ui-literals.py [--list]   (exit 1 when something is found)
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "crates" / "musicus-web" / "src"
# Files skipped by the lint (none: every text, also every error text, is a catalogue message).
EXCLUDE: set[str] = set()
ALLOW = {"𝄞 Musicus", "Musicus", "×", "↑", "↓", "☰", "✓", "✎", "▸", "▾", "—", "–", "·", "…"}
NON_TEXT = {
    "class", "id", "key", "r#type", "name", "value", "role", "href", "src", "to", "accept", "autocomplete",
    "inputmode", "r#for", "for", "style", "content", "rel", "method", "tabindex", "labelledby", "separators",
    "draggable", "enterkeyhint", "spellcheck", "autocapitalize", "pattern", "wrap", "capture", "active_class",
    "aria-controls", "aria-labelledby", "aria-describedby", "aria-haspopup", "aria-live", "aria-modal",
    "aria-expanded", "aria-pressed", "aria-selected", "aria-multiselectable", "aria-current", "aria-hidden",
    "aria-checked", "aria-busy", "aria-orientation", "aria-activedescendant", "aria-autocomplete",
    "aria-valuenow", "aria-valuemin", "aria-valuemax", "list", "min", "max", "step", "size", "width", "height",
    "viewBox", "d", "fill", "stroke", "target", "download", "lang", "dir", "headers", "colspan", "rowspan",
    "data-label-key", "webkitdirectory", "multiple", "translate", "loading", "decoding",
}
LITERAL = re.compile(r'(?<![A-Za-z0-9_#])(?:r#)?"((?:[^"\\]|\\.)*)"')
ATTR = re.compile(r'(?:"([A-Za-z0-9_\-]+)"|(?<![A-Za-z0-9_:])([A-Za-z_#][A-Za-z0-9_#]*))\s*:(?!:)')
MARK = "// i18n-ok"


def core(s: str) -> str:
    return re.sub(r"\{[^{}]*\}", "", s)


def has_letters(s: str) -> bool:
    return re.search(r"[^\W\d_]{2,}", core(s)) is not None


def prose(s: str) -> bool:
    c = core(s).strip()
    return bool(c) and (c[0].isupper() or any(x in c for x in ("…", "?", "!", ". ")))


def allowed(s: str) -> bool:
    return s in ALLOW or s.startswith("/") or "://" in s or not has_letters(s)


def scan(path: Path) -> list[tuple[int, str]]:
    text = path.read_text(encoding="utf-8").split("#[cfg(test)]")[0]
    found = []
    depth = 0  # brace depth inside rsx!, 0 = outside
    prev = ""  # previous code line (a call split over lines: `confirm(` then the literal)
    pending = False  # saw `rsx!`, waiting for its brace
    for ln, line in enumerate(text.splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("//") or stripped.startswith("///"):
            continue
        marked = MARK in line
        # Walk the line: track rsx! braces and literals in order.
        i = 0
        while i < len(line):
            if line.startswith("rsx!", i) and depth == 0:
                pending = True
                i += 4
                continue
            ch = line[i]
            if ch == "/" and line.startswith("//", i):
                break
            if ch == "'" and re.match(r"'(\\.|[^\\'])'", line[i:]):
                i += len(re.match(r"'(\\.|[^\\'])'", line[i:]).group(0))
                continue
            m = LITERAL.match(line, i) if ch in '"r' else None
            if m and (ch == '"' or line.startswith('r#"', i) or line.startswith('r"', i)):
                s = m.group(1)
                if not marked and not allowed(s):
                    before = line[:i]
                    if re.match(r"\s*:(?!:)", line[m.end():]) or s in ("true", "false"):
                        pass  # an attribute name, or a boolean attribute value
                    elif depth > 0:
                        attr = attribute_of(before)
                        if attr not in NON_TEXT and not attr_is_data(attr):
                            found.append((ln, s))
                    else:
                        after = line[m.end():]
                        call = re.search(r"(confirm|prompt)\(\s*&?$", before) or (
                            not before.strip() and re.search(r"(confirm|prompt)\(\s*$", prev)
                        )
                        stringy = after.startswith((".into()", ".to_owned()", ".to_string()")) or re.search(
                            r"(format!|String::from)\(\s*$", before
                        )
                        if call or (stringy and prose(s)):
                            found.append((ln, s))
                i = m.end()
                continue
            if depth > 0 or pending:
                if ch == "{":
                    if pending:
                        pending, depth = False, 1
                    else:
                        depth += 1
                elif ch == "}" and depth > 0:
                    depth -= 1
            i += 1
        if stripped:
            prev = stripped
    return found


def attribute_of(before: str) -> str | None:
    """The attribute whose value the literal belongs to: the last `name:` on the line with no
    top-level comma after it (`class: if a { "x" } else { "y" }` belongs to `class`)."""
    matches = list(ATTR.finditer(before))
    if not matches:
        return None
    m = matches[-1]
    depth = 0
    for ch in before[m.end():]:
        if ch in "({[":
            depth += 1
        elif ch in ")}]":
            depth -= 1
        elif ch == "," and depth <= 0:
            return None
    return m.group(1) or m.group(2)


def attr_is_data(attr: str | None) -> bool:
    return bool(attr) and attr.startswith("data-") and attr != "data-label"


def main() -> int:
    total = 0
    for f in sorted(ROOT.rglob("*.rs")):
        if f.name in EXCLUDE:
            continue
        for ln, s in scan(f):
            total += 1
            print(f"{f.relative_to(ROOT.parent.parent.parent)}:{ln}: literal UI text \"{s}\" (use musicus_i18n::t)")
    if total:
        print(f"{total} literal UI text(s); move them into crates/musicus-i18n/locales/en/*.ftl (R-I18N-010)")
        return 1
    print("ok: no literal UI text in the web client")
    return 0


if __name__ == "__main__":
    sys.exit(main())
