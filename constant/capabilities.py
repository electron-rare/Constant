from __future__ import annotations

from typing import Any


DEFAULT_AGENTS: list[dict[str, Any]] = [
    {
        "id": "planner",
        "label": "Planner",
        "role": "planner",
        "primary_cli": "claude",
        "capabilities": ["spec", "planning", "review", "summarization", "delegation"],
        "preferred_layers": ["reflection", "execution-prep"],
        "manual_only": False,
    },
    {
        "id": "executor",
        "label": "Executor",
        "role": "executor",
        "primary_cli": "codex",
        "capabilities": ["implementation", "debugging", "refactor", "tooling", "ops"],
        "preferred_layers": ["execution"],
        "manual_only": False,
    },
    {
        "id": "analyst",
        "label": "Analyst",
        "role": "analyst",
        "primary_cli": "vibe",
        "capabilities": ["brainstorm", "alternatives", "research", "comparison"],
        "preferred_layers": ["reflection"],
        "manual_only": False,
    },
    {
        "id": "assistant",
        "label": "Assistant",
        "role": "assistant",
        "primary_cli": "copilot",
        "capabilities": ["interactive-help", "inline-suggestions"],
        "preferred_layers": ["manual"],
        "manual_only": True,
    },
]


DEFAULT_SKILLS: list[dict[str, Any]] = [
    {
        "id": "spec-planner",
        "label": "Spec Planner",
        "layer": "reflection",
        "visibility": "public",
        "summary": "Transformer une demande floue en spec exploitable avant de coder.",
        "when_to_use": [
            "quand on part d'une idee brute",
            "quand il faut cadrer avant implementation",
            "quand on veut eviter de coder trop tot",
        ],
        "input_examples": [
            "Je veux ajouter un systeme de notifications",
            "On doit refondre l'auth OAuth",
        ],
        "output_expected": [
            "objectif",
            "perimetre",
            "hors_perimetre",
            "contraintes",
            "hypotheses",
            "criteres_acceptation",
            "risques",
            "questions_ouvertes",
            "plan_minimal",
        ],
        "usage_prompt": (
            "Fais une spec executable a partir de cette demande. "
            "Identifie les zones floues, propose des hypotheses raisonnables, "
            "liste les criteres d'acceptation et termine par un plan d'implementation minimal."
        ),
        "keywords": [
            "spec planner",
            "spec-planner",
            "spec",
            "cadrer",
            "clarifier",
            "clarify",
            "requirements",
            "scope",
            "perimetre",
            "oauth",
            "notifications",
            "idea brute",
            "je veux",
            "i want",
            "on doit",
            "we need",
            "refondre",
            "before coding",
        ],
        "aliases": ["summarize", "summary", "brief"],
        "preferred_cli": "claude",
        "preferred_agent": "planner",
    },
    {
        "id": "architecture-brainstorm",
        "label": "Architecture Brainstorm",
        "layer": "reflection",
        "visibility": "public",
        "summary": "Comparer plusieurs options d'architecture avant de choisir.",
        "when_to_use": [
            "quand plusieurs approches sont possibles",
            "quand on veut comparer les trade-offs",
            "quand il faut une vraie discussion d'architecture",
        ],
        "input_examples": [
            "Doit-on faire un backend event-driven ou un monolithe modulaire ?",
            "Quel runtime pour le cockpit interactif ?",
        ],
        "output_expected": [
            "options",
            "avantages",
            "inconvenients",
            "complexite",
            "impacts_perf",
            "impacts_securite",
            "impacts_dx",
            "recommendation",
            "raisons_du_choix",
        ],
        "usage_prompt": (
            "Analyse ce probleme comme un review partner senior. "
            "Propose plusieurs architectures possibles, compare les compromis, "
            "signale les pieges, puis recommande une option avec justification."
        ),
        "keywords": [
            "architecture",
            "brainstorm",
            "architecture-brainstorm",
            "trade-off",
            "tradeoff",
            "option",
            "compare",
            "alternatives",
            "design choice",
            "choix technique",
        ],
        "aliases": ["brainstorm"],
        "preferred_cli": "vibe",
        "preferred_agent": "analyst",
    },
    {
        "id": "repo-onboarding",
        "label": "Repo Onboarding",
        "layer": "execution-prep",
        "visibility": "public",
        "summary": "Comprendre rapidement un repo avant la premiere intervention.",
        "when_to_use": [
            "quand on arrive sur un nouveau projet",
            "quand il faut cartographier une codebase",
            "quand on veut savoir ou modifier sans casser le reste",
        ],
        "input_examples": [
            "Explore ce repo comme si tu preparais une premiere intervention",
            "Dis-moi ou intervenir pour cette tache",
        ],
        "output_expected": [
            "structure_repo",
            "points_entree",
            "modules_cles",
            "conventions",
            "commandes_utiles",
            "zones_a_risque",
            "strategie_modification",
        ],
        "usage_prompt": (
            "Explore ce repo comme si tu preparais une premiere intervention. "
            "Resume sa structure, les modules critiques, les conventions implicites, "
            "les commandes de dev/test, puis dis ou intervenir pour cette tache."
        ),
        "keywords": [
            "repo onboarding",
            "repo-onboarding",
            "onboarding",
            "explore repo",
            "new repo",
            "codebase",
            "structure du repo",
            "points d'entree",
            "where to change",
            "cartographier",
        ],
        "aliases": [],
        "preferred_cli": "claude",
        "preferred_agent": "planner",
    },
    {
        "id": "task-decomposer",
        "label": "Task Decomposer",
        "layer": "execution-prep",
        "visibility": "public",
        "summary": "Transformer une spec ou direction choisie en plan d'execution operationnel.",
        "when_to_use": [
            "quand la direction est choisie",
            "quand on veut passer du brainstorming a l'action",
            "quand il faut un ordre d'execution clair",
        ],
        "input_examples": [
            "Decoupe cette refonte en sous-taches concretement executables",
            "Fais-moi un plan d'implementation directement actionnable",
        ],
        "output_expected": [
            "etapes_ordonnees",
            "dependances",
            "quick_wins",
            "parallelisation",
            "tests",
            "definition_of_done",
        ],
        "usage_prompt": (
            "Decoupe cette tache en sous-taches concretes, ordonnees, avec dependances, "
            "risques, validations et tests associes. "
            "Le plan doit etre directement executable par un agent de code."
        ),
        "keywords": [
            "task decomposer",
            "task-decomposer",
            "decompose",
            "plan d'execution",
            "execution plan",
            "break down",
            "subtasks",
            "steps",
            "roadmap",
            "implementation plan",
        ],
        "aliases": [],
        "preferred_cli": "claude",
        "preferred_agent": "planner",
    },
    {
        "id": "pr-review-prep",
        "label": "PR Review Prep",
        "layer": "execution-prep",
        "visibility": "public",
        "summary": "Preparer une PR review-ready et anticiper les objections du reviewer.",
        "when_to_use": [
            "quand l'implementation est terminee",
            "quand il faut une bonne PR",
            "quand on veut preparer la review",
        ],
        "input_examples": [
            "Prepare une PR clean a partir de ces changements",
            "Resume le pourquoi, le quoi et les risques pour le reviewer",
        ],
        "output_expected": [
            "resume_du_changement",
            "impact_fonctionnel",
            "impact_technique",
            "migrations",
            "tests",
            "points_attention_reviewer",
            "message_de_pr",
        ],
        "usage_prompt": (
            "Prepare une PR review-ready a partir de ces changements. "
            "Resume le pourquoi, le quoi, les impacts, les risques, "
            "les tests effectues et les points a surveiller pour le reviewer."
        ),
        "keywords": [
            "pr review prep",
            "pr-review-prep",
            "pr",
            "pull request",
            "review prep",
            "release notes",
            "review-ready",
            "reviewer",
        ],
        "aliases": ["review"],
        "preferred_cli": "claude",
        "preferred_agent": "planner",
    },
    {
        "id": "implementation",
        "label": "Implementation",
        "layer": "execution",
        "visibility": "internal",
        "summary": "Ship code changes and concrete fixes.",
        "when_to_use": [
            "quand il faut modifier du code maintenant",
            "quand la spec est deja suffisamment claire",
        ],
        "input_examples": ["Fix the flaky test", "Implement the API endpoint"],
        "output_expected": ["code_changes", "validation", "tests"],
        "usage_prompt": "Implement the requested change directly and validate it.",
        "keywords": ["fix", "implement", "build", "write", "patch", "feature", "code"],
        "aliases": [],
        "preferred_cli": "codex",
        "preferred_agent": "executor",
    },
    {
        "id": "debug-restoration",
        "label": "Debug Restoration",
        "layer": "execution",
        "visibility": "internal",
        "summary": "Investigate failures and restore a working state.",
        "when_to_use": [
            "quand quelque chose casse",
            "quand il faut comprendre puis restaurer un etat sain",
        ],
        "input_examples": ["The pane dies instantly", "This build started failing today"],
        "output_expected": ["root_cause", "fix", "validation"],
        "usage_prompt": "Investigate the failure, isolate the cause, and restore a working state.",
        "keywords": ["bug", "debug", "failure", "broken", "error", "crash", "repair"],
        "aliases": ["debug"],
        "preferred_cli": "codex",
        "preferred_agent": "executor",
    },
    {
        "id": "ops-deployment",
        "label": "Ops Deployment",
        "layer": "execution",
        "visibility": "internal",
        "summary": "Handle shell, fleet, infra, network, and remote machine work.",
        "when_to_use": [
            "quand il faut deployer, scanner, installer, diagnostiquer des machines",
            "quand la tache est principalement shell/infra",
        ],
        "input_examples": ["Deploy the runtime on all machines", "Scan SSH hosts and configure the fleet"],
        "output_expected": ["ops_actions", "machine_state", "validation"],
        "usage_prompt": "Handle the shell, infra, fleet, and remote machine operations safely.",
        "keywords": ["ssh", "shell", "fleet", "ops", "network", "infra", "deploy"],
        "aliases": ["ops"],
        "preferred_cli": "codex",
        "preferred_agent": "executor",
    },
]


def _normalize_skill_id(value: str) -> str:
    return value.strip().lower()


def _skill_entries(include_internal: bool = True) -> list[dict[str, Any]]:
    if include_internal:
        return DEFAULT_SKILLS
    return [skill for skill in DEFAULT_SKILLS if skill.get("visibility") != "internal"]


def list_agents() -> list[dict[str, Any]]:
    return [dict(item) for item in DEFAULT_AGENTS]


def list_skills(include_internal: bool = True) -> list[dict[str, Any]]:
    return [dict(item) for item in _skill_entries(include_internal=include_internal)]


def skill_catalog(include_internal: bool = False) -> dict[str, list[dict[str, Any]]]:
    payload = {"reflection": [], "execution-prep": [], "execution": [], "manual": []}
    for skill in _skill_entries(include_internal=include_internal):
        payload.setdefault(str(skill["layer"]), []).append(dict(skill))
    return payload


def skill_catalog_brief(include_internal: bool = False) -> list[dict[str, Any]]:
    return [
        {
            "id": skill["id"],
            "layer": skill["layer"],
            "summary": skill["summary"],
            "preferred_cli": skill["preferred_cli"],
            "preferred_agent": skill["preferred_agent"],
        }
        for skill in _skill_entries(include_internal=include_internal)
    ]


def recommended_skill_stack() -> dict[str, Any]:
    return {
        "minimal": ["spec-planner", "architecture-brainstorm", "task-decomposer"],
        "workflow": [
            "spec-planner",
            "architecture-brainstorm",
            "repo-onboarding",
            "task-decomposer",
            "pr-review-prep",
        ],
        "layers": {
            "reflection": ["spec-planner", "architecture-brainstorm"],
            "execution": ["repo-onboarding", "task-decomposer", "pr-review-prep"],
        },
    }


def agent_by_id(agent_id: str) -> dict[str, Any]:
    for agent in DEFAULT_AGENTS:
        if agent["id"] == agent_id:
            return dict(agent)
    raise KeyError(f"Unknown agent: {agent_id}")


def agent_for_cli(cli: str) -> dict[str, Any]:
    for agent in DEFAULT_AGENTS:
        if agent["primary_cli"] == cli:
            return dict(agent)
    return agent_by_id("executor")


def agent_hint(agent_id: str) -> dict[str, Any]:
    return agent_by_id(agent_id)


def skill_by_id(skill_id: str, include_internal: bool = True) -> dict[str, Any]:
    normalized = _normalize_skill_id(skill_id)
    for skill in _skill_entries(include_internal=include_internal):
        aliases = [_normalize_skill_id(alias) for alias in skill.get("aliases", [])]
        if normalized == skill["id"] or normalized in aliases:
            return dict(skill)
    raise KeyError(f"Unknown skill: {skill_id}")


def _score_keywords(goal_l: str, skill: dict[str, Any]) -> int:
    score = 0
    for keyword in skill.get("keywords", []):
        if keyword in goal_l:
            score += 3 if " " in keyword or "-" in keyword else 1
    if skill["id"] in goal_l:
        score += 5
    for alias in skill.get("aliases", []):
        if alias in goal_l:
            score += 3
    return score


def match_skill(goal: str, include_internal: bool = True) -> dict[str, Any]:
    goal_l = goal.lower()
    best = skill_by_id("implementation", include_internal=include_internal) if include_internal else skill_by_id("spec-planner", include_internal=False)
    best_score = -1
    for skill in _skill_entries(include_internal=include_internal):
        score = _score_keywords(goal_l, skill)
        if score > best_score:
            best_score = score
            best = skill
    if best_score <= 0:
        if any(token in goal_l for token in ("repo", "codebase", "onboarding", "where to change", "first intervention")):
            return skill_by_id("repo-onboarding", include_internal=include_internal)
        if any(token in goal_l for token in ("je veux", "i want", "on doit", "we need", "idea brute", "high level idea")):
            return skill_by_id("spec-planner", include_internal=include_internal)
        if any(token in goal_l for token in ("architecture", "trade-off", "tradeoff", "option", "compare")):
            return skill_by_id("architecture-brainstorm", include_internal=include_internal)
        if any(token in goal_l for token in ("plan", "sub-task", "subtask", "decompose", "step-by-step", "roadmap")):
            return skill_by_id("task-decomposer", include_internal=include_internal)
        if any(token in goal_l for token in ("pr", "pull request", "reviewer", "release notes")):
            return skill_by_id("pr-review-prep", include_internal=include_internal)
        if include_internal and any(token in goal_l for token in ("bug", "crash", "failure", "broken", "error")):
            return skill_by_id("debug-restoration", include_internal=True)
        if include_internal and any(token in goal_l for token in ("ssh", "fleet", "deploy", "network", "machine", "infra")):
            return skill_by_id("ops-deployment", include_internal=True)
        return best
    return dict(best)


def resolve_skill_and_agent(
    *,
    goal: str | None = None,
    skill_id: str | None = None,
    agent_id: str | None = None,
    cli: str | None = None,
) -> dict[str, Any]:
    skill = skill_by_id(skill_id) if skill_id else match_skill(goal or "")
    if cli:
        agent = agent_for_cli(cli)
    elif agent_id:
        agent = agent_by_id(agent_id)
        cli = str(agent["primary_cli"])
    else:
        agent = agent_by_id(str(skill["preferred_agent"]))
        cli = str(skill["preferred_cli"])

    return {
        "skill": skill,
        "agent": agent,
        "cli": cli,
    }
