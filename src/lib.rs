use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use thiserror::Error;

pub mod benchmark;
pub mod runtime;

pub const CONVERSATION_FORMAT: &str = "0.1.0";
pub const ENDPOINT_FORMAT: &str = "0.1.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conversation {
    #[serde(rename = "$schema", default)]
    pub schema: Option<String>,
    pub eve_conversation: String,
    pub module: Module,
    pub roles: Vec<Role>,
    pub types: Vec<TypeDefinition>,
    #[serde(default)]
    pub failures: Vec<FailureDefinition>,
    pub initial: String,
    pub states: Vec<GlobalState>,
    #[serde(default)]
    pub annotations: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    pub id: String,
    #[serde(default)]
    pub semantic_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub id: String,
    #[serde(default)]
    pub placement: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDefinition {
    pub id: String,
    #[serde(flatten)]
    pub definition: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureDefinition {
    pub id: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GlobalState {
    Send {
        id: String,
        from: String,
        to: String,
        message: String,
        next: String,
        #[serde(default)]
        deadline: Option<String>,
        #[serde(default)]
        on_failure: BTreeMap<String, String>,
    },
    Choice {
        id: String,
        chooser: String,
        branches: BTreeMap<String, String>,
        #[serde(default)]
        on_failure: BTreeMap<String, String>,
    },
    Cancel {
        id: String,
        from: String,
        to: String,
        scope: String,
        next: String,
        #[serde(default)]
        on_failure: BTreeMap<String, String>,
    },
    End {
        id: String,
    },
    Fail {
        id: String,
        failure: String,
    },
}

impl GlobalState {
    pub fn id(&self) -> &str {
        match self {
            Self::Send { id, .. }
            | Self::Choice { id, .. }
            | Self::Cancel { id, .. }
            | Self::End { id }
            | Self::Fail { id, .. } => id,
        }
    }

    fn successors(&self) -> Vec<&str> {
        match self {
            Self::Send {
                next, on_failure, ..
            }
            | Self::Cancel {
                next, on_failure, ..
            } => std::iter::once(next.as_str())
                .chain(on_failure.values().map(String::as_str))
                .collect(),
            Self::Choice {
                branches,
                on_failure,
                ..
            } => branches
                .values()
                .chain(on_failure.values())
                .map(String::as_str)
                .collect(),
            Self::End { .. } | Self::Fail { .. } => Vec::new(),
        }
    }

    fn failure_target(&self, failure: &str) -> Option<&str> {
        match self {
            Self::Send { on_failure, .. }
            | Self::Choice { on_failure, .. }
            | Self::Cancel { on_failure, .. } => on_failure.get(failure).map(String::as_str),
            Self::End { .. } | Self::Fail { .. } => None,
        }
    }

    fn expected_frame(&self) -> String {
        match self {
            Self::Send {
                from, to, message, ..
            } => format!("data {from} -> {to}: {message}"),
            Self::Choice {
                chooser, branches, ..
            } => {
                let labels = branches.keys().cloned().collect::<Vec<_>>().join(" | ");
                format!("select by {chooser}: {labels}")
            }
            Self::Cancel {
                from, to, scope, ..
            } => format!("cancel {from} -> {to}: {scope}"),
            Self::End { .. } => "end of conversation".to_string(),
            Self::Fail { failure, .. } => format!("terminal failure {failure}"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationErrors {
    pub diagnostics: Vec<Diagnostic>,
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index > 0 {
                writeln!(formatter)?;
            }
            write!(
                formatter,
                "{} at {}: {}",
                diagnostic.code, diagnostic.path, diagnostic.message
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationErrors {}

pub fn validate(conversation: &Conversation) -> Result<(), ValidationErrors> {
    let mut diagnostics = Vec::new();

    if conversation.eve_conversation != CONVERSATION_FORMAT {
        diagnostics.push(Diagnostic {
            code: "EVE0001",
            path: "$.eve_conversation".to_string(),
            message: format!(
                "unsupported format {}; expected {CONVERSATION_FORMAT}",
                conversation.eve_conversation
            ),
        });
    }

    let role_ids = unique_ids(
        conversation.roles.iter().map(|role| role.id.as_str()),
        "$.roles",
        "role",
        &mut diagnostics,
    );
    if conversation.roles.len() != 2 {
        diagnostics.push(Diagnostic {
            code: "EVE0002",
            path: "$.roles".to_string(),
            message: "Eve Conversation v0 endpoint projection requires exactly two roles"
                .to_string(),
        });
    }

    let type_ids = unique_ids(
        conversation.types.iter().map(|item| item.id.as_str()),
        "$.types",
        "type",
        &mut diagnostics,
    );
    for (index, item) in conversation.types.iter().enumerate() {
        if !item.definition.contains_key("kind") {
            diagnostics.push(Diagnostic {
                code: "EVE0003",
                path: format!("$.types[{index}]"),
                message: format!("type {} is missing its kind", item.id),
            });
        }
    }

    let failure_ids = unique_ids(
        conversation.failures.iter().map(|item| item.id.as_str()),
        "$.failures",
        "failure",
        &mut diagnostics,
    );

    let mut states = BTreeMap::new();
    for (index, state) in conversation.states.iter().enumerate() {
        if states.insert(state.id(), state).is_some() {
            diagnostics.push(Diagnostic {
                code: "EVE0004",
                path: format!("$.states[{index}].id"),
                message: format!("duplicate state id {}", state.id()),
            });
        }
    }

    if !states.contains_key(conversation.initial.as_str()) {
        diagnostics.push(Diagnostic {
            code: "EVE0005",
            path: "$.initial".to_string(),
            message: format!("initial state {} does not exist", conversation.initial),
        });
    }

    for (index, state) in conversation.states.iter().enumerate() {
        match state {
            GlobalState::Send {
                from,
                to,
                message,
                next,
                on_failure,
                ..
            } => {
                validate_roles(index, from, to, &role_ids, &mut diagnostics);
                if !type_ids.contains(message) {
                    diagnostics.push(Diagnostic {
                        code: "EVE0006",
                        path: format!("$.states[{index}].message"),
                        message: format!("unknown message type {message}"),
                    });
                }
                validate_target(index, "next", next, &states, &mut diagnostics);
                validate_failure_edges(index, on_failure, &failure_ids, &states, &mut diagnostics);
            }
            GlobalState::Choice {
                chooser,
                branches,
                on_failure,
                ..
            } => {
                if !role_ids.contains(chooser) {
                    diagnostics.push(Diagnostic {
                        code: "EVE0007",
                        path: format!("$.states[{index}].chooser"),
                        message: format!("unknown chooser role {chooser}"),
                    });
                }
                if branches.len() < 2 {
                    diagnostics.push(Diagnostic {
                        code: "EVE0008",
                        path: format!("$.states[{index}].branches"),
                        message: "a choice requires at least two branches".to_string(),
                    });
                }
                for (label, target) in branches {
                    if label.is_empty() {
                        diagnostics.push(Diagnostic {
                            code: "EVE0009",
                            path: format!("$.states[{index}].branches"),
                            message: "branch labels cannot be empty".to_string(),
                        });
                    }
                    validate_target(index, label, target, &states, &mut diagnostics);
                }
                validate_failure_edges(index, on_failure, &failure_ids, &states, &mut diagnostics);
            }
            GlobalState::Cancel {
                from,
                to,
                scope,
                next,
                on_failure,
                ..
            } => {
                validate_roles(index, from, to, &role_ids, &mut diagnostics);
                if scope.is_empty() {
                    diagnostics.push(Diagnostic {
                        code: "EVE0010",
                        path: format!("$.states[{index}].scope"),
                        message: "cancellation scope cannot be empty".to_string(),
                    });
                }
                validate_target(index, "next", next, &states, &mut diagnostics);
                validate_failure_edges(index, on_failure, &failure_ids, &states, &mut diagnostics);
            }
            GlobalState::Fail { failure, .. } => {
                if !failure_ids.contains(failure) {
                    diagnostics.push(Diagnostic {
                        code: "EVE0019",
                        path: format!("$.states[{index}].failure"),
                        message: format!("unknown failure type {failure}"),
                    });
                }
            }
            GlobalState::End { .. } => {}
        }
    }

    if diagnostics.is_empty() {
        validate_reachability(conversation, &states, &mut diagnostics);
    }

    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(ValidationErrors { diagnostics })
    }
}

fn unique_ids<'a>(
    ids: impl Iterator<Item = &'a str>,
    path: &str,
    kind: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    for (index, id) in ids.enumerate() {
        if id.is_empty() {
            diagnostics.push(Diagnostic {
                code: "EVE0011",
                path: format!("{path}[{index}].id"),
                message: format!("{kind} id cannot be empty"),
            });
        } else if !seen.insert(id.to_string()) {
            diagnostics.push(Diagnostic {
                code: "EVE0012",
                path: format!("{path}[{index}].id"),
                message: format!("duplicate {kind} id {id}"),
            });
        }
    }
    seen
}

fn validate_roles(
    index: usize,
    from: &str,
    to: &str,
    roles: &BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !roles.contains(from) {
        diagnostics.push(Diagnostic {
            code: "EVE0013",
            path: format!("$.states[{index}].from"),
            message: format!("unknown sender role {from}"),
        });
    }
    if !roles.contains(to) {
        diagnostics.push(Diagnostic {
            code: "EVE0014",
            path: format!("$.states[{index}].to"),
            message: format!("unknown receiver role {to}"),
        });
    }
    if from == to {
        diagnostics.push(Diagnostic {
            code: "EVE0015",
            path: format!("$.states[{index}]"),
            message: "sender and receiver must be different roles".to_string(),
        });
    }
}

fn validate_target(
    index: usize,
    edge: &str,
    target: &str,
    states: &BTreeMap<&str, &GlobalState>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !states.contains_key(target) {
        diagnostics.push(Diagnostic {
            code: "EVE0016",
            path: format!("$.states[{index}].{edge}"),
            message: format!("target state {target} does not exist"),
        });
    }
}

fn validate_failure_edges(
    index: usize,
    on_failure: &BTreeMap<String, String>,
    failures: &BTreeSet<String>,
    states: &BTreeMap<&str, &GlobalState>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (failure, target) in on_failure {
        if !failures.contains(failure) {
            diagnostics.push(Diagnostic {
                code: "EVE0019",
                path: format!("$.states[{index}].on_failure.{failure}"),
                message: format!("unknown failure type {failure}"),
            });
        }
        validate_target(
            index,
            &format!("on_failure.{failure}"),
            target,
            states,
            diagnostics,
        );
        if let Some(target_state) = states.get(target.as_str()) {
            match target_state {
                GlobalState::Fail {
                    failure: terminal_failure,
                    ..
                } if terminal_failure == failure => {}
                _ => diagnostics.push(Diagnostic {
                    code: "EVE0020",
                    path: format!("$.states[{index}].on_failure.{failure}"),
                    message: format!(
                        "failure edge must target a terminal fail state for {failure}"
                    ),
                }),
            }
        }
    }
}

fn validate_reachability(
    conversation: &Conversation,
    states: &BTreeMap<&str, &GlobalState>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut reachable = BTreeSet::new();
    let mut queue = VecDeque::from([conversation.initial.as_str()]);
    let mut reaches_terminal = false;

    while let Some(id) = queue.pop_front() {
        if !reachable.insert(id) {
            continue;
        }
        let Some(state) = states.get(id) else {
            continue;
        };
        if matches!(state, GlobalState::End { .. } | GlobalState::Fail { .. }) {
            reaches_terminal = true;
        }
        queue.extend(state.successors());
    }

    for state in &conversation.states {
        if !reachable.contains(state.id()) {
            diagnostics.push(Diagnostic {
                code: "EVE0017",
                path: format!("$.states[id={}]", state.id()),
                message: "state is unreachable from the initial state".to_string(),
            });
        }
    }

    if !reaches_terminal {
        diagnostics.push(Diagnostic {
            code: "EVE0018",
            path: "$.states".to_string(),
            message: "no successful or failed terminal state is reachable from the initial state"
                .to_string(),
        });
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub eve_endpoint: String,
    pub conversation: String,
    pub role: String,
    pub initial: String,
    pub states: Vec<EndpointState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EndpointState {
    Send {
        id: String,
        to: String,
        message: String,
        next: String,
        #[serde(default)]
        deadline: Option<String>,
        #[serde(default)]
        on_failure: BTreeMap<String, String>,
    },
    Receive {
        id: String,
        from: String,
        message: String,
        next: String,
        #[serde(default)]
        deadline: Option<String>,
        #[serde(default)]
        on_failure: BTreeMap<String, String>,
    },
    Select {
        id: String,
        branches: BTreeMap<String, String>,
        #[serde(default)]
        on_failure: BTreeMap<String, String>,
    },
    Branch {
        id: String,
        from: String,
        branches: BTreeMap<String, String>,
        #[serde(default)]
        on_failure: BTreeMap<String, String>,
    },
    SendCancel {
        id: String,
        to: String,
        scope: String,
        next: String,
        #[serde(default)]
        on_failure: BTreeMap<String, String>,
    },
    ReceiveCancel {
        id: String,
        from: String,
        scope: String,
        next: String,
        #[serde(default)]
        on_failure: BTreeMap<String, String>,
    },
    End {
        id: String,
    },
    Fail {
        id: String,
        failure: String,
    },
}

impl EndpointState {
    pub fn id(&self) -> &str {
        match self {
            Self::Send { id, .. }
            | Self::Receive { id, .. }
            | Self::Select { id, .. }
            | Self::Branch { id, .. }
            | Self::SendCancel { id, .. }
            | Self::ReceiveCancel { id, .. }
            | Self::End { id }
            | Self::Fail { id, .. } => id,
        }
    }

    pub fn failure_target(&self, failure: &str) -> Option<&str> {
        match self {
            Self::Send { on_failure, .. }
            | Self::Receive { on_failure, .. }
            | Self::Select { on_failure, .. }
            | Self::Branch { on_failure, .. }
            | Self::SendCancel { on_failure, .. }
            | Self::ReceiveCancel { on_failure, .. } => on_failure.get(failure).map(String::as_str),
            Self::End { .. } | Self::Fail { .. } => None,
        }
    }
}

pub fn project(conversation: &Conversation) -> Result<Vec<Endpoint>, ValidationErrors> {
    validate(conversation)?;

    let mut endpoints = Vec::new();
    for role in &conversation.roles {
        let states = conversation
            .states
            .iter()
            .map(|state| project_state(state, &role.id))
            .collect();

        endpoints.push(Endpoint {
            eve_endpoint: ENDPOINT_FORMAT.to_string(),
            conversation: conversation.module.id.clone(),
            role: role.id.clone(),
            initial: conversation.initial.clone(),
            states,
        });
    }
    Ok(endpoints)
}

fn project_state(state: &GlobalState, role: &str) -> EndpointState {
    match state {
        GlobalState::Send {
            id,
            from,
            to,
            message,
            next,
            deadline,
            on_failure,
        } if from == role => EndpointState::Send {
            id: id.clone(),
            to: to.clone(),
            message: message.clone(),
            next: next.clone(),
            deadline: deadline.clone(),
            on_failure: on_failure.clone(),
        },
        GlobalState::Send {
            id,
            from,
            message,
            next,
            deadline,
            on_failure,
            ..
        } => EndpointState::Receive {
            id: id.clone(),
            from: from.clone(),
            message: message.clone(),
            next: next.clone(),
            deadline: deadline.clone(),
            on_failure: on_failure.clone(),
        },
        GlobalState::Choice {
            id,
            chooser,
            branches,
            on_failure,
        } if chooser == role => EndpointState::Select {
            id: id.clone(),
            branches: branches.clone(),
            on_failure: on_failure.clone(),
        },
        GlobalState::Choice {
            id,
            chooser,
            branches,
            on_failure,
        } => EndpointState::Branch {
            id: id.clone(),
            from: chooser.clone(),
            branches: branches.clone(),
            on_failure: on_failure.clone(),
        },
        GlobalState::Cancel {
            id,
            from,
            to,
            scope,
            next,
            on_failure,
        } if from == role => EndpointState::SendCancel {
            id: id.clone(),
            to: to.clone(),
            scope: scope.clone(),
            next: next.clone(),
            on_failure: on_failure.clone(),
        },
        GlobalState::Cancel {
            id,
            from,
            scope,
            next,
            on_failure,
            ..
        } => EndpointState::ReceiveCancel {
            id: id.clone(),
            from: from.clone(),
            scope: scope.clone(),
            next: next.clone(),
            on_failure: on_failure.clone(),
        },
        GlobalState::End { id } => EndpointState::End { id: id.clone() },
        GlobalState::Fail { id, failure } => EndpointState::Fail {
            id: id.clone(),
            failure: failure.clone(),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "op", rename_all = "snake_case")]
pub enum Frame {
    Data {
        from: String,
        to: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
    Select {
        by: String,
        label: String,
    },
    Cancel {
        from: String,
        to: String,
        scope: String,
    },
    Fault {
        observer: String,
        failure: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceReport {
    pub steps: usize,
    pub final_state: String,
    pub complete: bool,
    pub successful: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

#[derive(Debug, Error)]
#[error("invalid frame at step {step} in state {state}: expected {expected}; received {received}")]
pub struct TraceError {
    pub step: usize,
    pub state: String,
    pub expected: String,
    pub received: String,
}

pub fn verify_trace(
    conversation: &Conversation,
    frames: &[Frame],
) -> Result<TraceReport, Box<dyn std::error::Error>> {
    validate(conversation)?;
    let states = conversation
        .states
        .iter()
        .map(|state| (state.id(), state))
        .collect::<BTreeMap<_, _>>();
    let roles = conversation
        .roles
        .iter()
        .map(|role| role.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut current = conversation.initial.as_str();

    for (step, frame) in frames.iter().enumerate() {
        let state = states
            .get(current)
            .expect("validated conversation has all targets");
        current = match (state, frame) {
            (state, Frame::Fault { observer, failure })
                if roles.contains(observer.as_str()) && state.failure_target(failure).is_some() =>
            {
                state
                    .failure_target(failure)
                    .expect("guard established failure target")
            }
            (
                GlobalState::Send {
                    from,
                    to,
                    message,
                    next,
                    ..
                },
                Frame::Data {
                    from: actual_from,
                    to: actual_to,
                    message: actual_message,
                    ..
                },
            ) if from == actual_from && to == actual_to && message == actual_message => next,
            (
                GlobalState::Choice {
                    chooser, branches, ..
                },
                Frame::Select { by, label },
            ) if chooser == by && branches.contains_key(label) => &branches[label],
            (
                GlobalState::Cancel {
                    from,
                    to,
                    scope,
                    next,
                    ..
                },
                Frame::Cancel {
                    from: actual_from,
                    to: actual_to,
                    scope: actual_scope,
                },
            ) if from == actual_from && to == actual_to && scope == actual_scope => next,
            _ => {
                return Err(Box::new(TraceError {
                    step,
                    state: current.to_string(),
                    expected: state.expected_frame(),
                    received: describe_frame(frame),
                }));
            }
        };
    }

    let terminal = states[current];
    let complete = matches!(terminal, GlobalState::End { .. } | GlobalState::Fail { .. });
    let successful = matches!(terminal, GlobalState::End { .. });
    let failure = match terminal {
        GlobalState::Fail { failure, .. } => Some(failure.clone()),
        _ => None,
    };
    Ok(TraceReport {
        steps: frames.len(),
        final_state: current.to_string(),
        complete,
        successful,
        failure,
    })
}

pub fn describe_frame(frame: &Frame) -> String {
    match frame {
        Frame::Data {
            from, to, message, ..
        } => format!("data {from} -> {to}: {message}"),
        Frame::Select { by, label } => format!("select by {by}: {label}"),
        Frame::Cancel { from, to, scope } => format!("cancel {from} -> {to}: {scope}"),
        Frame::Fault { observer, failure } => {
            format!("fault observed by {observer}: {failure}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation() -> Conversation {
        serde_json::from_str(include_str!("../examples/generate.eveconv.json")).unwrap()
    }

    #[test]
    fn example_is_valid() {
        validate(&conversation()).unwrap();
    }

    #[test]
    fn projects_dual_endpoint_actions() {
        let endpoints = project(&conversation()).unwrap();
        let client = endpoints.iter().find(|item| item.role == "client").unwrap();
        let server = endpoints.iter().find(|item| item.role == "server").unwrap();

        assert!(matches!(
            &client.states[0],
            EndpointState::Send { to, message, .. }
                if to == "server" && message == "prompt"
        ));
        assert!(matches!(
            &server.states[0],
            EndpointState::Receive { from, message, .. }
                if from == "client" && message == "prompt"
        ));
        assert!(matches!(&server.states[1], EndpointState::Select { .. }));
        assert!(matches!(&client.states[1], EndpointState::Branch { .. }));
    }

    #[test]
    fn accepts_complete_conversation_trace() {
        let frames: Vec<Frame> = serde_json::from_str(include_str!(
            "../examples/traces/generate-cancel.valid.json"
        ))
        .unwrap();
        let report = verify_trace(&conversation(), &frames).unwrap();
        assert!(report.complete);
        assert_eq!(report.final_state, "end");
        assert!(report.successful);
        assert_eq!(report.failure, None);
    }

    #[test]
    fn accepts_declared_terminal_failure_trace() {
        let frames: Vec<Frame> = serde_json::from_str(include_str!(
            "../examples/traces/generate-transport-failure.valid.json"
        ))
        .unwrap();
        let report = verify_trace(&conversation(), &frames).unwrap();
        assert!(report.complete);
        assert!(!report.successful);
        assert_eq!(report.final_state, "transport-failed");
        assert_eq!(report.failure.as_deref(), Some("transport.closed"));
    }

    #[test]
    fn rejects_message_in_wrong_protocol_state() {
        let frames: Vec<Frame> = serde_json::from_str(include_str!(
            "../examples/traces/generate-wrong-order.invalid.json"
        ))
        .unwrap();
        let error = verify_trace(&conversation(), &frames).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("state decide"));
        assert!(message.contains("expected select by server"));
    }

    #[test]
    fn rejects_unknown_transition_target() {
        let mut invalid = conversation();
        if let GlobalState::Send { next, .. } = &mut invalid.states[0] {
            *next = "missing".to_string();
        }
        let errors = validate(&invalid).unwrap_err();
        assert!(
            errors
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EVE0016")
        );
    }

    #[test]
    fn rejects_failure_edge_to_a_non_failure_state() {
        let mut invalid = conversation();
        if let GlobalState::Send { on_failure, .. } = &mut invalid.states[0] {
            on_failure.insert("transport.closed".to_string(), "decide".to_string());
        }
        let errors = validate(&invalid).unwrap_err();
        assert!(
            errors
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EVE0020")
        );
    }
}
