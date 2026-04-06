use serde::Serialize;

pub const VERSION: &str = "0.1.0";

#[derive(Clone, Copy, Serialize)]
pub struct Agent {
    pub id: &'static str,
    pub label: &'static str,
    pub role: &'static str,
    pub primary_cli: &'static str,
    pub manual_only: bool,
    pub capabilities: &'static [&'static str],
    pub preferred_layers: &'static [&'static str],
}

#[derive(Clone, Copy, Serialize)]
pub struct Skill {
    pub id: &'static str,
    pub label: &'static str,
    pub layer: &'static str,
    pub visibility: &'static str,
    pub summary: &'static str,
    pub when_to_use: &'static [&'static str],
    pub input_examples: &'static [&'static str],
    pub output_expected: &'static [&'static str],
    pub usage_prompt: &'static str,
    pub keywords: &'static [&'static str],
    pub aliases: &'static [&'static str],
    pub preferred_cli: &'static str,
    pub preferred_agent: &'static str,
}

pub const AGENTS: &[Agent] = &[
    Agent {
        id: "planner",
        label: "Planner",
        role: "planner",
        primary_cli: "claude",
        manual_only: false,
        capabilities: &["spec", "planning", "review", "summarization", "delegation"],
        preferred_layers: &["reflection", "execution-prep"],
    },
    Agent {
        id: "executor",
        label: "Executor",
        role: "executor",
        primary_cli: "codex",
        manual_only: false,
        capabilities: &["implementation", "debugging", "refactor", "tooling", "ops"],
        preferred_layers: &["execution"],
    },
    Agent {
        id: "analyst",
        label: "Analyst",
        role: "analyst",
        primary_cli: "vibe",
        manual_only: false,
        capabilities: &["brainstorm", "alternatives", "research", "comparison"],
        preferred_layers: &["reflection"],
    },
    Agent {
        id: "assistant",
        label: "Assistant",
        role: "assistant",
        primary_cli: "copilot",
        manual_only: true,
        capabilities: &["interactive-help", "inline-suggestions"],
        preferred_layers: &["manual"],
    },
];

pub const SKILLS: &[Skill] = &[
    Skill {
        id: "spec-planner",
        label: "Spec Planner",
        layer: "reflection",
        visibility: "public",
        summary: "Transformer une demande floue en spec exploitable avant de coder.",
        when_to_use: &[
            "quand on part d'une idee brute",
            "quand il faut cadrer avant implementation",
            "quand on veut eviter de coder trop tot",
        ],
        input_examples: &[
            "Je veux ajouter un systeme de notifications",
            "On doit refondre l'auth OAuth",
        ],
        output_expected: &[
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
        usage_prompt: "Fais une spec executable a partir de cette demande. Identifie les zones floues, propose des hypotheses raisonnables, liste les criteres d'acceptation et termine par un plan d'implementation minimal.",
        keywords: &[
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
        aliases: &["summarize", "summary", "brief"],
        preferred_cli: "claude",
        preferred_agent: "planner",
    },
    Skill {
        id: "architecture-brainstorm",
        label: "Architecture Brainstorm",
        layer: "reflection",
        visibility: "public",
        summary: "Comparer plusieurs options d'architecture avant de choisir.",
        when_to_use: &[
            "quand plusieurs approches sont possibles",
            "quand on veut comparer les trade-offs",
            "quand il faut une vraie discussion d'architecture",
        ],
        input_examples: &[
            "Doit-on faire un backend event-driven ou un monolithe modulaire ?",
            "Quel runtime pour le cockpit interactif ?",
        ],
        output_expected: &[
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
        usage_prompt: "Analyse ce probleme comme un review partner senior. Propose plusieurs architectures possibles, compare les compromis, signale les pieges, puis recommande une option avec justification.",
        keywords: &[
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
        aliases: &["brainstorm"],
        preferred_cli: "vibe",
        preferred_agent: "analyst",
    },
    Skill {
        id: "repo-onboarding",
        label: "Repo Onboarding",
        layer: "execution-prep",
        visibility: "public",
        summary: "Comprendre rapidement un repo avant la premiere intervention.",
        when_to_use: &[
            "quand on arrive sur un nouveau projet",
            "quand il faut cartographier une codebase",
            "quand on veut savoir ou modifier sans casser le reste",
        ],
        input_examples: &[
            "Explore ce repo comme si tu preparais une premiere intervention",
            "Dis-moi ou intervenir pour cette tache",
        ],
        output_expected: &[
            "structure_repo",
            "points_entree",
            "modules_cles",
            "conventions",
            "commandes_utiles",
            "zones_a_risque",
            "strategie_modification",
        ],
        usage_prompt: "Explore ce repo comme si tu preparais une premiere intervention. Resume sa structure, les modules critiques, les conventions implicites, les commandes de dev/test, puis dis ou intervenir pour cette tache.",
        keywords: &[
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
        aliases: &[],
        preferred_cli: "claude",
        preferred_agent: "planner",
    },
    Skill {
        id: "task-decomposer",
        label: "Task Decomposer",
        layer: "execution-prep",
        visibility: "public",
        summary: "Transformer une spec ou direction choisie en plan d'execution operationnel.",
        when_to_use: &[
            "quand la direction est choisie",
            "quand on veut passer du brainstorming a l'action",
            "quand il faut un ordre d'execution clair",
        ],
        input_examples: &[
            "Decoupe cette refonte en sous-taches concretement executables",
            "Fais-moi un plan d'implementation directement actionnable",
        ],
        output_expected: &[
            "etapes_ordonnees",
            "dependances",
            "quick_wins",
            "parallelisation",
            "tests",
            "definition_of_done",
        ],
        usage_prompt: "Decoupe cette tache en sous-taches concretes, ordonnees, avec dependances, risques, validations et tests associes. Le plan doit etre directement executable par un agent de code.",
        keywords: &[
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
        aliases: &[],
        preferred_cli: "claude",
        preferred_agent: "planner",
    },
    Skill {
        id: "pr-review-prep",
        label: "PR Review Prep",
        layer: "execution-prep",
        visibility: "public",
        summary: "Preparer une PR review-ready et anticiper les objections du reviewer.",
        when_to_use: &[
            "quand l'implementation est terminee",
            "quand il faut une bonne PR",
            "quand on veut preparer la review",
        ],
        input_examples: &[
            "Prepare une PR clean a partir de ces changements",
            "Resume le pourquoi, le quoi et les risques pour le reviewer",
        ],
        output_expected: &[
            "resume_du_changement",
            "impact_fonctionnel",
            "impact_technique",
            "migrations",
            "tests",
            "points_attention_reviewer",
            "message_de_pr",
        ],
        usage_prompt: "Prepare une PR review-ready a partir de ces changements. Resume le pourquoi, le quoi, les impacts, les risques, les tests effectues et les points a surveiller pour le reviewer.",
        keywords: &[
            "pr review prep",
            "pr-review-prep",
            "pr",
            "pull request",
            "review prep",
            "release notes",
            "review-ready",
            "reviewer",
        ],
        aliases: &["review"],
        preferred_cli: "claude",
        preferred_agent: "planner",
    },
    Skill {
        id: "implementation",
        label: "Implementation",
        layer: "execution",
        visibility: "internal",
        summary: "Ship code changes and concrete fixes.",
        when_to_use: &[
            "quand il faut modifier du code maintenant",
            "quand la spec est deja suffisamment claire",
        ],
        input_examples: &["Fix the flaky test", "Implement the API endpoint"],
        output_expected: &["code_changes", "validation", "tests"],
        usage_prompt: "Implement the requested change directly and validate it.",
        keywords: &[
            "fix",
            "implement",
            "build",
            "write",
            "patch",
            "feature",
            "code",
        ],
        aliases: &[],
        preferred_cli: "codex",
        preferred_agent: "executor",
    },
    Skill {
        id: "debug-restoration",
        label: "Debug Restoration",
        layer: "execution",
        visibility: "internal",
        summary: "Investigate failures and restore a working state.",
        when_to_use: &[
            "quand quelque chose casse",
            "quand il faut comprendre puis restaurer un etat sain",
        ],
        input_examples: &[
            "The pane dies instantly",
            "This build started failing today",
        ],
        output_expected: &["root_cause", "fix", "validation"],
        usage_prompt: "Investigate the failure, isolate the cause, and restore a working state.",
        keywords: &[
            "bug", "debug", "failure", "broken", "error", "crash", "repair",
        ],
        aliases: &["debug"],
        preferred_cli: "codex",
        preferred_agent: "executor",
    },
    Skill {
        id: "ops-deployment",
        label: "Ops Deployment",
        layer: "execution",
        visibility: "internal",
        summary: "Handle shell, fleet, infra, network, and remote machine work.",
        when_to_use: &[
            "quand il faut deployer, scanner, installer, diagnostiquer des machines",
            "quand la tache est principalement shell/infra",
        ],
        input_examples: &[
            "Deploy the runtime on all machines",
            "Scan SSH hosts and configure the fleet",
        ],
        output_expected: &["ops_actions", "machine_state", "validation"],
        usage_prompt: "Handle the shell, infra, fleet, and remote machine operations safely.",
        keywords: &["ssh", "shell", "fleet", "ops", "network", "infra", "deploy"],
        aliases: &["ops"],
        preferred_cli: "codex",
        preferred_agent: "executor",
    },
];

#[derive(Clone, Copy)]
pub struct SkillResolution {
    pub skill: &'static Skill,
    pub agent: &'static Agent,
    pub cli: &'static str,
}

pub fn agent_by_id(agent_id: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|agent| agent.id == agent_id)
}

pub fn agent_for_cli(cli: &str) -> &'static Agent {
    AGENTS
        .iter()
        .find(|agent| agent.primary_cli == cli)
        .or_else(|| agent_by_id("executor"))
        .expect("executor agent must exist")
}

pub fn skill_by_id(skill_id: &str, include_internal: bool) -> Option<&'static Skill> {
    let normalized = normalize_skill_id(skill_id);
    SKILLS.iter().find(|skill| {
        (include_internal || skill.visibility != "internal")
            && (normalized == skill.id
                || skill
                    .aliases
                    .iter()
                    .any(|alias| normalized == normalize_skill_id(alias)))
    })
}

pub fn resolve_skill_and_agent(
    goal: Option<&str>,
    skill_id: Option<&str>,
    agent_id: Option<&str>,
    cli: Option<&str>,
) -> SkillResolution {
    let skill = skill_id
        .and_then(|value| skill_by_id(value, true))
        .unwrap_or_else(|| match_skill(goal.unwrap_or(""), true));

    if let Some(selected_cli) = cli {
        let agent = agent_for_cli(selected_cli);
        return SkillResolution {
            skill,
            agent,
            cli: agent.primary_cli,
        };
    }

    if let Some(selected_agent) = agent_id {
        if let Some(agent) = agent_by_id(selected_agent) {
            return SkillResolution {
                skill,
                agent,
                cli: agent.primary_cli,
            };
        }
    }

    let agent = agent_by_id(skill.preferred_agent).expect("preferred agent must exist");
    SkillResolution {
        skill,
        agent,
        cli: skill.preferred_cli,
    }
}

pub fn match_skill(goal: &str, include_internal: bool) -> &'static Skill {
    let goal_l = goal.to_lowercase();
    let mut best = if include_internal {
        skill_by_id("implementation", true).expect("implementation skill must exist")
    } else {
        skill_by_id("spec-planner", false).expect("spec-planner must exist")
    };
    let mut best_score = 0_i32;

    for skill in SKILLS
        .iter()
        .filter(|skill| include_internal || skill.visibility != "internal")
    {
        let score = score_keywords(&goal_l, skill);
        if score > best_score {
            best_score = score;
            best = skill;
        }
    }

    if best_score == 0 {
        if contains_any(
            &goal_l,
            &[
                "repo",
                "codebase",
                "onboarding",
                "where to change",
                "first intervention",
            ],
        ) {
            return skill_by_id("repo-onboarding", include_internal)
                .expect("repo-onboarding must exist");
        }
        if contains_any(
            &goal_l,
            &[
                "je veux",
                "i want",
                "on doit",
                "we need",
                "idea brute",
                "high level idea",
            ],
        ) {
            return skill_by_id("spec-planner", include_internal).expect("spec-planner must exist");
        }
        if contains_any(
            &goal_l,
            &["architecture", "trade-off", "tradeoff", "option", "compare"],
        ) {
            return skill_by_id("architecture-brainstorm", include_internal)
                .expect("architecture-brainstorm must exist");
        }
        if contains_any(
            &goal_l,
            &[
                "plan",
                "sub-task",
                "subtask",
                "decompose",
                "step-by-step",
                "roadmap",
            ],
        ) {
            return skill_by_id("task-decomposer", include_internal)
                .expect("task-decomposer must exist");
        }
        if contains_any(
            &goal_l,
            &["pr", "pull request", "reviewer", "release notes"],
        ) {
            return skill_by_id("pr-review-prep", include_internal)
                .expect("pr-review-prep must exist");
        }
        if include_internal
            && contains_any(&goal_l, &["bug", "crash", "failure", "broken", "error"])
        {
            return skill_by_id("debug-restoration", true).expect("debug-restoration must exist");
        }
        if include_internal
            && contains_any(
                &goal_l,
                &["ssh", "fleet", "deploy", "network", "machine", "infra"],
            )
        {
            return skill_by_id("ops-deployment", true).expect("ops-deployment must exist");
        }
    }

    best
}

fn normalize_skill_id(value: &str) -> String {
    value.trim().to_lowercase()
}

fn score_keywords(goal: &str, skill: &Skill) -> i32 {
    let mut score = 0_i32;
    for keyword in skill.keywords {
        if goal.contains(keyword) {
            score += if keyword.contains(' ') || keyword.contains('-') {
                3
            } else {
                1
            };
        }
    }
    if goal.contains(skill.id) {
        score += 5;
    }
    for alias in skill.aliases {
        if goal.contains(alias) {
            score += 3;
        }
    }
    score
}

fn contains_any(goal: &str, tokens: &[&str]) -> bool {
    tokens.iter().any(|token| goal.contains(token))
}

pub const MINIMAL_STACK: &[&str] = &["spec-planner", "architecture-brainstorm", "task-decomposer"];

pub const WORKFLOW_STACK: &[&str] = &[
    "spec-planner",
    "architecture-brainstorm",
    "repo-onboarding",
    "task-decomposer",
    "pr-review-prep",
];
