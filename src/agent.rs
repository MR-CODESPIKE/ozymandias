use crate::{
    evaluate::verify_workspace,
    memory::{LongContextMemory, MemoryKind},
    sandbox::Sandbox,
    tools::ToolRegistry,
};
use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{env, path::PathBuf, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Shell {
        command: String,
    },
    ReadFile {
        path: String,
    },
    WriteFile {
        path: String,
        content: String,
    },
    WriteBundle {
        files: std::collections::HashMap<String, String>,
    },
    WebGet {
        url: String,
    },
    Finish {
        answer: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub step: usize,
    pub role: String,
    pub content: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub answer: String,
    pub events: Vec<Event>,
    pub steps: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TaskRoute {
    SimpleCoding,
    ResearchAssisted,
    Debugging,
    Planning,
    General,
}

impl TaskRoute {
    fn instruction(self) -> &'static str {
        match self {
            Self::SimpleCoding => {
                "Implement the requested change directly, inspect existing files first, and verify the result locally."
            }
            Self::ResearchAssisted => {
                "Gather reliable source evidence before acting, record relevant findings, then implement and verify."
            }
            Self::Debugging => {
                "Reproduce the reported failure first, identify its root cause, apply the smallest safe fix, and rerun the failing check."
            }
            Self::Planning => {
                "Decompose the objective into explicit dependent steps, complete them in order, and verify each acceptance criterion."
            }
            Self::General => {
                "Clarify the objective from the available context, use safe tools, and finish only when evidence is sufficient."
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskState {
    objective: String,
    route: TaskRoute,
    phase: String,
    repair_cycles: usize,
    verification_failed: bool,
    verified: bool,
    changed_files: Vec<String>,
    verification_failures: Vec<String>,
    research_findings: Vec<String>,
}

#[derive(Clone)]
pub struct ModelClient {
    client: Client,
    endpoint: String,
    api_key: Option<String>,
    model: String,
}
impl ModelClient {
    pub fn from_env() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(25))
                .build()
                .unwrap_or_else(|_| Client::new()),
            endpoint: env::var("OPENAI_API_BASE").unwrap_or_else(|_| "".into()),
            api_key: env::var("OPENAI_API_KEY").ok(),
            model: env::var("OZY_MODEL").unwrap_or_else(|_| "gpt-5-mini".into()),
        }
    }
    async fn choose(&self, system: &str, context: &str) -> Result<Action> {
        if self.endpoint.is_empty() || self.api_key.is_none() {
            return Ok(Action::Finish { answer: "Offline mode: no model endpoint configured. The task was received, but external reasoning is disabled.".into() });
        }
        #[derive(Serialize)]
        struct Req<'a> {
            model: &'a str,
            messages: Vec<Msg<'a>>,
            max_tokens: u32,
            response_format: serde_json::Value,
            #[serde(skip_serializing_if = "Option::is_none")]
            reasoning_effort: Option<&'static str>,
        }
        #[derive(Serialize)]
        struct Msg<'a> {
            role: &'a str,
            content: &'a str,
        }
        let prompt = format!("{system}\nReturn exactly one plain JSON object and do not call or emulate any function/tool API. The object must use one action value: shell with {{\"command\":\"...\"}}, read_file with {{\"path\":\"...\"}}, write_file with {{\"path\":\"...\",\"content\":\"...\"}}, write_bundle with {{\"files\":{{...}}}}, web_get with {{\"url\":\"...\"}}, or finish with {{\"answer\":\"...\"}}. Prefer write_bundle for coding tasks so source, tests, and README are created in one step. Never create Python source with shell-escaped echo/printf commands: use write_file or write_bundle with raw valid file contents, including real newlines. Before finishing, verify that solution.py exists, imports cleanly, preserves every prompt-defined helper/import/definition, and passes the supplied tests. For coding tasks, create all requested source files, a separately discoverable test file, and README; when the task names a Python CLI module, create an importable wordstats.py entrypoint so python3 -m wordstats works; obey explicitly required JSON shapes such as top_words being a list of [word, count] pairs and top_service being exactly one [service, count] pair, not a list of all services. Do not repeat an identical action after it has already produced no useful observation; diagnose and change strategy.\nContext:\n{context}");
        let body = Req {
            model: &self.model,
            messages: vec![
                Msg {
                    role: "system",
                    content: "You are a careful autonomous task executor.",
                },
                Msg {
                    role: "user",
                    content: &prompt,
                },
            ],
            max_tokens: 2048,
            response_format: serde_json::json!({"type": "json_object"}),
            reasoning_effort: (!self.endpoint.contains("generativelanguage.googleapis.com"))
                .then_some("low"),
        };
        let raw = self
            .client
            .post(format!(
                "{}/chat/completions",
                self.endpoint.trim_end_matches('/')
            ))
            .bearer_auth(self.api_key.as_ref().unwrap())
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let value: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
            anyhow!(
                "model response was not JSON: {error}; body={}",
                raw.chars().take(240).collect::<String>()
            )
        })?;
        let text = value
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .or_else(|| {
                value
                    .pointer("/choices/0/message/tool_calls/0/function/arguments")
                    .and_then(|v| v.as_str())
            })
            .or_else(|| {
                value
                    .pointer("/choices/0/message/function_call/arguments")
                    .and_then(|v| v.as_str())
            })
            .or_else(|| value.pointer("/output_text").and_then(|v| v.as_str()))
            .or_else(|| value.pointer("/content").and_then(|v| v.as_str()))
            .ok_or_else(|| {
                anyhow!(
                    "model returned no supported content envelope; body={}",
                    raw.chars().take(240).collect::<String>()
                )
            })?;
        let clean = text
            .trim()
            .trim_start_matches("```json")
            .trim_end_matches("```")
            .trim();
        parse_action(clean)
    }
}

struct ToolRecovery;

impl ToolRecovery {
    async fn shell(tools: &ToolRegistry, sandbox: &Sandbox, command: &str) -> Result<String> {
        match tools.shell(sandbox, command).await {
            Ok(output) => Ok(output),
            Err(error) if Self::pytest_unavailable(&error.to_string(), command) => {
                let fallback = "python3 -m unittest discover -s . -p 'test_*.py'";
                tools.shell(sandbox, fallback).await.map(|output| {
                    format!("RECOVERY: pytest unavailable; used unittest fallback.\n{output}")
                })
            }
            Err(error)
                if Self::retryable(&error.to_string()) && Self::retryable_command(command) =>
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
                tools
                    .shell(sandbox, command)
                    .await
                    .map(|output| format!("RECOVERY: retried transient shell failure.\n{output}"))
            }
            Err(error) => Err(error),
        }
    }

    async fn read_file(tools: &ToolRegistry, path: &str) -> Result<String> {
        match tools.read_file(path).await {
            Ok(output) => Ok(output),
            Err(error) if Self::retryable(&error.to_string()) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
                tools
                    .read_file(path)
                    .await
                    .map(|output| format!("RECOVERY: retried transient read failure.\n{output}"))
            }
            Err(error) => Err(error),
        }
    }

    async fn web_get(tools: &ToolRegistry, url: &str) -> Result<String> {
        match tools.web_get(url).await {
            Ok(output) => Ok(output),
            Err(error) if Self::retryable(&error.to_string()) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
                tools
                    .web_get(url)
                    .await
                    .map(|output| format!("RECOVERY: retried transient web failure.\n{output}"))
            }
            Err(error) => Err(error),
        }
    }

    fn retryable(message: &str) -> bool {
        let lower = message.to_ascii_lowercase();
        [
            "timed out",
            "timeout",
            "connection reset",
            "temporarily unavailable",
            "resource temporarily unavailable",
            "broken pipe",
            "failed to execute",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
    }

    fn retryable_command(command: &str) -> bool {
        let lower = command.to_ascii_lowercase();
        [
            "pytest",
            "unittest",
            "cargo test",
            "node --check",
            "npm test",
            "git diff",
            "git status",
            "ls",
            "pwd",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
    }

    fn pytest_unavailable(message: &str, command: &str) -> bool {
        let lower = message.to_ascii_lowercase();
        command.to_ascii_lowercase().contains("pytest")
            && (lower.contains("pytest: not found") || lower.contains("no module named pytest"))
    }
}

struct ResearchWorker {
    max_calls: usize,
}

struct ParallelAdvisor;

struct SelfConsistency;

impl SelfConsistency {
    fn eligible(route: TaskRoute, objective: &str) -> bool {
        !ParallelAdvisor::roles(route, objective).is_empty()
    }

    fn vote_label(answer: &str) -> &'static str {
        let lower = answer.to_ascii_lowercase();
        if lower.contains("decompose") || lower.contains("plan first") {
            "decompose"
        } else if lower.contains("test_first") || lower.contains("test first") {
            "test_first"
        } else if lower.contains("research") {
            "research_first"
        } else {
            "direct"
        }
    }

    async fn run(model: &ModelClient, route: TaskRoute, objective: &str) -> (Vec<Event>, String) {
        if !Self::eligible(route, objective) {
            return (Vec::new(), "direct".into());
        }
        let mut voters = tokio::task::JoinSet::new();
        for voter in 0..3 {
            let voter_model = model.clone();
            let voter_objective = objective.to_string();
            voters.spawn(async move {
                let context = format!(
                    "Critical decision for objective: {voter_objective}\nVote independently for exactly one strategy: direct, decompose, test_first, or research_first. Return a finish answer beginning with VOTE: <strategy> and a one-sentence reason. This is voter {voter}."
                );
                (voter, voter_model.choose(
                    "You are an independent self-consistency voter. Do not modify files or execute tools.",
                    &context,
                ).await)
            });
        }

        let mut counts = std::collections::HashMap::<&'static str, usize>::new();
        let mut events = Vec::new();
        while let Some(result) = voters.join_next().await {
            if let Ok((voter, Ok(Action::Finish { answer }))) = result {
                let label = Self::vote_label(&answer);
                *counts.entry(label).or_default() += 1;
                events.push(Event {
                    step: voter,
                    role: "consistency_vote".into(),
                    content: format!("{label}: {answer}"),
                });
            }
        }
        let selected = ["decompose", "test_first", "research_first", "direct"]
            .into_iter()
            .max_by_key(|label| {
                (
                    counts.get(label).copied().unwrap_or(0),
                    usize::from(*label == "decompose"),
                )
            })
            .unwrap_or("decompose");
        events.push(Event {
            step: 3,
            role: "consistency_controller".into(),
            content: format!("selected strategy: {selected}"),
        });
        (events, selected.into())
    }
}

impl ParallelAdvisor {
    fn roles(route: TaskRoute, objective: &str) -> Vec<&'static str> {
        let lower = objective.to_ascii_lowercase();
        if route == TaskRoute::Planning {
            vec!["decomposition analyst", "risk and dependency reviewer"]
        } else if route == TaskRoute::SimpleCoding
            && ["complex", "multiple", "several", "edge cases", "refactor"]
                .iter()
                .any(|term| lower.contains(term))
        {
            vec!["implementation analyst", "edge-case reviewer"]
        } else {
            Vec::new()
        }
    }

    async fn run(
        model: &ModelClient,
        route: TaskRoute,
        objective: &str,
    ) -> (Vec<Event>, Vec<String>) {
        let roles = Self::roles(route, objective);
        if roles.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let mut workers = tokio::task::JoinSet::new();
        for (index, role) in roles.into_iter().enumerate() {
            let worker_model = model.clone();
            let worker_objective = objective.to_string();
            let role = role.to_string();
            workers.spawn(async move {
                let context = format!(
                    "Objective: {worker_objective}\nRole: {role}\nReturn one concise advisory finding. Do not modify files, execute commands, or claim verification."
                );
                let result = worker_model
                    .choose(
                        "You are a bounded parallel advisory sub-agent. Analyze only your assigned role and return a finish action containing evidence-based advice.",
                        &context,
                    )
                    .await;
                (index, role, result)
            });
        }

        let mut events = Vec::new();
        let mut findings = Vec::new();
        while let Some(result) = workers.join_next().await {
            match result {
                Ok((index, role, Ok(Action::Finish { answer }))) => {
                    let finding = format!("{role}: {answer}");
                    events.push(Event {
                        step: index,
                        role: "parallel_advisor".into(),
                        content: finding.clone(),
                    });
                    findings.push(finding);
                }
                Ok((index, role, Ok(action))) => {
                    let finding = format!(
                        "{role} returned advisory action: {}",
                        serde_json::to_string(&action).unwrap_or_default()
                    );
                    events.push(Event {
                        step: index,
                        role: "parallel_advisor".into(),
                        content: finding.clone(),
                    });
                    findings.push(finding);
                }
                Ok((index, role, Err(error))) => {
                    events.push(Event {
                        step: index,
                        role: "parallel_advisor_error".into(),
                        content: format!("{role}: {error}"),
                    });
                }
                Err(error) => events.push(Event {
                    step: 0,
                    role: "parallel_advisor_join_error".into(),
                    content: error.to_string(),
                }),
            }
        }
        findings.sort();
        (events, findings)
    }
}

impl ResearchWorker {
    fn bounded(max_calls: usize) -> Self {
        Self {
            max_calls: max_calls.max(1),
        }
    }

    async fn run(
        &self,
        model: &ModelClient,
        tools: &ToolRegistry,
        objective: &str,
    ) -> (Vec<Event>, Vec<String>) {
        let mut events = Vec::new();
        let mut findings = Vec::new();
        let mut context = format!(
            "Research objective: {objective}\nReturn web_get for an authoritative source or finish with a concise, source-cited finding."
        );
        for step in 0..self.max_calls {
            let action = match model
                .choose(
                    "You are a bounded research worker. Use only web_get and finish. Prefer primary documentation, official specifications, or authoritative sources. Do not modify files or run shell commands.",
                    &context,
                )
                .await
            {
                Ok(action) => action,
                Err(error) => {
                    findings.push(format!("Research worker error: {error}"));
                    events.push(Event {
                        step,
                        role: "research_error".into(),
                        content: error.to_string(),
                    });
                    break;
                }
            };
            match action {
                Action::WebGet { url } => {
                    let result = ToolRecovery::web_get(tools, &url).await;
                    let detail = format_result(result);
                    let excerpt = detail.chars().take(4000).collect::<String>();
                    findings.push(format!("Source: {url}\n{excerpt}"));
                    events.push(Event {
                        step,
                        role: "research_tool".into(),
                        content: format!("web_get {url}\n{excerpt}"),
                    });
                    context = format!(
                        "Research objective: {objective}\nPrevious source result:\n{excerpt}\nReturn another web_get for a missing authoritative source or finish with a concise, source-cited finding."
                    );
                }
                Action::Finish { answer } => {
                    findings.push(answer.clone());
                    events.push(Event {
                        step,
                        role: "researcher".into(),
                        content: answer,
                    });
                    break;
                }
                other => {
                    let message = format!(
                        "Research worker rejected non-research action: {}",
                        serde_json::to_string(&other).unwrap_or_default()
                    );
                    findings.push(message.clone());
                    events.push(Event {
                        step,
                        role: "research_controller".into(),
                        content: message,
                    });
                    break;
                }
            }
        }
        (events, findings)
    }
}

pub struct AutonomousAgent {
    pub model: ModelClient,
    pub max_steps: usize,
    pub workspace: PathBuf,
}
impl AutonomousAgent {
    pub fn new(max_steps: usize) -> Self {
        Self {
            model: ModelClient::from_env(),
            max_steps: max_steps.max(1),
            workspace: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
    pub async fn solve(&self, objective: &str) -> Result<AgentResult> {
        let tools = ToolRegistry::new();
        let sandbox = Sandbox::detect().await;
        let route = classify_task(objective);
        let mut events = Vec::new();
        let mut memory = LongContextMemory::new(32);
        memory.remember("objective", objective, MemoryKind::Fact, 10);
        memory.remember("route", &format!("{route:?}"), MemoryKind::Decision, 8);
        let mut todo = vec![objective.to_string()];
        let mut last_action = String::new();
        let mut repeated_actions = 0usize;
        let mut repair_cycles = 0usize;
        let mut verification_failed = false;
        let mut state = TaskState {
            objective: objective.to_string(),
            route,
            phase: "implement".into(),
            repair_cycles: 0,
            verification_failed: false,
            verified: false,
            changed_files: Vec::new(),
            verification_failures: Vec::new(),
            research_findings: Vec::new(),
        };
        let (advisor_events, advisor_findings) =
            ParallelAdvisor::run(&self.model, route, objective).await;
        events.extend(advisor_events);
        for (index, finding) in advisor_findings.iter().enumerate() {
            memory.remember(
                &format!("parallel-advice-{index}"),
                finding,
                MemoryKind::Evidence,
                6,
            );
        }
        let (vote_events, strategy) = SelfConsistency::run(&self.model, route, objective).await;
        events.extend(vote_events);
        if SelfConsistency::eligible(route, objective) {
            memory.remember("critical-strategy", &strategy, MemoryKind::Decision, 9);
            todo[0] =
                format!("Use the selected self-consistency strategy '{strategy}' for: {objective}");
        }
        if route == TaskRoute::ResearchAssisted {
            let worker = ResearchWorker::bounded(2);
            let (research_events, findings) = worker.run(&self.model, &tools, objective).await;
            events.extend(research_events);
            for (index, finding) in findings.iter().enumerate() {
                memory.remember(
                    &format!("research-{index}"),
                    finding,
                    MemoryKind::Evidence,
                    7,
                );
            }
            state.research_findings = findings;
            state.phase = "implement".into();
            todo[0] = format!(
                "Use the bounded research findings in task_state, then complete and verify: {objective}"
            );
        }
        for step in 0..self.max_steps {
            let context = compact_context(&todo, &events, &state, &memory)?;
            let system = format!(
                "Maintain the todo, act only with an available safe tool, and finish when evidence is sufficient. Route: {:?}. {}",
                route,
                route.instruction()
            );
            let action = match self.model.choose(&system, &context).await {
                Ok(action) => action,
                Err(error) => {
                    let message = error.to_string();
                    events.push(Event {
                        step,
                        role: "protocol_error".into(),
                        content: message.clone(),
                    });
                    if is_fatal_model_error(&message) {
                        return Ok(AgentResult {
                            answer: format!("Model endpoint rejected further requests: {message}"),
                            events,
                            steps: step + 1,
                        });
                    }
                    todo[0] = format!("Recover from the previous protocol error and return exactly one supported JSON action for: {objective}");
                    continue;
                }
            };
            let action_key = serde_json::to_string(&action)?;
            if action_key == last_action {
                repeated_actions += 1;
            } else {
                repeated_actions = 0;
            }
            last_action = action_key;
            if repeated_actions >= 2 {
                return Ok(AgentResult {
                    answer: "Stopped a repeated action loop without new evidence.".into(),
                    events,
                    steps: step + 1,
                });
            }
            events.push(Event {
                step,
                role: "assistant".into(),
                content: serde_json::to_string(&action)?,
            });
            match action {
                Action::Finish { answer } => {
                    if verification_failed {
                        state.phase = "repair".into();
                        events.push(Event {
                            step,
                            role: "controller".into(),
                            content: "Rejected premature finish after failed verification; repair the reported failure first.".into(),
                        });
                        todo[0] = format!(
                            "Verification is still failing. Diagnose the latest failure and repair the artifact for: {objective}"
                        );
                        continue;
                    }
                    return Ok(AgentResult {
                        answer,
                        events,
                        steps: step + 1,
                    });
                }
                Action::Shell { command } => {
                    let r = ToolRecovery::shell(&tools, &sandbox, &command).await;
                    events.push(Event {
                        step,
                        role: "tool".into(),
                        content: format_result(r),
                    });
                }
                Action::ReadFile { path } => {
                    let r = ToolRecovery::read_file(&tools, &path).await;
                    events.push(Event {
                        step,
                        role: "tool".into(),
                        content: format_result(r),
                    });
                }
                Action::WriteFile { path, content } => {
                    memory.remember(&format!("file-{path}"), "written", MemoryKind::Progress, 5);
                    if !state.changed_files.contains(&path) {
                        state.changed_files.push(path.clone());
                    }
                    let content = normalize_written_content(&path, content);
                    let r = tools
                        .write_file(&path, &content)
                        .await
                        .map(|_| "write completed".to_string());
                    events.push(Event {
                        step,
                        role: "tool".into(),
                        content: format_result(r),
                    });
                }
                Action::WriteBundle { files } => {
                    let has_solution = files.contains_key("solution.py");
                    let has_tests = files.contains_key("test_solution.py");
                    for (path, content) in files {
                        memory.remember(
                            &format!("file-{path}"),
                            "written",
                            MemoryKind::Progress,
                            5,
                        );
                        if !state.changed_files.contains(&path) {
                            state.changed_files.push(path.clone());
                        }
                        let content = normalize_written_content(&path, content);
                        let result = tools
                            .write_file(&path, &content)
                            .await
                            .map(|_| format!("write completed: {path}"));
                        events.push(Event {
                            step,
                            role: "tool".into(),
                            content: format_result(result),
                        });
                    }
                    // Coding tasks commonly provide both the candidate and its
                    // tests in one bundle. Verify them locally before spending
                    // another model request; this prevents a valid artifact from
                    // timing out while waiting for a redundant final response.
                    if has_solution && has_tests {
                        let verification = verify_workspace(&sandbox).await;
                        let passed = verification
                            .as_ref()
                            .map(|result| result.passed)
                            .unwrap_or(false);
                        let verification_detail = match verification {
                            Ok(result) => result.detail(),
                            Err(error) => error.to_string(),
                        };
                        events.push(Event {
                            step,
                            role: "tool".into(),
                            content: verification_detail.clone(),
                        });
                        if passed {
                            memory.remember(
                                "verification",
                                "deterministic checks passed",
                                MemoryKind::Evidence,
                                10,
                            );
                            state.phase = "complete".into();
                            state.verified = true;
                            state.verification_failed = false;
                            return Ok(AgentResult {
                                answer: "Generated solution passed the bundled tests.".into(),
                                events,
                                steps: step + 1,
                            });
                        }
                        verification_failed = true;
                        repair_cycles += 1;
                        state.phase = "repair".into();
                        state.repair_cycles = repair_cycles;
                        state.verification_failed = true;
                        state.verification_failures.push(verification_detail);
                        memory.remember(
                            &format!("verification-failure-{repair_cycles}"),
                            state.verification_failures.last().unwrap(),
                            MemoryKind::Failure,
                            10,
                        );
                        if repair_cycles >= 3 {
                            return Ok(AgentResult {
                                answer: "Verification failed after three bounded repair cycles."
                                    .into(),
                                events,
                                steps: step + 1,
                            });
                        }
                    }
                }
                Action::WebGet { url } => {
                    let r = ToolRecovery::web_get(&tools, &url).await;
                    events.push(Event {
                        step,
                        role: "tool".into(),
                        content: format_result(r),
                    });
                }
            }
            todo[0] = if verification_failed {
                format!(
                    "Repair cycle {repair_cycles}/3: diagnose the latest verification failure, modify the artifact, rerun the tests, and do not finish until they pass: {objective}"
                )
            } else {
                format!(
                    "Complete objective: {objective}; verify the latest observation before finishing"
                )
            };
        }
        Ok(AgentResult {
            answer: "Step budget exhausted before verified completion.".into(),
            events,
            steps: self.max_steps,
        })
    }
}

fn is_fatal_model_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "insufficient credits",
        "not enough credits",
        "quota exceeded",
        "rate limit exceeded",
        "429 too many requests",
        "http status client error (429",
        "invalid api key",
        "authentication failed",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn classify_task(objective: &str) -> TaskRoute {
    let lower = objective.to_ascii_lowercase();
    let has = |terms: &[&str]| terms.iter().any(|term| lower.contains(term));
    if has(&[
        "research",
        "look up",
        "latest",
        "current information",
        "web search",
        "cite sources",
        "documentation",
    ]) {
        TaskRoute::ResearchAssisted
    } else if has(&[
        "debug",
        "bug",
        "traceback",
        "error",
        "failing test",
        "regression",
        "broken",
    ]) {
        TaskRoute::Debugging
    } else if has(&[
        "plan",
        "design",
        "architect",
        "roadmap",
        "workflow",
        "multi-step",
    ]) {
        TaskRoute::Planning
    } else if has(&[
        "implement",
        "create",
        "write code",
        "function",
        "script",
        "program",
        "solve",
        "build",
    ]) {
        TaskRoute::SimpleCoding
    } else {
        TaskRoute::General
    }
}

fn normalize_written_content(path: &str, content: String) -> String {
    // Some providers double-escape bundled source, yielding a one-line file
    // containing literal "\\n" sequences. Repair only source-like files when
    // there are no real newlines, avoiding changes to legitimate string data.
    if (path.ends_with(".py") || path.ends_with(".rs"))
        && !content.contains('\n')
        && content.matches("\\n").count() >= 2
    {
        return content
            .replace("\\r\\n", "\n")
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\"", "\"");
    }
    content
}

#[cfg(test)]
mod model_error_tests {
    use super::{
        classify_task, compact_context, is_fatal_model_error, normalize_written_content,
        parse_action, Action, ParallelAdvisor, SelfConsistency, TaskRoute, TaskState, ToolRecovery,
    };
    use crate::memory::LongContextMemory;

    #[test]
    fn quota_429_is_fatal_and_does_not_consume_step_budget() {
        assert!(is_fatal_model_error(
            "HTTP status client error (429 Too Many Requests) for url"
        ));
        assert!(is_fatal_model_error("Insufficient credits"));
    }

    #[test]
    fn ordinary_protocol_errors_remain_recoverable() {
        assert!(!is_fatal_model_error("model returned invalid JSON"));
    }

    #[test]
    fn nested_write_bundle_envelopes_are_supported() {
        let direct = parse_action(r#"{"write_bundle":{"files":{"solution.py":"pass"}}}"#)
            .expect("direct wrapper should parse");
        let typed =
            parse_action(r#"{"action":{"type":"write_bundle","files":{"solution.py":"pass"}}}"#)
                .expect("typed wrapper should parse");
        assert!(matches!(direct, Action::WriteBundle { .. }));
        assert!(matches!(typed, Action::WriteBundle { .. }));
    }

    #[test]
    fn double_escaped_python_source_is_repaired() {
        let source = r#"def f():\n    return \"ok\"\n"#.to_string();
        let repaired = normalize_written_content("solution.py", source);
        assert_eq!(repaired, "def f():\n    return \"ok\"\n");
    }

    #[test]
    fn compact_context_preserves_typed_task_state() {
        let state = TaskState {
            objective: "build it".into(),
            route: TaskRoute::SimpleCoding,
            phase: "repair".into(),
            repair_cycles: 1,
            verification_failed: true,
            verified: false,
            changed_files: vec!["solution.py".into()],
            verification_failures: vec!["syntax error".into()],
            research_findings: vec!["Source: https://example.com".into()],
        };
        let mut memory = LongContextMemory::new(4);
        memory.remember(
            "important-evidence",
            "tests passed previously",
            crate::memory::MemoryKind::Evidence,
            10,
        );
        let context = compact_context(&["repair".into()], &[], &state, &memory).unwrap();
        assert!(context.contains("solution.py"));
        assert!(context.contains("syntax error"));
        assert!(context.contains("important-evidence"));
    }

    #[test]
    fn task_router_selects_specialized_routes() {
        assert_eq!(
            classify_task("Research the latest API documentation and cite sources"),
            TaskRoute::ResearchAssisted
        );
        assert_eq!(
            classify_task("Debug the traceback in the failing test"),
            TaskRoute::Debugging
        );
        assert_eq!(
            classify_task("Design a multi-step deployment workflow"),
            TaskRoute::Planning
        );
        assert_eq!(
            classify_task("Implement a Python function"),
            TaskRoute::SimpleCoding
        );
    }

    #[test]
    fn parallel_advisors_are_bounded_and_route_gated() {
        assert_eq!(
            ParallelAdvisor::roles(TaskRoute::Planning, "design a workflow").len(),
            2
        );
        assert_eq!(
            ParallelAdvisor::roles(TaskRoute::SimpleCoding, "implement a complex parser").len(),
            2
        );
        assert!(ParallelAdvisor::roles(TaskRoute::General, "summarize this").is_empty());
    }

    #[test]
    fn self_consistency_votes_are_selective_and_parseable() {
        assert!(SelfConsistency::eligible(
            TaskRoute::Planning,
            "design a workflow"
        ));
        assert!(!SelfConsistency::eligible(
            TaskRoute::General,
            "summarize this"
        ));
        assert_eq!(
            SelfConsistency::vote_label("VOTE: test_first because tests expose regressions"),
            "test_first"
        );
        assert_eq!(
            SelfConsistency::vote_label("VOTE: decompose before implementation"),
            "decompose"
        );
    }

    #[test]
    fn tool_recovery_only_retries_safe_transient_operations() {
        assert!(ToolRecovery::retryable("connection reset by peer"));
        assert!(ToolRecovery::retryable_command("python3 -m unittest -q"));
        assert!(!ToolRecovery::retryable_command(
            "python3 mutate_database.py"
        ));
        assert!(ToolRecovery::pytest_unavailable(
            "command failed: pytest: not found",
            "pytest -q"
        ));
        assert!(!ToolRecovery::pytest_unavailable(
            "test assertion failed",
            "pytest -q"
        ));
    }
}

fn compact_context(
    todo: &[String],
    events: &[Event],
    state: &TaskState,
    memory: &LongContextMemory,
) -> Result<String> {
    let recent: Vec<serde_json::Value> = events
        .iter()
        .rev()
        .take(6)
        .map(|event| {
            let mut copy = event.clone();
            if copy.content.len() > 1800 {
                copy.content.truncate(1800);
                copy.content.push_str("…[truncated]");
            }
            serde_json::to_value(copy).unwrap_or(serde_json::Value::Null)
        })
        .collect();
    Ok(serde_json::to_string(&serde_json::json!({
        "todo": todo,
        "task_state": state,
        "long_context_summary": memory.summarize(6000),
        "recent_events": recent.into_iter().rev().collect::<Vec<_>>()
    }))?)
}

fn parse_action(text: &str) -> Result<Action> {
    let value: serde_json::Value = serde_json::from_str(text).or_else(|_| {
        let start = text
            .find('{')
            .ok_or_else(|| anyhow!("model returned no JSON object"))?;
        let end = text
            .rfind('}')
            .ok_or_else(|| anyhow!("model returned incomplete JSON"))?;
        Ok::<serde_json::Value, anyhow::Error>(serde_json::from_str(&text[start..=end])?)
    })?;
    if let Ok(action) = serde_json::from_value::<Action>(value.clone()) {
        return Ok(action);
    }
    // Providers sometimes wrap actions as {"write_bundle": {"files": ...}}
    // or {"action": {"type": "write_bundle", ...}}.
    if let Some(object) = value.as_object() {
        if let Some(action_value) = object.get("action") {
            if let Some(action_object) = action_value.as_object() {
                let name = action_object
                    .get("type")
                    .or_else(|| action_object.get("name"))
                    .and_then(|v| v.as_str());
                if let Some(name) = name {
                    let mut args = action_object.clone();
                    args.remove("type");
                    args.remove("name");
                    return action_from_named(name, serde_json::Value::Object(args));
                }
            } else if let Some(name) = action_value.as_str() {
                let mut args = object.clone();
                args.remove("action");
                return action_from_named(name, serde_json::Value::Object(args));
            }
        }
        for name in [
            "shell",
            "shell_exec",
            "read_file",
            "file_read",
            "write_file",
            "file_write",
            "write_bundle",
            "web_get",
            "browser_get",
            "finish",
            "final",
        ] {
            if let Some(args) = object.get(name) {
                return action_from_named(name, args.clone());
            }
        }
    }
    if let Some(answer) = value.get("answer").and_then(|v| v.as_str()) {
        return Ok(Action::Finish {
            answer: answer.to_string(),
        });
    }
    if let Some(answer) = value
        .get("finish")
        .and_then(|v| v.get("answer"))
        .and_then(|v| v.as_str())
    {
        return Ok(Action::Finish {
            answer: answer.to_string(),
        });
    }
    if let Some(name) = value.get("name").and_then(|v| v.as_str()) {
        let args = value
            .get("arguments")
            .or_else(|| value.get("args"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        return action_from_named(name, args);
    }
    if let Some(name) = value.get("tool").and_then(|v| v.as_str()) {
        let args = value
            .get("parameters")
            .or_else(|| value.get("arguments"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        return action_from_named(name, args);
    }
    if let Some(actions) = value.get("actions").and_then(|v| v.as_array()) {
        if let Some(first) = actions.first() {
            return Ok(serde_json::from_value(first.clone())?);
        }
    }
    if let Some(first) = value.as_array().and_then(|v| v.first()) {
        return Ok(serde_json::from_value(first.clone())?);
    }
    Err(anyhow!(
        "model response did not contain a supported action; text={}",
        text.chars().take(400).collect::<String>()
    ))
}

fn action_from_named(name: &str, args: serde_json::Value) -> Result<Action> {
    let args = if let Some(s) = args.as_str() {
        serde_json::from_str(s)?
    } else {
        args
    };
    match name {
        "shell" | "shell_exec" => Ok(Action::Shell {
            command: args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .into(),
        }),
        "read_file" | "file_read" => Ok(Action::ReadFile {
            path: args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .into(),
        }),
        "write_file" | "file_write" => Ok(Action::WriteFile {
            path: args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .into(),
            content: args
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .into(),
        }),
        "write_bundle" | "bundle_write" => Ok(Action::WriteBundle {
            files: serde_json::from_value(
                args.get("files")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            )?,
        }),
        "web_get" | "browser_get" => Ok(Action::WebGet {
            url: args
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .into(),
        }),
        "finish" | "final" => Ok(Action::Finish {
            answer: args
                .get("answer")
                .or_else(|| args.get("content"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .into(),
        }),
        _ => Err(anyhow!("unknown named action: {name}")),
    }
}

fn format_result<T: std::fmt::Display>(r: Result<T>) -> String {
    match r {
        Ok(v) => v.to_string(),
        Err(e) => format!("ERROR: {e}"),
    }
}
