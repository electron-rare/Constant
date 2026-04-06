#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::config::{MemoryConfig, load_memory_config, save_memory_config};
use crate::paths;
use crate::state::{Mission, MissionStep, now_utc};

const TEXT_SUFFIXES: &[&str] = &[
    ".c", ".cc", ".cfg", ".conf", ".cpp", ".css", ".go", ".h", ".hpp", ".html", ".java", ".js",
    ".json", ".jsx", ".md", ".prompt", ".py", ".rb", ".rs", ".sh", ".sql", ".swift", ".toml",
    ".ts", ".tsx", ".txt", ".yaml", ".yml", ".zsh",
];

const IGNORED_DIR_NAMES: &[&str] = &[
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
];

const INSTRUCTION_DIRS: &[&str] = &[".claude", ".copilot", ".agents", ".agent"];
const INSTRUCTION_FILES: &[&str] = &["CLAUDE.md", "AGENTS.md"];
const INSTRUCTION_SUFFIXES: &[&str] =
    &[".json", ".md", ".prompt", ".toml", ".txt", ".yaml", ".yml"];
const MAX_TEXT_BYTES: u64 = 256 * 1024;
const MAX_INSTRUCTION_BYTES: u64 = 128 * 1024;

const SCHEMA_SQL: &str = r#"
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
    document_id integer not null,
    workspace text not null,
    path text not null,
    chunk_index integer not null,
    content text not null,
    content_hash text not null,
    token_count integer not null,
    updated_at text not null
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

create table if not exists decision_edges (
    id integer primary key,
    from_decision_id text not null,
    to_decision_id text not null,
    edge_type text not null,
    created_at text not null,
    unique(from_decision_id, to_decision_id, edge_type)
);
"#;

pub fn memory_status(workspace: Option<&str>) -> Result<Value, String> {
    ensure_schema()?;
    let config = load_memory_config()?;
    let payload = json!({
        "store_path": paths::memory_store_path().display().to_string(),
        "persona_path": paths::persona_path().display().to_string(),
        "enrollments": config.workspace_enrollments,
        "counts": {
            "workspaces": query_count("select count(*) as count from workspaces")?,
            "documents": query_count("select count(*) as count from documents")?,
            "chunks": query_count("select count(*) as count from chunks")?,
            "instruction_sources": query_count("select count(*) as count from instruction_sources")?,
            "persona_facts": query_count("select count(*) as count from persona_facts")?,
            "mission_summaries": query_count("select count(*) as count from mission_summaries")?,
            "decisions": query_count("select count(*) as count from decisions")?,
            "decision_edges": query_count("select count(*) as count from decision_edges")?,
        }
    });
    if let Some(workspace) = workspace {
        let workspace = normalize_workspace(workspace)?;
        let workspace_str = workspace.display().to_string();
        let workspace_counts = json!({
            "documents": query_count(&format!(
                "select count(*) as count from documents where workspace = {}",
                sql_quote(&workspace_str)
            ))?,
            "mission_summaries": query_count(&format!(
                "select count(*) as count from mission_summaries where workspace = {}",
                sql_quote(&workspace_str)
            ))?,
            "decisions": query_count(&format!(
                "select count(*) as count from decisions where workspace = {}",
                sql_quote(&workspace_str)
            ))?,
        });
        return Ok(json!({
            "store_path": payload["store_path"],
            "persona_path": payload["persona_path"],
            "enrollments": payload["enrollments"],
            "counts": payload["counts"],
            "workspace": workspace_str,
            "workspace_counts": workspace_counts,
        }));
    }
    Ok(payload)
}

pub fn enroll_workspace(workspace: &str) -> Result<Value, String> {
    let workspace = normalize_workspace(workspace)?;
    let mut config = load_memory_config()?;
    let workspace_str = workspace.display().to_string();
    if !config
        .workspace_enrollments
        .iter()
        .any(|entry| entry == &workspace_str)
    {
        config.workspace_enrollments.push(workspace_str.clone());
        config.workspace_enrollments.sort();
        config.workspace_enrollments.dedup();
        save_memory_config(&config)?;
    }
    rebuild_workspace_memory(&workspace_str, false)
}

pub fn rebuild_workspace_memory(workspace: &str, enroll: bool) -> Result<Value, String> {
    ensure_schema()?;
    let workspace = normalize_workspace(workspace)?;
    let workspace_str = workspace.display().to_string();
    let repo_root = detect_repo_root(&workspace);
    let repo_root_str = repo_root.display().to_string();
    let mut config = load_memory_config()?;
    if enroll
        && !config
            .workspace_enrollments
            .iter()
            .any(|entry| entry == &workspace_str)
    {
        config.workspace_enrollments.push(workspace_str.clone());
        config.workspace_enrollments.sort();
        config.workspace_enrollments.dedup();
        save_memory_config(&config)?;
    }

    let files = walk_repo_files(&workspace)?;
    let discovered_instructions = discover_instruction_files(&workspace, &repo_root, &config)?;
    let now = now_utc();

    let mut sql = String::new();
    sql.push_str(SCHEMA_SQL);
    sql.push_str("begin;\n");
    sql.push_str(&format!(
        "insert into workspaces(path, repo_root, enrolled_at, last_indexed_at, last_summary_at) values ({}, {}, {}, {}, coalesce((select last_summary_at from workspaces where path = {}), null)) on conflict(path) do update set repo_root=excluded.repo_root, last_indexed_at=excluded.last_indexed_at;\n",
        sql_quote(&workspace_str),
        sql_quote(&repo_root_str),
        sql_quote(&now),
        sql_quote(&now),
        sql_quote(&workspace_str),
    ));
    sql.push_str(&format!(
        "delete from chunks where workspace = {};\n",
        sql_quote(&workspace_str)
    ));
    sql.push_str(&format!(
        "delete from documents where workspace = {};\n",
        sql_quote(&workspace_str)
    ));
    sql.push_str(&format!(
        "delete from instruction_sources where workspace = {} and scope != 'user';\n",
        sql_quote(&workspace_str)
    ));

    let mut indexed = 0_u64;
    let mut skipped = 0_u64;
    let mut chunk_count = 0_u64;

    for path in files {
        let Some(content) = read_text(&path, MAX_TEXT_BYTES)? else {
            skipped += 1;
            continue;
        };
        if content.trim().is_empty() {
            skipped += 1;
            continue;
        }
        indexed += 1;
        let stat =
            fs::metadata(&path).map_err(|err| format!("cannot stat {}: {err}", path.display()))?;
        let resolved = path.display().to_string();
        let rel_path = relative_path(&path, &workspace);
        let language = language_for(&path);
        let digest = sha256_text(&content);
        sql.push_str(&format!(
            "insert into documents(workspace, path, rel_path, kind, language, source_type, content_hash, mtime, size, weight, updated_at) values ({}, {}, {}, 'repo', {}, 'repo', {}, {}, {}, 1.0, {});\n",
            sql_quote(&workspace_str),
            sql_quote(&resolved),
            sql_quote(&rel_path),
            sql_quote(&language),
            sql_quote(&digest),
            stat.modified().ok().and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok()).map(|v| v.as_secs_f64()).unwrap_or(0.0),
            stat.len(),
            sql_quote(&now),
        ));
        for (idx, chunk) in chunk_text(&content).into_iter().enumerate() {
            let token_count = tokenize(&chunk).len();
            let chunk_hash = sha256_text(&chunk);
            chunk_count += 1;
            sql.push_str(&format!(
                "insert into chunks(document_id, workspace, path, chunk_index, content, content_hash, token_count, updated_at) values ((select id from documents where path = {}), {}, {}, {}, {}, {}, {}, {});\n",
                sql_quote(&resolved),
                sql_quote(&workspace_str),
                sql_quote(&resolved),
                idx + 1,
                sql_quote(&chunk),
                sql_quote(&chunk_hash),
                token_count,
                sql_quote(&now),
            ));
        }
    }

    let mut source_count = 0_u64;
    let mut refreshed = 0_u64;
    let mut persona_facts = 0_u64;
    let discovered_paths = discovered_instructions
        .iter()
        .map(|entry| entry.path.display().to_string())
        .collect::<HashSet<_>>();
    if !discovered_paths.is_empty() {
        sql.push_str("delete from instruction_sources where path in (");
        sql.push_str(
            &discovered_paths
                .iter()
                .map(|path| sql_quote(path))
                .collect::<Vec<_>>()
                .join(", "),
        );
        sql.push_str(");\n");
    }
    for entry in &discovered_instructions {
        source_count += 1;
        refreshed += 1;
        let path = entry.path.display().to_string();
        let digest = sha256_text(&entry.content);
        sql.push_str(&format!(
            "insert into instruction_sources(workspace, path, scope, source_kind, weight, content, content_hash, updated_at) values ({}, {}, {}, {}, {}, {}, {}, {});\n",
            sql_quote(&workspace_str),
            sql_quote(&path),
            sql_quote(entry.scope),
            sql_quote(entry.kind.as_str()),
            entry.weight,
            sql_quote(&entry.content),
            sql_quote(&digest),
            sql_quote(&now),
        ));
        sql.push_str(&format!(
            "delete from persona_facts where source_path = {};\n",
            sql_quote(&path)
        ));
        for fact in extract_persona_facts(&entry.content) {
            persona_facts += 1;
            sql.push_str(&format!(
                "insert into persona_facts(fact, weight, source_path, updated_at) values ({}, {}, {}, {}) on conflict(fact) do update set weight=max(weight, excluded.weight), source_path=excluded.source_path, updated_at=excluded.updated_at;\n",
                sql_quote(&fact),
                entry.weight,
                sql_quote(&path),
                sql_quote(&now),
            ));
        }
    }
    sql.push_str("commit;\n");
    sqlite_exec_script(&sql)?;

    let persona = render_persona()?;
    fs::create_dir_all(
        paths::persona_path()
            .parent()
            .ok_or_else(|| "invalid persona directory".to_string())?,
    )
    .map_err(|err| format!("cannot create persona dir: {err}"))?;
    fs::write(paths::persona_path(), &persona)
        .map_err(|err| format!("cannot write {}: {err}", paths::persona_path().display()))?;

    Ok(json!({
        "workspace": workspace_str,
        "repo_root": repo_root_str,
        "repo": {
            "indexed": indexed,
            "skipped": skipped,
            "pruned": 0,
            "chunks": chunk_count,
        },
        "instructions": {
            "sources": source_count,
            "refreshed": refreshed,
            "removed": 0,
            "persona_facts": persona_facts,
        },
        "persona_path": paths::persona_path().display().to_string(),
        "store_path": paths::memory_store_path().display().to_string(),
    }))
}

pub fn search_memory(
    query: &str,
    workspace: Option<&str>,
    limit: Option<usize>,
) -> Result<Value, String> {
    ensure_schema()?;
    let max_hits = limit.unwrap_or(load_memory_config()?.max_chunks_per_query as usize);
    let workspace = workspace.map(normalize_workspace).transpose()?;
    let workspace_sql = workspace.as_ref().map(|path| path.display().to_string());
    let conditions = like_conditions("content", query);
    let workspace_clause = workspace_sql
        .as_ref()
        .map(|value| format!(" and workspace = {}", sql_quote(value)))
        .unwrap_or_default();
    let mut hits = Vec::new();

    let chunk_rows = sqlite_query_json(&format!(
        "select path, content from chunks where {}{} limit {};",
        conditions,
        workspace_clause,
        max_hits * 3
    ))?;
    for row in chunk_rows {
        let path = row.get("path").and_then(Value::as_str).unwrap_or_default();
        let content = row
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        hits.push(json!({
            "kind": "repo",
            "path": path,
            "score": lexical_score(content, query),
            "snippet": snippet(content, 220),
        }));
    }

    let instruction_rows = sqlite_query_json(&format!(
        "select path, scope, weight, content from instruction_sources where {} order by weight desc limit {};",
        like_conditions("content", query),
        max_hits * 2
    ))?;
    for row in instruction_rows {
        let path = row.get("path").and_then(Value::as_str).unwrap_or_default();
        let content = row
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let weight = row.get("weight").and_then(Value::as_f64).unwrap_or(0.0);
        hits.push(json!({
            "kind": "instruction",
            "path": path,
            "score": lexical_score(content, query) + weight + 1.0,
            "snippet": snippet(content, 220),
            "scope": row.get("scope").cloned().unwrap_or(Value::Null),
        }));
    }

    let mission_clause = workspace_sql
        .as_ref()
        .map(|value| format!(" and workspace = {}", sql_quote(value)))
        .unwrap_or_default();
    let mission_rows = sqlite_query_json(&format!(
        "select mission_id, title, summary from mission_summaries where ({}){} order by updated_at desc limit {};",
        like_conditions("title || ' ' || summary", query),
        mission_clause,
        max_hits * 2
    ))?;
    for row in mission_rows {
        let mission_id = row
            .get("mission_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let summary = row
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default();
        hits.push(json!({
            "kind": "mission",
            "path": mission_id,
            "score": lexical_score(summary, query) + 1.25,
            "snippet": snippet(summary, 220),
        }));
    }

    let decision_rows = sqlite_query_json(&format!(
        "select decision_id, summary, status from decisions where ({}){} order by updated_at desc limit {};",
        like_conditions("title || ' ' || summary", query),
        mission_clause,
        max_hits * 2
    ))?;
    for row in decision_rows {
        let decision_id = row
            .get("decision_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let summary = row
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default();
        hits.push(json!({
            "kind": "decision",
            "path": decision_id,
            "score": lexical_score(summary, query) + 1.1,
            "snippet": snippet(summary, 220),
            "status": row.get("status").cloned().unwrap_or(Value::Null),
        }));
    }

    hits.sort_by(|a, b| {
        b.get("score")
            .and_then(Value::as_f64)
            .partial_cmp(&a.get("score").and_then(Value::as_f64))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for hit in hits {
        let key = format!(
            "{}:{}",
            hit.get("kind").and_then(Value::as_str).unwrap_or(""),
            hit.get("path").and_then(Value::as_str).unwrap_or("")
        );
        if seen.insert(key) {
            deduped.push(hit);
            if deduped.len() >= max_hits {
                break;
            }
        }
    }

    Ok(json!({
        "query": query,
        "workspace": workspace_sql,
        "hits": deduped,
    }))
}

#[allow(dead_code)]
pub fn instruction_skill_sources(
    workspace: &str,
    query: Option<&str>,
    limit: usize,
) -> Result<Value, String> {
    ensure_schema()?;
    let workspace = normalize_workspace(workspace)?;
    let workspace_str = workspace.display().to_string();
    let rows = sqlite_query_json(&format!(
        "select path, scope, source_kind, weight, content, updated_at from instruction_sources where workspace = {} or scope = 'user' order by weight desc, updated_at desc limit 64;",
        sql_quote(&workspace_str)
    ))?;
    let query_tokens = query.map(tokenize).unwrap_or_default();
    let query_lower = query.unwrap_or("").to_lowercase();
    let mut hits = Vec::new();
    for row in rows {
        let path = row.get("path").and_then(Value::as_str).unwrap_or_default();
        let content = row
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut score = row.get("weight").and_then(Value::as_f64).unwrap_or(0.0);
        if let Some(_) = query {
            let haystack = content.to_lowercase();
            let path_lower = path.to_lowercase();
            let matched = haystack.contains(&query_lower)
                || query_tokens
                    .iter()
                    .any(|token| haystack.contains(token) || path_lower.contains(token));
            if !matched {
                continue;
            }
            score += 1.0;
        }
        hits.push(json!({
            "path": path,
            "scope": row.get("scope").cloned().unwrap_or(Value::Null),
            "source_kind": row.get("source_kind").cloned().unwrap_or(Value::Null),
            "weight": score,
            "snippet": snippet(content, 160),
        }));
    }
    hits.sort_by(|a, b| {
        b.get("weight")
            .and_then(Value::as_f64)
            .partial_cmp(&a.get("weight").and_then(Value::as_f64))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(json!(hits.into_iter().take(limit).collect::<Vec<_>>()))
}

pub fn persona_markdown() -> Result<String, String> {
    if paths::persona_path().exists() {
        return fs::read_to_string(paths::persona_path())
            .map_err(|err| format!("cannot read {}: {err}", paths::persona_path().display()));
    }
    render_persona()
}

pub fn list_decisions(workspace: Option<&str>, mission_id: Option<&str>) -> Result<Value, String> {
    ensure_schema()?;
    let mut clauses = Vec::new();
    if let Some(workspace) = workspace {
        clauses.push(format!(
            "workspace = {}",
            sql_quote(&normalize_workspace(workspace)?.display().to_string())
        ));
    }
    if let Some(mission_id) = mission_id {
        clauses.push(format!("mission_id = {}", sql_quote(mission_id)));
    }
    let mut sql = "select decision_id, mission_id, step_id, workspace, title, summary, status, weight, updated_at from decisions".to_string();
    if !clauses.is_empty() {
        sql.push_str(" where ");
        sql.push_str(&clauses.join(" and "));
    }
    sql.push_str(" order by updated_at desc, decision_id asc;");
    Ok(json!({ "decisions": sqlite_query_json(&sql)? }))
}

pub fn summarize_mission_to_memory(mission: &Mission) -> Result<Value, String> {
    ensure_schema()?;
    let workspace = normalize_workspace(&mission.workspace)?;
    let workspace_str = workspace.display().to_string();
    let repo_root = detect_repo_root(&workspace).display().to_string();
    let status_mix = status_mix(mission);
    let status_mix_text = status_mix
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ");
    let routes = mission
        .steps
        .iter()
        .map(|step| {
            format!(
                "{}={}/{}/{}:{}",
                step.step_id, step.machine, step.cli, step.backend, step.status
            )
        })
        .collect::<Vec<_>>();
    let summary = format!(
        "Mission {} ended as {}. Steps={}. Status mix={}. Routes: {}.",
        mission.title,
        mission.status,
        mission.steps.len(),
        status_mix_text,
        if routes.is_empty() {
            "none".to_string()
        } else {
            routes.join("; ")
        }
    );
    let keywords = mission_keywords(mission);
    let now = now_utc();

    let mut sql = String::new();
    sql.push_str(SCHEMA_SQL);
    sql.push_str("begin;\n");
    sql.push_str(&format!(
        "insert into workspaces(path, repo_root, enrolled_at, last_indexed_at, last_summary_at) values ({}, {}, {}, coalesce((select last_indexed_at from workspaces where path = {}), null), {}) on conflict(path) do update set repo_root=excluded.repo_root, last_summary_at=excluded.last_summary_at;\n",
        sql_quote(&workspace_str),
        sql_quote(&repo_root),
        sql_quote(&now),
        sql_quote(&workspace_str),
        sql_quote(&now),
    ));
    sql.push_str(&format!(
        "insert into mission_summaries(mission_id, workspace, title, status, summary, keywords_json, updated_at) values ({}, {}, {}, {}, {}, {}, {}) on conflict(mission_id) do update set workspace=excluded.workspace, title=excluded.title, status=excluded.status, summary=excluded.summary, keywords_json=excluded.keywords_json, updated_at=excluded.updated_at;\n",
        sql_quote(&mission.mission_id),
        sql_quote(&workspace_str),
        sql_quote(&mission.title),
        sql_quote(&mission.status),
        sql_quote(&summary),
        sql_quote(&serde_json::to_string(&keywords).map_err(|err| format!("cannot encode keywords: {err}"))?),
        sql_quote(&now),
    ));
    let mut previous_decision_id: Option<String> = None;
    let mut decision_count = 0_u64;
    for step in &mission.steps {
        let decision_id = format!("{}:{}", mission.mission_id, step.step_id);
        let title = step_title(step);
        let decision_summary = format!(
            "Route {}/{}/{} ended as {}. {}",
            step.machine, step.cli, step.backend, step.status, step.result_summary
        )
        .trim()
        .to_string();
        sql.push_str(&format!(
            "insert into decisions(decision_id, mission_id, step_id, workspace, title, summary, status, weight, updated_at) values ({}, {}, {}, {}, {}, {}, {}, {}, {}) on conflict(decision_id) do update set mission_id=excluded.mission_id, step_id=excluded.step_id, workspace=excluded.workspace, title=excluded.title, summary=excluded.summary, status=excluded.status, weight=excluded.weight, updated_at=excluded.updated_at;\n",
            sql_quote(&decision_id),
            sql_quote(&mission.mission_id),
            sql_quote(&step.step_id),
            sql_quote(&workspace_str),
            sql_quote(&title),
            sql_quote(&decision_summary),
            sql_quote(&step.status),
            if step.status == "done" { 1.0 } else { 0.7 },
            sql_quote(&now),
        ));
        if let Some(previous) = previous_decision_id.as_deref() {
            sql.push_str(&format!(
                "insert or ignore into decision_edges(from_decision_id, to_decision_id, edge_type, created_at) values ({}, {}, 'depends_on', {});\n",
                sql_quote(previous),
                sql_quote(&decision_id),
                sql_quote(&now),
            ));
        }
        previous_decision_id = Some(decision_id);
        decision_count += 1;
    }
    sql.push_str("commit;\n");
    sqlite_exec_script(&sql)?;

    let persona = render_persona()?;
    fs::create_dir_all(
        paths::persona_path()
            .parent()
            .ok_or_else(|| "invalid persona directory".to_string())?,
    )
    .map_err(|err| format!("cannot create persona dir: {err}"))?;
    fs::write(paths::persona_path(), persona)
        .map_err(|err| format!("cannot write {}: {err}", paths::persona_path().display()))?;

    Ok(json!({
        "mission_id": mission.mission_id,
        "summary": summary,
        "keywords": keywords,
        "decisions": decision_count,
        "persona_path": paths::persona_path().display().to_string(),
    }))
}

fn ensure_schema() -> Result<(), String> {
    let db = paths::memory_store_path();
    if let Some(parent) = db.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }
    sqlite_exec_script(SCHEMA_SQL)
}

fn sqlite_exec_script(sql: &str) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!(
        "constant-memory-{}-{}.sql",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default()
    ));
    fs::write(&tmp, sql).map_err(|err| format!("cannot write {}: {err}", tmp.display()))?;
    let input =
        fs::File::open(&tmp).map_err(|err| format!("cannot open {}: {err}", tmp.display()))?;
    let output = Command::new("sqlite3")
        .arg(paths::memory_store_path())
        .stdin(Stdio::from(input))
        .output()
        .map_err(|err| format!("cannot start sqlite3: {err}"))?;
    let _ = fs::remove_file(&tmp);
    if !output.status.success() {
        return Err(format!(
            "sqlite3 failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn sqlite_query_json(sql: &str) -> Result<Vec<Value>, String> {
    let output = Command::new("sqlite3")
        .arg("-json")
        .arg(paths::memory_store_path())
        .arg(sql)
        .output()
        .map_err(|err| format!("cannot query sqlite3: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "sqlite3 query failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&stdout).map_err(|err| format!("cannot parse sqlite json: {err}"))
}

fn query_count(sql: &str) -> Result<u64, String> {
    let rows = sqlite_query_json(sql)?;
    Ok(rows
        .first()
        .and_then(|row| row.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(0))
}

fn normalize_workspace(path: &str) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(path).expand_home();
    candidate
        .canonicalize()
        .or_else(|_| Ok::<PathBuf, std::io::Error>(candidate))
        .map_err(|err| format!("cannot resolve workspace {path}: {err}"))
}

fn detect_repo_root(workspace: &Path) -> PathBuf {
    let mut current = Some(workspace);
    while let Some(path) = current {
        if path.join(".git").exists() {
            return path.to_path_buf();
        }
        current = path.parent();
    }
    workspace.to_path_buf()
}

fn walk_repo_files(workspace: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    walk_dir_recursive(workspace, &mut files)?;
    files.sort();
    Ok(files)
}

fn walk_dir_recursive(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in
        fs::read_dir(root).map_err(|err| format!("cannot read {}: {err}", root.display()))?
    {
        let entry = entry.map_err(|err| format!("cannot iterate {}: {err}", root.display()))?;
        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if IGNORED_DIR_NAMES.contains(&file_name) {
                continue;
            }
            walk_dir_recursive(&path, files)?;
            continue;
        }
        if should_index_repo_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn should_index_repo_file(path: &Path) -> bool {
    let file_name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or_default();
    if file_name.starts_with('.') && !matches!(file_name, ".gitignore" | ".env.example") {
        return INSTRUCTION_FILES.contains(&file_name);
    }
    let suffix = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default();
    let suffix = if suffix.is_empty() {
        String::new()
    } else {
        format!(".{suffix}")
    };
    if TEXT_SUFFIXES.iter().any(|candidate| *candidate == suffix) {
        return true;
    }
    matches!(file_name, "Dockerfile" | "Makefile" | "justfile")
}

fn read_text(path: &Path, max_bytes: u64) -> Result<Option<String>, String> {
    let metadata =
        fs::metadata(path).map_err(|err| format!("cannot stat {}: {err}", path.display()))?;
    if metadata.len() > max_bytes {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    if bytes.contains(&0) {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&bytes).to_string();
    Ok(Some(text))
}

#[derive(Clone)]
struct InstructionSource {
    path: PathBuf,
    scope: &'static str,
    weight: f64,
    kind: String,
    content: String,
}

fn discover_instruction_files(
    workspace: &Path,
    repo_root: &Path,
    config: &MemoryConfig,
) -> Result<Vec<InstructionSource>, String> {
    let mut seen = HashSet::new();
    let mut bases = vec![workspace.to_path_buf(), repo_root.to_path_buf()];
    for parent in repo_root.ancestors().skip(1) {
        bases.push(parent.to_path_buf());
    }
    if let Some(home) = paths::home_dir() {
        if !bases.iter().any(|entry| entry == &home) {
            bases.push(home);
        }
    }

    let home = paths::home_dir();
    let mut entries = Vec::new();
    for base in bases {
        if let Some(home) = &home {
            if !base.starts_with(home) && base != *workspace && base != *repo_root {
                continue;
            }
        }
        for candidate in instruction_candidates(&base)? {
            let resolved = candidate
                .canonicalize()
                .unwrap_or_else(|_| candidate.clone());
            let key = resolved.display().to_string();
            if !seen.insert(key.clone()) {
                continue;
            }
            let Some(content) = read_text(&resolved, MAX_INSTRUCTION_BYTES)? else {
                continue;
            };
            if content.trim().is_empty() {
                continue;
            }
            let (scope, weight) = instruction_scope(&resolved, workspace, repo_root, config);
            let kind = resolved
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|value| value.to_str())
                .unwrap_or_else(|| {
                    resolved
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("instruction")
                })
                .to_string();
            entries.push(InstructionSource {
                path: resolved,
                scope,
                weight,
                kind,
                content,
            });
        }
    }
    entries.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(entries)
}

fn instruction_candidates(base: &Path) -> Result<Vec<PathBuf>, String> {
    let mut candidates = Vec::new();
    for name in INSTRUCTION_FILES {
        let path = base.join(name);
        if path.exists() {
            candidates.push(path);
        }
    }
    for dirname in INSTRUCTION_DIRS {
        let directory = base.join(dirname);
        if !directory.is_dir() {
            continue;
        }
        walk_instruction_dir(&directory, &directory, &mut candidates)?;
    }
    Ok(candidates)
}

fn walk_instruction_dir(root: &Path, current: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let depth = current
        .strip_prefix(root)
        .ok()
        .map(|value| value.components().count())
        .unwrap_or(0);
    if depth > 3 {
        return Ok(());
    }
    for entry in
        fs::read_dir(current).map_err(|err| format!("cannot read {}: {err}", current.display()))?
    {
        let entry = entry.map_err(|err| format!("cannot iterate {}: {err}", current.display()))?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if IGNORED_DIR_NAMES.contains(&name) {
                continue;
            }
            walk_instruction_dir(root, &path, out)?;
        } else {
            let suffix = path
                .extension()
                .and_then(|v| v.to_str())
                .unwrap_or_default();
            let suffix = if suffix.is_empty() {
                String::new()
            } else {
                format!(".{suffix}")
            };
            if INSTRUCTION_SUFFIXES
                .iter()
                .any(|candidate| *candidate == suffix)
                || matches!(name, "config.json" | "settings.json")
            {
                out.push(path);
            }
        }
    }
    Ok(())
}

fn instruction_scope(
    path: &Path,
    workspace: &Path,
    repo_root: &Path,
    config: &MemoryConfig,
) -> (&'static str, f64) {
    let weights = &config.instruction_weights;
    let home = paths::home_dir();
    if path == workspace || path.starts_with(workspace) {
        return ("workspace", *weights.get("workspace").unwrap_or(&1.0));
    }
    if path == repo_root || path.starts_with(repo_root) {
        return ("repo", *weights.get("repo").unwrap_or(&0.85));
    }
    if let Some(home) = home {
        if path.parent() == Some(home.as_path()) || path.starts_with(&home) {
            return ("user", *weights.get("user").unwrap_or(&0.45));
        }
    }
    ("ancestor", *weights.get("ancestor").unwrap_or(&0.65))
}

fn render_persona() -> Result<String, String> {
    ensure_schema()?;
    let facts = sqlite_query_json(
        "select fact, source_path from persona_facts order by weight desc, fact asc limit 32;",
    )?;
    let missions = sqlite_query_json(
        "select mission_id, title, summary from mission_summaries order by updated_at desc limit 6;",
    )?;
    let decisions = sqlite_query_json(
        "select decision_id, title, summary from decisions order by updated_at desc limit 8;",
    )?;
    let mut lines = vec![
        "# Constant Persona".to_string(),
        String::new(),
        "## Durable Rules".to_string(),
    ];
    if facts.is_empty() {
        lines.push("- No durable rules extracted yet.".to_string());
    } else {
        for row in facts {
            let fact = row.get("fact").and_then(Value::as_str).unwrap_or_default();
            let source_name = row
                .get("source_path")
                .and_then(Value::as_str)
                .map(Path::new)
                .and_then(|path| path.file_name())
                .and_then(|value| value.to_str())
                .unwrap_or("source");
            lines.push(format!("- {fact} ({source_name})"));
        }
    }
    lines.push(String::new());
    lines.push("## Recent Mission Summaries".to_string());
    if missions.is_empty() {
        lines.push("- No mission summaries yet.".to_string());
    } else {
        for row in missions {
            lines.push(format!(
                "- `{}` {}: {}",
                row.get("mission_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                row.get("title").and_then(Value::as_str).unwrap_or_default(),
                row.get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            ));
        }
    }
    lines.push(String::new());
    lines.push("## Decision Graph Snapshot".to_string());
    if decisions.is_empty() {
        lines.push("- No decisions captured yet.".to_string());
    } else {
        for row in decisions {
            lines.push(format!(
                "- `{}` {}: {}",
                row.get("decision_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                row.get("title").and_then(Value::as_str).unwrap_or_default(),
                row.get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            ));
        }
    }
    Ok(lines.join("\n") + "\n")
}

fn extract_persona_facts(text: &str) -> Vec<String> {
    let mut facts = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with("```")
            || line.starts_with('{')
            || line.starts_with('}')
            || line.starts_with('[')
            || line.starts_with(']')
        {
            continue;
        }
        if !(12..=220).contains(&line.len()) {
            continue;
        }
        let cleaned = line
            .trim_start_matches("- ")
            .trim_start_matches("* ")
            .trim()
            .to_string();
        if !cleaned.is_empty() && !facts.contains(&cleaned) {
            facts.push(cleaned);
        }
        if facts.len() >= 24 {
            break;
        }
    }
    facts
}

fn mission_keywords(mission: &Mission) -> Vec<String> {
    let mut keywords = BTreeSet::new();
    for token in tokenize(&format!("{} {}", mission.title, mission.goal)) {
        keywords.insert(token);
    }
    for step in &mission.steps {
        for token in [
            step.machine.as_str(),
            step.cli.as_str(),
            step.backend.as_str(),
            step.status.as_str(),
        ] {
            if !token.is_empty() {
                keywords.insert(token.to_lowercase());
            }
        }
    }
    keywords.into_iter().take(24).collect()
}

fn status_mix(mission: &Mission) -> BTreeMap<String, u64> {
    let mut map = BTreeMap::new();
    for step in &mission.steps {
        *map.entry(step.status.clone()).or_insert(0) += 1;
    }
    map
}

fn step_title(step: &MissionStep) -> String {
    step.extra
        .get("title")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| step.step_id.clone())
}

fn sha256_text(text: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '/' | ':' | '-') {
            current.push(ch.to_ascii_lowercase());
        } else {
            if current.len() >= 2 {
                tokens.push(current.clone());
            }
            current.clear();
        }
    }
    if current.len() >= 2 {
        tokens.push(current);
    }
    tokens
}

fn lexical_score(text: &str, query: &str) -> f64 {
    let haystack = text.to_lowercase();
    tokenize(query)
        .into_iter()
        .filter(|token| haystack.contains(token))
        .count() as f64
}

fn chunk_text(text: &str) -> Vec<String> {
    let lines = text.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < lines.len() {
        let mut total = 0usize;
        let mut end = start;
        while end < lines.len() && total < 1200 {
            total += lines[end].len() + 1;
            end += 1;
        }
        let chunk = lines[start..end].join("\n").trim().to_string();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }
        if end >= lines.len() {
            break;
        }
        start = end.saturating_sub(3).max(start + 1);
    }
    chunks
}

fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn language_for(path: &Path) -> String {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
    {
        "py" => "python",
        "sh" | "zsh" => "shell",
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "md" => "markdown",
        "toml" => "toml",
        "rs" => "rust",
        "go" => "go",
        "swift" => "swift",
        "html" => "html",
        "css" => "css",
        other if !other.is_empty() => other,
        _ => "text",
    }
    .to_string()
}

fn snippet(text: &str, limit: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() > limit {
        format!("{}...", &compact[..limit])
    } else {
        compact
    }
}

fn like_conditions(column: &str, query: &str) -> String {
    let tokens = tokenize(query);
    if tokens.is_empty() {
        return format!("{column} like '%constant%'");
    }
    tokens
        .into_iter()
        .take(4)
        .map(|token| format!("{column} like {}", sql_quote(&format!("%{token}%"))))
        .collect::<Vec<_>>()
        .join(" or ")
}

fn sql_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

trait ExpandHomePath {
    fn expand_home(self) -> PathBuf;
}

impl ExpandHomePath for PathBuf {
    fn expand_home(self) -> PathBuf {
        let value = self.to_string_lossy().to_string();
        if value == "~" {
            return paths::home_dir().unwrap_or_else(|| PathBuf::from(value));
        }
        if let Some(rest) = value.strip_prefix("~/") {
            return paths::home_dir()
                .map(|home| home.join(rest))
                .unwrap_or_else(|| PathBuf::from(value));
        }
        if let Some(rest) = value.strip_prefix("$HOME/") {
            return paths::home_dir()
                .map(|home| home.join(rest))
                .unwrap_or_else(|| PathBuf::from(value));
        }
        PathBuf::from(value)
    }
}
