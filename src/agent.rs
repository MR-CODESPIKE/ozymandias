use crate::{sandbox::Sandbox, tools::ToolRegistry};
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
        };
        for step in 0..self.max_steps {
            let context = compact_context(&todo, &events, &state)?;
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
                    let r = tools.shell(&sandbox, &command).await;
                    events.push(Event {
                        step,
                        role: "tool".into(),
                        content: format_result(r),
                    });
                }
                Action::ReadFile { path } => {
                    let r = tools.read_file(&path).await;
                    events.push(Event {
                        step,
                        role: "tool".into(),
                        content: format_result(r),
                    });
                }
                Action::WriteFile { path, content } => {
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
                        let verification = tools
                            .shell(
                                &sandbox,
                                "python3 -m py_compile solution.py test_solution.py && python3 -m pytest -q test_solution.py",
                            )
                            .await;
                        let passed = verification.is_ok();
                        let verification_detail = format_result(verification);
                        events.push(Event {
                            step,
                            role: "tool".into(),
                            content: verification_detail.clone(),
                        });
                        if passed {
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
                    let r = tools.web_get(&url).await;
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
        parse_action, Action, TaskRoute, TaskState,
    };

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
        };
        let context = compact_context(&["repair".into()], &[], &state).unwrap();
        assert!(context.contains("solution.py"));
        assert!(context.contains("syntax error"));
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
}

fn compact_context(todo: &[String], events: &[Event], state: &TaskState) -> Result<String> {
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
