from __future__ import annotations

import hashlib
import json
import math
import os
import re
import sqlite3
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

from .paths import memory_store_path, persona_path
from .state import (
    append_event,
    load_memory_config,
    load_mission,
    now_utc,
    save_memory_config,
)


TEXT_SUFFIXES = {
    ".c",
    ".cc",
    ".cfg",
    ".conf",
    ".cpp",
    ".css",
    ".go",
    ".h",
    ".hpp",
    ".html",
    ".java",
    ".js",
    ".json",
    ".jsx",
    ".md",
    ".prompt",
    ".py",
    ".rb",
    ".rs",
    ".sh",
    ".sql",
    ".swift",
    ".toml",
    ".ts",
    ".tsx",
    ".txt",
    ".yaml",
    ".yml",
    ".zsh",
}

IGNORED_DIR_NAMES = {
    ".git",
    ".hg",
    ".idea",
    ".next",
    ".venv",
    "__pycache__",
    "build",
    "cache",
    "dist",
    "ide",
    "logs",
    "node_modules",
    "output",
    "session-state",
    "target",
    "tmp",
    "venv",
}

INSTRUCTION_DIRS = (".claude", ".copilot", ".agents", ".agent")
INSTRUCTION_FILES = ("CLAUDE.md", "AGENTS.md")
INSTRUCTION_SUFFIXES = {".json", ".md", ".prompt", ".toml", ".txt", ".yaml", ".yml"}
MAX_TEXT_BYTES = 256 * 1024
MAX_INSTRUCTION_BYTES = 128 * 1024
SQLITE_TIMEOUT_SECONDS = 10.0
SQLITE_BUSY_TIMEOUT_MS = 10_000


def _enable_wal(connection: sqlite3.Connection) -> None:
    try:
        row = connection.execute("pragma journal_mode").fetchone()
        current_mode = str(row[0]).lower() if row else ""
        if current_mode == "wal":
            return
        connection.execute("pragma journal_mode = wal")
    except sqlite3.OperationalError:
        # Another process may already hold the database. Keep the connection usable
        # instead of failing the whole command or TUI refresh.
        return


def _connect() -> sqlite3.Connection:
    path = memory_store_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    connection = sqlite3.connect(path, timeout=SQLITE_TIMEOUT_SECONDS)
    connection.row_factory = sqlite3.Row
    connection.execute(f"pragma busy_timeout = {SQLITE_BUSY_TIMEOUT_MS}")
    connection.execute("pragma foreign_keys = on")
    _enable_wal(connection)
    _ensure_schema(connection)
    return connection


def _ensure_schema(connection: sqlite3.Connection) -> None:
    connection.executescript(
        """
        create table if not exists workspaces (
            path text primary key,
            repo_root text not null,
            enrolled_at text not null,
            last_indexed_at text,
            last_summary_at text
        );

        create table if not exists documents (
            id integer primary key,
            workspace text not null,
            path text not null unique,
            rel_path text not null,
            kind text not null,
            language text,
            source_type text not null,
            content_hash text not null,
            mtime real not null,
            size integer not null,
            weight real not null default 0,
            updated_at text not null
        );

        create table if not exists chunks (
            id integer primary key,
            document_id integer not null references documents(id) on delete cascade,
            workspace text not null,
            path text not null,
            chunk_index integer not null,
            content text not null,
            content_hash text not null,
            token_count integer not null,
            updated_at text not null,
            unique(document_id, chunk_index)
        );

        create virtual table if not exists chunk_fts using fts5(
            chunk_id unindexed,
            workspace,
            path,
            content
        );

        create table if not exists embeddings (
            chunk_id integer primary key references chunks(id) on delete cascade,
            dims integer not null,
            vector_json text not null
        );

        create table if not exists instruction_sources (
            id integer primary key,
            workspace text,
            path text not null unique,
            scope text not null,
            source_kind text not null,
            weight real not null,
            content text not null,
            content_hash text not null,
            updated_at text not null
        );

        create virtual table if not exists instruction_fts using fts5(
            source_id unindexed,
            path,
            content
        );

        create table if not exists persona_facts (
            id integer primary key,
            fact text not null unique,
            weight real not null,
            source_path text,
            updated_at text not null
        );

        create table if not exists mission_summaries (
            mission_id text primary key,
            workspace text not null,
            title text not null,
            status text not null,
            summary text not null,
            keywords_json text not null,
            updated_at text not null
        );

        create virtual table if not exists mission_fts using fts5(
            mission_id unindexed,
            workspace,
            title,
            summary
        );

        create table if not exists decisions (
            decision_id text primary key,
            mission_id text not null,
            step_id text,
            workspace text not null,
            title text not null,
            summary text not null,
            status text not null,
            weight real not null,
            updated_at text not null
        );

        create virtual table if not exists decision_fts using fts5(
            decision_id unindexed,
            workspace,
            title,
            summary
        );

        create table if not exists decision_edges (
            id integer primary key,
            from_decision_id text not null,
            to_decision_id text not null,
            edge_type text not null,
            created_at text not null,
            unique(from_decision_id, to_decision_id, edge_type)
        );

        create table if not exists entities (
            id integer primary key,
            name text not null,
            entity_type text not null,
            source text,
            updated_at text not null,
            unique(name, entity_type)
        );
        """
    )
    connection.commit()


def _normalize_workspace(path: str | Path) -> Path:
    return Path(path).expanduser().resolve()


def _detect_repo_root(workspace: Path) -> Path:
    for candidate in [workspace, *workspace.parents]:
        if (candidate / ".git").exists():
            return candidate
    return workspace


def _sanitize_fts_query(query: str) -> str:
    tokens = re.findall(r"[A-Za-z0-9_.:/-]+", query.lower())
    return " ".join(tokens[:12]) or "constant"


def _tokenize(text: str) -> list[str]:
    return re.findall(r"[A-Za-z0-9_./:-]{2,}", text.lower())


def _sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _embed_text(text: str, dims: int) -> list[float]:
    vector = [0.0] * dims
    if dims <= 0:
        return vector
    for token, count in Counter(_tokenize(text)).items():
        digest = hashlib.sha256(token.encode("utf-8")).digest()
        index = int.from_bytes(digest[:4], "big") % dims
        sign = 1.0 if digest[4] % 2 == 0 else -1.0
        vector[index] += sign * (1.0 + math.log1p(count))
    norm = math.sqrt(sum(value * value for value in vector))
    if norm == 0:
        return vector
    return [round(value / norm, 6) for value in vector]


def _cosine_similarity(vec_a: list[float], vec_b: list[float]) -> float:
    if not vec_a or not vec_b or len(vec_a) != len(vec_b):
        return 0.0
    return sum(a * b for a, b in zip(vec_a, vec_b))


def _relative_path(path: Path, root: Path) -> str:
    try:
        return str(path.relative_to(root))
    except ValueError:
        return str(path)


def _language_for(path: Path) -> str:
    suffix = path.suffix.lower()
    return {
        ".py": "python",
        ".sh": "shell",
        ".zsh": "shell",
        ".js": "javascript",
        ".jsx": "javascript",
        ".ts": "typescript",
        ".tsx": "typescript",
        ".json": "json",
        ".yaml": "yaml",
        ".yml": "yaml",
        ".md": "markdown",
        ".toml": "toml",
        ".rs": "rust",
        ".go": "go",
        ".swift": "swift",
        ".html": "html",
        ".css": "css",
    }.get(suffix, suffix.lstrip(".") or "text")


def _is_probably_text(path: Path, max_bytes: int) -> bool:
    if not path.is_file():
        return False
    try:
        if path.stat().st_size > max_bytes:
            return False
        with path.open("rb") as handle:
            sample = handle.read(2048)
        return b"\x00" not in sample
    except OSError:
        return False


def _read_text(path: Path, max_bytes: int) -> str | None:
    if not _is_probably_text(path, max_bytes):
        return None
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        try:
            return path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            return None
    except OSError:
        return None


def _should_index_repo_file(path: Path) -> bool:
    if path.name.startswith(".") and path.name not in {".gitignore", ".env.example"}:
        return path.name in INSTRUCTION_FILES
    if path.suffix.lower() in TEXT_SUFFIXES:
        return True
    return path.name in {"Dockerfile", "Makefile", "justfile"}


def _walk_repo_files(workspace: Path) -> list[Path]:
    files: list[Path] = []
    for root, dirnames, filenames in os.walk(workspace):
        dirnames[:] = sorted(name for name in dirnames if name not in IGNORED_DIR_NAMES)
        for filename in sorted(filenames):
            path = Path(root) / filename
            if _should_index_repo_file(path):
                files.append(path)
    return files


def _iter_instruction_candidates(base: Path) -> list[Path]:
    candidates: list[Path] = []
    for name in INSTRUCTION_FILES:
        candidate = base / name
        if candidate.exists():
            candidates.append(candidate)

    for dirname in INSTRUCTION_DIRS:
        directory = base / dirname
        if not directory.exists() or not directory.is_dir():
            continue
        for root, dirnames, filenames in os.walk(directory):
            dirnames[:] = sorted(name for name in dirnames if name not in IGNORED_DIR_NAMES)
            depth = len(Path(root).relative_to(directory).parts)
            if depth > 3:
                dirnames[:] = []
                continue
            for filename in sorted(filenames):
                path = Path(root) / filename
                if path.suffix.lower() in INSTRUCTION_SUFFIXES or filename in {"config.json", "settings.json"}:
                    candidates.append(path)
    return candidates


def _instruction_scope(path: Path, workspace: Path, repo_root: Path, config: dict[str, Any]) -> tuple[str, float]:
    weights = config["instruction_weights"]
    home = Path.home().resolve()
    if path == workspace or workspace in path.parents:
        return "workspace", float(weights["workspace"])
    if path == repo_root or repo_root in path.parents:
        return "repo", float(weights["repo"])
    if home == path.parent or home in path.parents:
        return "user", float(weights["user"])
    return "ancestor", float(weights["ancestor"])


def _discover_instruction_files(workspace: Path) -> list[dict[str, Any]]:
    config = load_memory_config()
    repo_root = _detect_repo_root(workspace)
    seen: set[str] = set()
    entries: list[dict[str, Any]] = []
    bases = [workspace, repo_root, *repo_root.parents]
    home = Path.home().resolve()
    if home not in bases:
        bases.append(home)

    for base in bases:
        if not str(base).startswith(str(home)) and base != workspace and base != repo_root:
            continue
        for candidate in _iter_instruction_candidates(base):
            key = str(candidate.resolve())
            if key in seen:
                continue
            seen.add(key)
            content = _read_text(candidate, MAX_INSTRUCTION_BYTES)
            if not content or not content.strip():
                continue
            scope, weight = _instruction_scope(candidate.resolve(), workspace, repo_root, config)
            entries.append(
                {
                    "path": candidate.resolve(),
                    "scope": scope,
                    "weight": weight,
                    "kind": candidate.parent.name if candidate.parent != candidate.parent.parent else candidate.name,
                    "content": content,
                }
            )
    return sorted(entries, key=lambda item: (-item["weight"], str(item["path"])))


def _chunk_text(text: str, target_chars: int = 1200, overlap_lines: int = 3) -> list[str]:
    lines = text.splitlines()
    if not lines:
        return []

    chunks: list[str] = []
    start = 0
    while start < len(lines):
        total = 0
        end = start
        while end < len(lines) and total < target_chars:
            total += len(lines[end]) + 1
            end += 1
        chunk = "\n".join(lines[start:end]).strip()
        if chunk:
            chunks.append(chunk)
        if end >= len(lines):
            break
        start = max(end - overlap_lines, start + 1)
    return chunks


def _upsert_workspace(connection: sqlite3.Connection, workspace: Path) -> None:
    repo_root = _detect_repo_root(workspace)
    connection.execute(
        """
        insert into workspaces(path, repo_root, enrolled_at, last_indexed_at, last_summary_at)
        values (?, ?, ?, null, null)
        on conflict(path) do update set repo_root=excluded.repo_root
        """,
        (str(workspace), str(repo_root), now_utc()),
    )


def _prune_missing_documents(connection: sqlite3.Connection, workspace: Path, keep_paths: set[str]) -> int:
    removed = 0
    rows = connection.execute("select id, path from documents where workspace = ?", (str(workspace),)).fetchall()
    for row in rows:
        if row["path"] not in keep_paths:
            connection.execute("delete from chunk_fts where chunk_id in (select id from chunks where document_id = ?)", (row["id"],))
            connection.execute("delete from documents where id = ?", (row["id"],))
            removed += 1
    return removed


def _index_repo_documents(connection: sqlite3.Connection, workspace: Path, dims: int) -> dict[str, int]:
    indexed = 0
    skipped = 0
    chunks_written = 0
    files = _walk_repo_files(workspace)
    keep_paths = {str(path.resolve()) for path in files}
    pruned = _prune_missing_documents(connection, workspace, keep_paths)

    for path in files:
        resolved = path.resolve()
        stat = resolved.stat()
        existing = connection.execute(
            "select id, mtime, size from documents where path = ?",
            (str(resolved),),
        ).fetchone()
        if existing and float(existing["mtime"]) == stat.st_mtime and int(existing["size"]) == stat.st_size:
            skipped += 1
            continue

        content = _read_text(resolved, MAX_TEXT_BYTES)
        if not content or not content.strip():
            skipped += 1
            continue

        digest = _sha256_text(content)
        if existing:
            connection.execute("delete from chunk_fts where chunk_id in (select id from chunks where document_id = ?)", (existing["id"],))
            connection.execute("delete from documents where id = ?", (existing["id"],))

        cursor = connection.execute(
            """
            insert into documents(workspace, path, rel_path, kind, language, source_type, content_hash, mtime, size, weight, updated_at)
            values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                str(workspace),
                str(resolved),
                _relative_path(resolved, workspace),
                "repo",
                _language_for(resolved),
                "repo",
                digest,
                stat.st_mtime,
                stat.st_size,
                1.0,
                now_utc(),
            ),
        )
        document_id = int(cursor.lastrowid)
        for chunk_index, chunk in enumerate(_chunk_text(content), start=1):
            chunk_hash = _sha256_text(chunk)
            chunk_cursor = connection.execute(
                """
                insert into chunks(document_id, workspace, path, chunk_index, content, content_hash, token_count, updated_at)
                values (?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    document_id,
                    str(workspace),
                    str(resolved),
                    chunk_index,
                    chunk,
                    chunk_hash,
                    len(_tokenize(chunk)),
                    now_utc(),
                ),
            )
            chunk_id = int(chunk_cursor.lastrowid)
            connection.execute(
                "insert into chunk_fts(chunk_id, workspace, path, content) values (?, ?, ?, ?)",
                (chunk_id, str(workspace), str(resolved), chunk),
            )
            connection.execute(
                "insert into embeddings(chunk_id, dims, vector_json) values (?, ?, ?)",
                (chunk_id, dims, json.dumps(_embed_text(chunk, dims))),
            )
            chunks_written += 1
        indexed += 1

    connection.execute(
        "update workspaces set last_indexed_at = ? where path = ?",
        (now_utc(), str(workspace)),
    )
    return {"indexed": indexed, "skipped": skipped, "pruned": pruned, "chunks": chunks_written}


def _extract_persona_facts(text: str) -> list[str]:
    facts: list[str] = []
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        if line.startswith(("#", "```", "{", "}", "[", "]")):
            continue
        if len(line) < 12 or len(line) > 220:
            continue
        cleaned = re.sub(r"^[*-]\s*", "", line)
        if cleaned and cleaned not in facts:
            facts.append(cleaned)
        if len(facts) >= 24:
            break
    return facts


def _refresh_instruction_sources(connection: sqlite3.Connection, workspace: Path) -> dict[str, int]:
    discovered = _discover_instruction_files(workspace)
    keep_paths = {str(entry["path"]) for entry in discovered}
    current = connection.execute("select path from instruction_sources").fetchall()
    removed = 0
    for row in current:
        if row["path"] not in keep_paths:
            connection.execute("delete from instruction_fts where path = ?", (row["path"],))
            connection.execute("delete from instruction_sources where path = ?", (row["path"],))
            removed += 1

    refreshed = 0
    facts = 0
    for entry in discovered:
        path = entry["path"]
        content = entry["content"]
        digest = _sha256_text(content)
        existing = connection.execute(
            "select content_hash from instruction_sources where path = ?",
            (str(path),),
        ).fetchone()
        if existing and existing["content_hash"] == digest:
            continue
        connection.execute("delete from instruction_fts where path = ?", (str(path),))
        connection.execute(
            """
            insert into instruction_sources(workspace, path, scope, source_kind, weight, content, content_hash, updated_at)
            values (?, ?, ?, ?, ?, ?, ?, ?)
            on conflict(path) do update set
                workspace=excluded.workspace,
                scope=excluded.scope,
                source_kind=excluded.source_kind,
                weight=excluded.weight,
                content=excluded.content,
                content_hash=excluded.content_hash,
                updated_at=excluded.updated_at
            """,
            (
                str(workspace),
                str(path),
                entry["scope"],
                entry["kind"],
                float(entry["weight"]),
                content,
                digest,
                now_utc(),
            ),
        )
        connection.execute(
            "insert into instruction_fts(source_id, path, content) values ((select id from instruction_sources where path = ?), ?, ?)",
            (str(path), str(path), content),
        )
        for fact in _extract_persona_facts(content):
            connection.execute(
                """
                insert into persona_facts(fact, weight, source_path, updated_at)
                values (?, ?, ?, ?)
                on conflict(fact) do update set
                    weight=max(weight, excluded.weight),
                    source_path=excluded.source_path,
                    updated_at=excluded.updated_at
                """,
                (fact, float(entry["weight"]), str(path), now_utc()),
            )
            facts += 1
        refreshed += 1

    return {"sources": len(discovered), "refreshed": refreshed, "removed": removed, "persona_facts": facts}


def _render_persona(connection: sqlite3.Connection) -> str:
    rows = connection.execute(
        "select fact, weight, source_path from persona_facts order by weight desc, fact asc limit 32"
    ).fetchall()
    mission_rows = connection.execute(
        "select mission_id, title, summary from mission_summaries order by updated_at desc limit 6"
    ).fetchall()
    decision_rows = connection.execute(
        "select decision_id, title, summary from decisions order by updated_at desc limit 8"
    ).fetchall()

    lines = [
        "# Constant Persona",
        "",
        "## Durable Rules",
    ]
    if rows:
        for row in rows:
            lines.append(f"- {row['fact']} ({Path(row['source_path']).name})")
    else:
        lines.append("- No durable rules extracted yet.")

    lines.extend(["", "## Recent Mission Summaries"])
    if mission_rows:
        for row in mission_rows:
            lines.append(f"- `{row['mission_id']}` {row['title']}: {row['summary']}")
    else:
        lines.append("- No mission summaries yet.")

    lines.extend(["", "## Decision Graph Snapshot"])
    if decision_rows:
        for row in decision_rows:
            lines.append(f"- `{row['decision_id']}` {row['title']}: {row['summary']}")
    else:
        lines.append("- No decisions captured yet.")
    return "\n".join(lines).strip() + "\n"


def rebuild_workspace_memory(workspace: str | Path, enroll: bool = True) -> dict[str, Any]:
    workspace_path = _normalize_workspace(workspace)
    config = load_memory_config()
    if enroll and str(workspace_path) not in config["workspace_enrollments"]:
        config["workspace_enrollments"].append(str(workspace_path))
        config["workspace_enrollments"] = sorted(set(config["workspace_enrollments"]))
        save_memory_config(config)

    connection = _connect()
    try:
        _upsert_workspace(connection, workspace_path)
        dims = int(config.get("vector_dimensions", 96))
        repo_stats = _index_repo_documents(connection, workspace_path, dims)
        instruction_stats = _refresh_instruction_sources(connection, workspace_path)
        persona_markdown = _render_persona(connection)
        persona_path().parent.mkdir(parents=True, exist_ok=True)
        persona_path().write_text(persona_markdown, encoding="utf-8")
        connection.commit()
        return {
            "workspace": str(workspace_path),
            "repo_root": str(_detect_repo_root(workspace_path)),
            "repo": repo_stats,
            "instructions": instruction_stats,
            "persona_path": str(persona_path()),
            "store_path": str(memory_store_path()),
        }
    finally:
        connection.close()


def prime_workspace_memory(workspace: str | Path, enroll: bool = True) -> dict[str, Any]:
    workspace_path = _normalize_workspace(workspace)
    config = load_memory_config()
    if enroll and str(workspace_path) not in config["workspace_enrollments"]:
        config["workspace_enrollments"].append(str(workspace_path))
        config["workspace_enrollments"] = sorted(set(config["workspace_enrollments"]))
        save_memory_config(config)

    connection = _connect()
    try:
        _upsert_workspace(connection, workspace_path)
        row = connection.execute(
            """
            select count(*) as document_count, max(updated_at) as last_document_at
            from documents
            where workspace = ?
            """,
            (str(workspace_path),),
        ).fetchone()
        workspace_row = connection.execute(
            "select repo_root, last_indexed_at from workspaces where path = ?",
            (str(workspace_path),),
        ).fetchone()
        connection.commit()
        return {
            "workspace": str(workspace_path),
            "repo_root": str(workspace_row["repo_root"]) if workspace_row else str(_detect_repo_root(workspace_path)),
            "enrolled": str(workspace_path) in load_memory_config()["workspace_enrollments"],
            "documents": int(row["document_count"] or 0) if row else 0,
            "last_document_at": row["last_document_at"] if row else None,
            "last_indexed_at": workspace_row["last_indexed_at"] if workspace_row else None,
            "store_path": str(memory_store_path()),
            "mode": "prime",
        }
    finally:
        connection.close()


def memory_status(workspace: str | None = None) -> dict[str, Any]:
    connection = _connect()
    try:
        payload = {
            "store_path": str(memory_store_path()),
            "persona_path": str(persona_path()),
            "enrollments": load_memory_config()["workspace_enrollments"],
            "counts": {
                "workspaces": connection.execute("select count(*) from workspaces").fetchone()[0],
                "documents": connection.execute("select count(*) from documents").fetchone()[0],
                "chunks": connection.execute("select count(*) from chunks").fetchone()[0],
                "instruction_sources": connection.execute("select count(*) from instruction_sources").fetchone()[0],
                "persona_facts": connection.execute("select count(*) from persona_facts").fetchone()[0],
                "mission_summaries": connection.execute("select count(*) from mission_summaries").fetchone()[0],
                "decisions": connection.execute("select count(*) from decisions").fetchone()[0],
                "decision_edges": connection.execute("select count(*) from decision_edges").fetchone()[0],
            },
        }
        if workspace:
            payload["workspace"] = str(_normalize_workspace(workspace))
            payload["workspace_counts"] = {
                "documents": connection.execute("select count(*) from documents where workspace = ?", (payload["workspace"],)).fetchone()[0],
                "mission_summaries": connection.execute("select count(*) from mission_summaries where workspace = ?", (payload["workspace"],)).fetchone()[0],
                "decisions": connection.execute("select count(*) from decisions where workspace = ?", (payload["workspace"],)).fetchone()[0],
            }
        return payload
    finally:
        connection.close()


def enroll_workspace(workspace: str | Path) -> dict[str, Any]:
    workspace_path = str(_normalize_workspace(workspace))
    config = load_memory_config()
    config["workspace_enrollments"] = sorted(set(config["workspace_enrollments"] + [workspace_path]))
    save_memory_config(config)
    return rebuild_workspace_memory(workspace_path, enroll=False)


def _snippet(text: str, limit: int = 220) -> str:
    compact = " ".join(text.split())
    return compact[:limit] + ("..." if len(compact) > limit else "")


def search_memory(query: str, workspace: str | None = None, limit: int | None = None) -> dict[str, Any]:
    config = load_memory_config()
    max_hits = limit or int(config.get("max_chunks_per_query", 8))
    workspace_path = str(_normalize_workspace(workspace)) if workspace else None
    connection = _connect()
    try:
        hits: list[dict[str, Any]] = []
        fts_query = _sanitize_fts_query(query)
        params: list[Any] = [fts_query]
        chunk_sql = (
            "select chunk_id, path, content, bm25(chunk_fts) as rank from chunk_fts where chunk_fts match ?"
        )
        if workspace_path:
            chunk_sql += " and workspace = ?"
            params.append(workspace_path)
        chunk_sql += " order by rank limit ?"
        params.append(max_hits * 3)
        for row in connection.execute(chunk_sql, tuple(params)).fetchall():
            hits.append(
                {
                    "kind": "repo",
                    "path": row["path"],
                    "score": round(2.0 - float(row["rank"]), 4),
                    "snippet": _snippet(row["content"]),
                }
            )

        query_vector = _embed_text(query, int(config.get("vector_dimensions", 96)))
        vector_rows = connection.execute(
            "select chunks.path, chunks.content, embeddings.vector_json from chunks join embeddings on embeddings.chunk_id = chunks.id"
            + (" where chunks.workspace = ?" if workspace_path else ""),
            ((workspace_path,) if workspace_path else ()),
        ).fetchall()
        for row in vector_rows:
            score = _cosine_similarity(query_vector, json.loads(row["vector_json"]))
            if score <= 0:
                continue
            hits.append(
                {
                    "kind": "repo-vector",
                    "path": row["path"],
                    "score": round(score, 4),
                    "snippet": _snippet(row["content"]),
                }
            )

        source_rows = connection.execute(
            "select path, scope, weight, content from instruction_sources order by weight desc"
        ).fetchall()
        for row in source_rows:
            haystack = row["content"].lower()
            if query.lower() in haystack or any(token in haystack for token in _tokenize(query)[:4]):
                hits.append(
                    {
                        "kind": "instruction",
                        "path": row["path"],
                        "score": round(float(row["weight"]) + 1.0, 4),
                        "snippet": _snippet(row["content"]),
                        "scope": row["scope"],
                    }
                )

        summary_rows = connection.execute(
            "select mission_id, title, summary from mission_summaries order by updated_at desc"
            + (" limit 64" if not workspace_path else ""),
        ).fetchall()
        for row in summary_rows:
            text = f"{row['title']} {row['summary']}".lower()
            if query.lower() in text or any(token in text for token in _tokenize(query)[:4]):
                hits.append(
                    {
                        "kind": "mission",
                        "path": row["mission_id"],
                        "score": 1.25,
                        "snippet": _snippet(row["summary"]),
                    }
                )

        decision_rows = connection.execute(
            "select decision_id, title, summary, status from decisions order by updated_at desc limit 128"
        ).fetchall()
        for row in decision_rows:
            text = f"{row['title']} {row['summary']}".lower()
            if query.lower() in text or any(token in text for token in _tokenize(query)[:4]):
                hits.append(
                    {
                        "kind": "decision",
                        "path": row["decision_id"],
                        "score": 1.1,
                        "snippet": _snippet(row["summary"]),
                        "status": row["status"],
                    }
                )

        ranked = sorted(hits, key=lambda item: item["score"], reverse=True)
        deduped: list[dict[str, Any]] = []
        seen: set[tuple[str, str]] = set()
        for hit in ranked:
            key = (hit["kind"], hit["path"])
            if key in seen:
                continue
            seen.add(key)
            deduped.append(hit)
            if len(deduped) >= max_hits:
                break

        return {
            "query": query,
            "workspace": workspace_path,
            "hits": deduped,
        }
    finally:
        connection.close()


def persona_markdown() -> str:
    if persona_path().exists():
        return persona_path().read_text(encoding="utf-8")
    rebuild_workspace_memory(Path.cwd(), enroll=False)
    return persona_path().read_text(encoding="utf-8") if persona_path().exists() else "# Constant Persona\n"


def list_decisions(workspace: str | None = None, mission_id: str | None = None) -> dict[str, Any]:
    connection = _connect()
    try:
        clauses = []
        params: list[Any] = []
        if workspace:
            clauses.append("workspace = ?")
            params.append(str(_normalize_workspace(workspace)))
        if mission_id:
            clauses.append("mission_id = ?")
            params.append(mission_id)
        sql = "select decision_id, mission_id, step_id, workspace, title, summary, status, weight, updated_at from decisions"
        if clauses:
            sql += " where " + " and ".join(clauses)
        sql += " order by updated_at desc, decision_id asc"
        rows = connection.execute(sql, tuple(params)).fetchall()
        return {"decisions": [dict(row) for row in rows]}
    finally:
        connection.close()


def _mission_keywords(mission: dict[str, Any]) -> list[str]:
    tokens = set(_tokenize(mission["title"] + " " + mission["goal"]))
    for step in mission["steps"]:
        tokens.update([step.get("machine", ""), step.get("cli", ""), step.get("backend", ""), step.get("status", "")])
    return sorted(token for token in tokens if token)[:24]


def summarize_mission(mission_id: str) -> dict[str, Any]:
    mission = load_mission(mission_id)
    workspace = str(_normalize_workspace(mission["workspace"]))
    connection = _connect()
    try:
        _upsert_workspace(connection, Path(workspace))
        status_counts = Counter(step.get("status", "unknown") for step in mission["steps"])
        route_bits = []
        for step in mission["steps"]:
            route_bits.append(f"{step['step_id']}={step['machine']}/{step['cli']}/{step['backend']}:{step['status']}")
        summary = (
            f"Mission {mission['title']} ended as {mission['status']}. "
            f"Steps={len(mission['steps'])}. "
            f"Status mix={dict(status_counts)}. "
            f"Routes: {'; '.join(route_bits[:6]) or 'none'}."
        )
        keywords = _mission_keywords(mission)
        connection.execute(
            """
            insert into mission_summaries(mission_id, workspace, title, status, summary, keywords_json, updated_at)
            values (?, ?, ?, ?, ?, ?, ?)
            on conflict(mission_id) do update set
                workspace=excluded.workspace,
                title=excluded.title,
                status=excluded.status,
                summary=excluded.summary,
                keywords_json=excluded.keywords_json,
                updated_at=excluded.updated_at
            """,
            (
                mission_id,
                workspace,
                mission["title"],
                mission["status"],
                summary,
                json.dumps(keywords),
                now_utc(),
            ),
        )
        connection.execute(
            "delete from mission_fts where mission_id = ?",
            (mission_id,),
        )
        connection.execute(
            "insert into mission_fts(mission_id, workspace, title, summary) values (?, ?, ?, ?)",
            (mission_id, workspace, mission["title"], summary),
        )

        previous_decision_id: str | None = None
        decision_count = 0
        for step in mission["steps"]:
            decision_id = f"{mission_id}:{step['step_id']}"
            decision_summary = (
                f"Route {step['machine']}/{step['cli']}/{step['backend']} ended as {step['status']}. "
                f"{step.get('result_summary', '')}".strip()
            )
            connection.execute(
                """
                insert into decisions(decision_id, mission_id, step_id, workspace, title, summary, status, weight, updated_at)
                values (?, ?, ?, ?, ?, ?, ?, ?, ?)
                on conflict(decision_id) do update set
                    mission_id=excluded.mission_id,
                    step_id=excluded.step_id,
                    workspace=excluded.workspace,
                    title=excluded.title,
                    summary=excluded.summary,
                    status=excluded.status,
                    weight=excluded.weight,
                    updated_at=excluded.updated_at
                """,
                (
                    decision_id,
                    mission_id,
                    step["step_id"],
                    workspace,
                    step["title"],
                    decision_summary,
                    step["status"],
                    1.0 if step["status"] == "done" else 0.7,
                    now_utc(),
                ),
            )
            connection.execute("delete from decision_fts where decision_id = ?", (decision_id,))
            connection.execute(
                "insert into decision_fts(decision_id, workspace, title, summary) values (?, ?, ?, ?)",
                (decision_id, workspace, step["title"], decision_summary),
            )
            if previous_decision_id:
                connection.execute(
                    """
                    insert or ignore into decision_edges(from_decision_id, to_decision_id, edge_type, created_at)
                    values (?, ?, ?, ?)
                    """,
                    (previous_decision_id, decision_id, "depends_on", now_utc()),
                )
            previous_decision_id = decision_id
            decision_count += 1

        connection.execute(
            "update workspaces set last_summary_at = ? where path = ?",
            (now_utc(), workspace),
        )
        persona_markdown_value = _render_persona(connection)
        persona_path().parent.mkdir(parents=True, exist_ok=True)
        persona_path().write_text(persona_markdown_value, encoding="utf-8")
        connection.commit()
        append_event(mission_id, "memory.summary_written", {"summary": summary, "decisions": decision_count})
        return {
            "mission_id": mission_id,
            "summary": summary,
            "keywords": keywords,
            "decisions": decision_count,
            "persona_path": str(persona_path()),
        }
    finally:
        connection.close()


def planner_context(workspace: str | Path, query: str) -> dict[str, Any]:
    workspace_path = str(_normalize_workspace(workspace))
    rebuild_workspace_memory(workspace_path, enroll=True)
    search = search_memory(query, workspace_path)
    connection = _connect()
    try:
        instruction_rows = connection.execute(
            """
            select path, scope, weight, content
            from instruction_sources
            where workspace = ? or scope = 'user'
            order by weight desc, updated_at desc
            limit 8
            """,
            (workspace_path,),
        ).fetchall()
        decision_rows = connection.execute(
            """
            select decision_id, title, summary, status
            from decisions
            where workspace = ?
            order by updated_at desc
            limit 6
            """,
            (workspace_path,),
        ).fetchall()
        persona_rows = connection.execute(
            "select fact from persona_facts order by weight desc, updated_at desc limit 12"
        ).fetchall()
        return {
            "workspace": workspace_path,
            "instruction_excerpt": [
                {
                    "path": row["path"],
                    "scope": row["scope"],
                    "weight": row["weight"],
                    "snippet": _snippet(row["content"], 180),
                }
                for row in instruction_rows
            ],
            "repo_hits": search["hits"],
            "recent_decisions": [dict(row) for row in decision_rows],
            "persona_facts": [row["fact"] for row in persona_rows],
        }
    finally:
        connection.close()


def sync_qdrant(workspace: str | None = None) -> dict[str, Any]:
    config = load_memory_config()
    url = str(config.get("qdrant_url", "")).strip()
    if not url:
        return {"ok": False, "skipped": True, "reason": "qdrant_url is not configured"}

    search_payload = search_memory("*", workspace, limit=32)
    collection = config.get("qdrant_collection", "constant_memory")
    request_body = {
        "points": [
            {
                "id": abs(hash((hit["kind"], hit["path"]))) % 2_147_483_647,
                "vector": _embed_text(hit["snippet"], int(config.get("vector_dimensions", 96))),
                "payload": hit,
            }
            for hit in search_payload["hits"]
        ]
    }
    req = urllib.request.Request(
        url.rstrip("/") + f"/collections/{collection}/points?wait=true",
        data=json.dumps(request_body).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="PUT",
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as response:
            payload = json.loads(response.read().decode("utf-8"))
        return {"ok": True, "response": payload, "points": len(request_body["points"])}
    except urllib.error.URLError as exc:
        return {"ok": False, "skipped": False, "reason": str(exc), "points": len(request_body["points"])}
