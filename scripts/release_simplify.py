"""Safe simplifications of a merged release update, and their verification.

Used by `scripts/release-migrations.py` (a-preview-migrations). The previews of one release cycle
are appended in order (the reference). No released program ever saw the states between them, so
some steps can be folded together:

* fold-add:    `ALTER TABLE t ADD COLUMN c …` into the `CREATE TABLE t` of the same update;
* fold-rename: `ALTER TABLE t RENAME COLUMN a TO b` into that `CREATE TABLE t`;
* drop-pair:   `CREATE TABLE/INDEX/TRIGGER/VIEW x` … `DROP … x` both disappear (with the
               indexes and triggers of a dropped table created between them);
* column-pair: `ALTER TABLE t ADD COLUMN c` … `ALTER TABLE t DROP COLUMN c` both disappear.

A rule fires only if no statement between the two touches the object (a data step, a view,
a trigger, another ALTER) — only other CREATE TABLE / CREATE INDEX statements may mention it.
Anything not recognised stays as it is. Only the additive section is simplified.

`verify` applies the previous state (empty database + the earlier release updates, or a fixture
database of the previous release) plus the appended and plus the simplified version to two
databases and compares the schema (columns, constraints, indexes, triggers, views) and every
table's rows. On any difference the caller writes the appended version.
"""

from __future__ import annotations

import shutil
import sqlite3
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

# --- Lexer -------------------------------------------------------------------------------------


@dataclass
class Tok:
    kind: str  # word, ident (quoted), string, punct, comment, ws
    text: str
    start: int
    end: int

    @property
    def value(self) -> str:
        if self.kind == "ident":
            return self.text[1:-1]
        if self.kind == "string":
            return self.text[1:-1].replace("''", "'")
        return self.text

    def significant(self) -> bool:
        return self.kind not in ("comment", "ws")


def lex(sql: str) -> list[Tok]:
    out: list[Tok] = []
    i, n = 0, len(sql)
    while i < n:
        c = sql[i]
        if sql.startswith("--", i):
            j = sql.find("\n", i)
            j = n if j < 0 else j
            out.append(Tok("comment", sql[i:j], i, j))
        elif sql.startswith("/*", i):
            j = sql.find("*/", i + 2)
            j = n if j < 0 else j + 2
            out.append(Tok("comment", sql[i:j], i, j))
        elif c == "'":
            j = i + 1
            while j < n:
                if sql[j] == "'" and sql.startswith("''", j):
                    j += 2
                elif sql[j] == "'":
                    break
                else:
                    j += 1
            j = min(j + 1, n)
            out.append(Tok("string", sql[i:j], i, j))
        elif c in "\"`[":
            close = {"\"": "\"", "`": "`", "[": "]"}[c]
            j = sql.find(close, i + 1)
            j = n if j < 0 else j + 1
            out.append(Tok("ident", sql[i:j], i, j))
        elif c.isspace():
            j = i
            while j < n and sql[j].isspace():
                j += 1
            out.append(Tok("ws", sql[i:j], i, j))
        elif c.isalnum() or c in "_$":
            j = i
            while j < n and (sql[j].isalnum() or sql[j] in "_$"):
                j += 1
            out.append(Tok("word", sql[i:j], i, j))
        else:
            j = i + 1
            out.append(Tok("punct", c, i, j))
        i = j
    return out


def sig(sql: str) -> list[Tok]:
    return [t for t in lex(sql) if t.significant()]


def normalise(sql: str) -> str:
    """Comparable form: no comments and layout, identifiers unquoted and lower case."""
    parts = []
    for t in sig(sql):
        parts.append(t.text if t.kind == "string" else t.value.lower())
    return " ".join(parts)


# --- Statements --------------------------------------------------------------------------------


@dataclass
class Chunk:
    text: str
    stmt: bool  # False: comments / blank lines only
    info: dict = field(default_factory=dict)


def split(sql: str) -> list[Chunk]:
    """Statements (with their trailing text up to the line end) and comment blocks, in order."""
    chunks: list[Chunk] = []
    buf = ""
    for line in sql.splitlines(keepends=True):
        if not buf and (not line.strip() or line.strip().startswith("--")):
            if chunks and not chunks[-1].stmt:
                chunks[-1].text += line
            else:
                chunks.append(Chunk(line, False))
            continue
        buf += line
        if sqlite3.complete_statement(buf):
            chunks.append(Chunk(buf, True))
            buf = ""
    if buf:
        chunks.append(Chunk(buf, True))
    for c in chunks:
        if c.stmt:
            c.info = classify(c.text)
    return chunks


def _name_after(toks: list[Tok], i: int) -> tuple[str | None, int]:
    words = lambda k: toks[k].value.upper() if k < len(toks) and toks[k].kind == "word" else None  # noqa: E731
    if words(i) == "IF" and words(i + 1) == "NOT" and words(i + 2) == "EXISTS":
        i += 3
    elif words(i) == "IF" and words(i + 1) == "EXISTS":
        i += 2
    if i < len(toks) and toks[i].kind in ("word", "ident"):
        name = toks[i].value
        if i + 2 < len(toks) and toks[i + 1].text == "." and toks[i + 2].kind in ("word", "ident"):
            return toks[i + 2].value.lower(), i + 3
        return name.lower(), i + 1
    return None, i


def classify(sql: str) -> dict:
    t = sig(sql)
    up = [x.value.upper() if x.kind == "word" else x.text for x in t]
    info: dict = {"op": "other"}
    if not up:
        return info
    i = 0
    if up[0] == "CREATE":
        i = 1
        if i < len(up) and up[i] in ("TEMP", "TEMPORARY"):
            i += 1
        unique = i < len(up) and up[i] == "UNIQUE"
        if unique:
            i += 1
        if i < len(up) and up[i] == "VIRTUAL" and up[i + 1 : i + 2] == ["TABLE"]:
            name, _ = _name_after(t, i + 2)
            return {"op": "create", "kind": "vtable", "name": name}
        if i < len(up) and up[i] in ("TABLE", "INDEX", "TRIGGER", "VIEW"):
            kind = up[i].lower()
            name, j = _name_after(t, i + 1)
            info = {"op": "create", "kind": kind, "name": name}
            if kind in ("index", "trigger") and "ON" in up[j:]:
                k = up.index("ON", j)
                info["on"] = t[k + 1].value.lower() if k + 1 < len(t) else None
            return info
    elif up[0] == "DROP" and len(up) > 1 and up[1] in ("TABLE", "INDEX", "TRIGGER", "VIEW"):
        name, _ = _name_after(t, 2)
        return {"op": "drop", "kind": up[1].lower(), "name": name}
    elif up[:2] == ["ALTER", "TABLE"]:
        table, j = _name_after(t, 2)
        if j < len(up) and up[j] == "ADD":
            k = j + 1 + (j + 1 < len(up) and up[j + 1] == "COLUMN")
            end = len(t) - 1 if t[-1].text == ";" else len(t)
            if k < end:
                return {"op": "add", "table": table, "column": t[k].value.lower(),
                        "def": sql[t[k].start : t[end - 1].end]}
        if j < len(up) and up[j] == "DROP":
            k = j + 1 + (j + 1 < len(up) and up[j + 1] == "COLUMN")
            if k < len(t):
                return {"op": "dropcol", "table": table, "column": t[k].value.lower()}
        if j < len(up) and up[j] == "RENAME":
            k = j + 1 + (j + 1 < len(up) and up[j + 1] == "COLUMN")
            if k + 2 < len(t) and up[k] != "TO" and up[k + 1] == "TO":
                return {"op": "rename", "table": table, "old": t[k].value.lower(), "new": t[k + 2].value}
    return info


def mentions(chunk: Chunk, name: str) -> bool:
    return any(t.kind in ("word", "ident", "string") and t.value.lower() == name for t in sig(chunk.text))


def only_ddl_between(chunks: list[Chunk], i: int, j: int, table: str) -> bool:
    """True if every statement strictly between i and j that mentions `table` is a CREATE TABLE of
    another table, a CREATE VIRTUAL TABLE or a CREATE INDEX (no data step, view, trigger, ALTER)."""
    for c in chunks[i + 1 : j]:
        if c.stmt and mentions(c, table):
            op, kind = c.info.get("op"), c.info.get("kind")
            if not (op == "create" and (kind == "index" or kind == "vtable" or (kind == "table" and c.info["name"] != table))):
                return False
    return True


def _latest(chunks: list[Chunk], j: int, pred) -> int | None:
    for i in range(j - 1, -1, -1):
        if chunks[i].stmt and pred(chunks[i].info):
            return i
    return None


def _body(sql: str) -> tuple[list[Tok], int, int] | None:
    """Significant tokens of a CREATE TABLE and the indexes of its opening and closing paren."""
    t = sig(sql)
    try:
        open_ = next(k for k, x in enumerate(t) if x.text == "(")
    except StopIteration:
        return None
    depth = 0
    for k in range(open_, len(t)):
        if t[k].text == "(":
            depth += 1
        elif t[k].text == ")":
            depth -= 1
            if depth == 0:
                return t, open_, k
    return None


def _items(t: list[Tok], open_: int, close: int) -> list[tuple[int, int]]:
    items, depth, start = [], 0, open_ + 1
    for k in range(open_ + 1, close):
        if t[k].text == "(":
            depth += 1
        elif t[k].text == ")":
            depth -= 1
        elif t[k].text == "," and depth == 0:
            items.append((start, k - 1))
            start = k + 1
    items.append((start, close - 1))
    return items


CONSTRAINT_WORDS = {"CONSTRAINT", "PRIMARY", "UNIQUE", "CHECK", "FOREIGN"}


def _fold_add(create: str, coldef: str) -> str | None:
    b = _body(create)
    if not b:
        return None
    t, open_, close = b
    cols = [(s, e) for s, e in _items(t, open_, close) if t[s].value.upper() not in CONSTRAINT_WORDS]
    if not cols:
        return None
    pos = t[cols[-1][1]].end
    return create[:pos] + ",\n    " + coldef.strip() + create[pos:]


def _fold_rename(create: str, old: str, new: str) -> str | None:
    b = _body(create)
    if not b:
        return None
    t, open_, close = b
    hits = [k for k in range(open_ + 1, close) if t[k].kind in ("word", "ident", "string") and t[k].value.lower() == old]
    if len(hits) != 1:
        return None
    k = hits[0]
    if not any(s == k for s, _ in _items(t, open_, close)):
        return None
    return create[: t[k].start] + new + create[t[k].end :]


def _set(chunks: list[Chunk], i: int, text: str) -> None:
    chunks[i] = Chunk(text, True, classify(text))


def _one_step(chunks: list[Chunk]) -> str | None:
    # Pairs that vanish first, so that an added-and-dropped column is not folded.
    return _one_pass(chunks, ("drop", "dropcol")) or _one_pass(chunks, ("add", "rename"))


def _one_pass(chunks: list[Chunk], ops: tuple[str, ...]) -> str | None:
    for j, c in enumerate(chunks):
        if not c.stmt or c.info["op"] not in ops:
            continue
        info = c.info
        if info["op"] == "drop" and info["name"]:
            kinds = ("table", "vtable") if info["kind"] == "table" else (info["kind"],)
            i = _latest(chunks, j, lambda x: x.get("op") == "create" and x.get("kind") in kinds and x.get("name") == info["name"])
            if i is None:
                continue
            name, dependants, ok = info["name"], [], True
            for k in range(i + 1, j):
                ck = chunks[k]
                if ck.stmt and mentions(ck, name):
                    if info["kind"] == "table" and ck.info.get("kind") in ("index", "trigger") and ck.info.get("on") == name:
                        dependants.append(k)
                    else:
                        ok = False
                        break
            if ok:
                for k in sorted([i, j, *dependants], reverse=True):
                    del chunks[k]
                extra = f" (and {len(dependants)} index/trigger)" if dependants else ""
                return f"drop-pair: {info['kind']} `{name}` created and dropped again{extra}"
        elif info["op"] == "dropcol":
            col, table = info["column"], info["table"]
            i = _latest(chunks, j, lambda x: x.get("op") == "add" and x.get("table") == table and x.get("column") == col)
            if i is None:
                continue
            between = [ck for ck in chunks[i + 1 : j] if ck.stmt]
            if not any(mentions(ck, col) for ck in between) and only_ddl_between(chunks, i, j, table):
                del chunks[j]
                del chunks[i]
                return f"column-pair: `{table}.{col}` added and dropped again"
        elif info["op"] in ("add", "rename"):
            table = info["table"]
            i = _latest(chunks, j, lambda x: x.get("op") in ("create", "drop") and x.get("kind") in ("table", "vtable") and x.get("name") == table)
            if i is None or chunks[i].info["op"] != "create" or chunks[i].info["kind"] != "table":
                continue
            if not only_ddl_between(chunks, i, j, table):
                continue
            if info["op"] == "add":
                new = _fold_add(chunks[i].text, info["def"])
                what = f"fold-add: column `{info['column']}` into CREATE TABLE `{table}`"
            else:
                if any(ck.stmt and mentions(ck, info["old"]) for ck in chunks[i + 1 : j]):
                    continue
                new = _fold_rename(chunks[i].text, info["old"], info["new"])
                what = f"fold-rename: `{table}.{info['old']}` → `{info['new']}` in CREATE TABLE"
            if new is None:
                continue
            _set(chunks, i, new)
            del chunks[j]
            return what
    return None


def simplify(sql: str) -> tuple[str, list[str]]:
    """The simplified SQL and one note per applied simplification."""
    chunks = split(sql)
    notes = []
    while (note := _one_step(chunks)) is not None:
        notes.append(note)
    return "".join(c.text for c in chunks), notes


# --- Verification ------------------------------------------------------------------------------


def _table_signature(conn: sqlite3.Connection, name: str, sql: str):
    cols = conn.execute("SELECT cid, name, type, \"notnull\", dflt_value, pk, hidden FROM pragma_table_xinfo(?)", (name,)).fetchall()
    b = _body(sql)
    items, tail = [], normalise(sql)
    if b:
        t, open_, close = b
        items = sorted(normalise(sql[t[s].start : t[e].end]) for s, e in _items(t, open_, close))
        tail = normalise(sql[t[close].end :])
    return ("table", tuple(cols), tuple(items), tail)


def schema(conn: sqlite3.Connection) -> dict:
    out = {}
    for type_, name, tbl, sql in conn.execute("SELECT type, name, tbl_name, sql FROM sqlite_master"):
        if type_ == "table" and sql and sql.upper().startswith("CREATE TABLE"):
            out[(type_, name.lower())] = (tbl.lower(), _table_signature(conn, name, sql))
        else:
            out[(type_, name.lower())] = (tbl.lower(), normalise(sql or ""))
    return out


def contents(conn: sqlite3.Connection) -> dict:
    out = {}
    for (name,) in conn.execute("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name"):
        try:
            rows = conn.execute(f'SELECT * FROM "{name}"').fetchall()
        except sqlite3.Error:
            continue
        out[name.lower()] = sorted(rows, key=repr)
    return out


def _apply(path: Path, scripts: list[str]) -> sqlite3.Connection:
    conn = sqlite3.connect(path)
    for s in scripts:
        conn.executescript(s)
    return conn


def compare(base: Path | None, earlier: list[str], appended: str, simplified: str) -> str | None:
    """None if both versions give the same schema and rows; else where they diverge."""
    with tempfile.TemporaryDirectory() as tmp:
        dbs = []
        for k, sql in enumerate((appended, simplified)):
            path = Path(tmp) / f"{k}.sqlite"
            if base is not None:
                shutil.copyfile(base, path)
            try:
                dbs.append(_apply(path, ([] if base is not None else earlier) + [sql]))
            except sqlite3.Error as e:
                return f"the {'appended' if k == 0 else 'simplified'} version fails: {e}"
        a, b = (schema(c) for c in dbs)
        for key in sorted(set(a) | set(b)):
            if a.get(key) != b.get(key):
                return f"schema differs at {key[0]} `{key[1]}`"
        ca, cb = (contents(c) for c in dbs)
        for key in sorted(set(ca) | set(cb)):
            if ca.get(key) != cb.get(key):
                return f"rows differ in table `{key}`"
        for c in dbs:
            c.close()
    return None


def verify(earlier: list[str], appended: str, simplified: str, fixture: Path | None) -> str | None:
    """Compare on an empty database with the earlier release updates and, if given, on a copy of
    the fixture database of the previous release."""
    problem = compare(None, earlier, appended, simplified)
    if problem is None and fixture is not None:
        problem = compare(fixture, [], appended, simplified)
        if problem:
            problem += " (on the fixture)"
    return problem


__all__ = ["simplify", "verify", "split", "classify", "normalise", "schema", "contents"]
