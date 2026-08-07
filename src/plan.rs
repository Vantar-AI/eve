//! Compiled, reusable execution plans for Eve conversations.

use crate::{
    Conversation, ENDPOINT_FORMAT, Endpoint, EndpointState, ValidationErrors, project_validated,
    validate,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::Arc;
use thiserror::Error;

pub const PLAN_FORMAT: &str = "0.1.0";

#[derive(Debug, Error)]
pub enum PlanError {
    #[error(transparent)]
    InvalidConversation(#[from] ValidationErrors),
    #[error("plan codec error: {0}")]
    Codec(#[from] serde_json::Error),
    #[error("unsupported Eve Plan version {actual}; expected {expected}")]
    Version {
        actual: String,
        expected: &'static str,
    },
    #[error("plan identity mismatch: declared {declared}, calculated {calculated}")]
    IdentityMismatch {
        declared: String,
        calculated: String,
    },
    #[error("invalid Eve Plan: {0}")]
    Invalid(String),
}

/// A validated conversation projected into immutable endpoint machines.
///
/// Endpoints are reference counted so starting a session clones only an `Arc`, the semantic
/// identities, and the initial state rather than the complete projected state graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvePlan {
    pub eve_plan: String,
    pub conversation: String,
    pub conversation_identity: String,
    pub plan_identity: String,
    pub endpoints: Vec<Arc<Endpoint>>,
}

/// A plan whose digest and structural references have been verified for session use.
#[derive(Debug, Clone)]
pub struct PreparedPlan {
    conversation: String,
    conversation_identity: String,
    plan_identity: String,
    endpoints: Vec<Arc<Endpoint>>,
}

impl EvePlan {
    pub fn compile(conversation: &Conversation) -> Result<Self, PlanError> {
        validate(conversation)?;
        let conversation_identity = conversation_identity_validated(conversation)?;
        let mut endpoints = project_validated(conversation);
        endpoints.sort_by(|left, right| left.role.cmp(&right.role));
        let mut plan = Self {
            eve_plan: PLAN_FORMAT.to_string(),
            conversation: conversation.module.id.clone(),
            conversation_identity,
            plan_identity: String::new(),
            endpoints: endpoints.into_iter().map(Arc::new).collect(),
        };
        plan.plan_identity = plan.calculate_identity()?;
        plan.verify()?;
        Ok(plan)
    }

    /// Verify a deserialized plan once before using it to create cheap sessions.
    pub fn verify(&self) -> Result<(), PlanError> {
        if self.eve_plan != PLAN_FORMAT {
            return Err(PlanError::Version {
                actual: self.eve_plan.clone(),
                expected: PLAN_FORMAT,
            });
        }
        if !valid_id(&self.conversation) || !valid_sha256(&self.conversation_identity) {
            return Err(PlanError::Invalid(
                "conversation or conversation_identity has an invalid representation".to_string(),
            ));
        }
        if self.endpoints.len() != 2 {
            return Err(PlanError::Invalid(
                "Eve Plan v0 requires exactly two endpoints".to_string(),
            ));
        }

        let roles = self
            .endpoints
            .iter()
            .map(|endpoint| endpoint.role.as_str())
            .collect::<BTreeSet<_>>();
        if roles.len() != self.endpoints.len() {
            return Err(PlanError::Invalid(
                "endpoint roles must be unique".to_string(),
            ));
        }
        for endpoint in &self.endpoints {
            verify_endpoint(endpoint, &self.conversation, &roles)?;
        }

        let calculated = self.calculate_identity()?;
        if self.plan_identity != calculated {
            return Err(PlanError::IdentityMismatch {
                declared: self.plan_identity.clone(),
                calculated,
            });
        }
        Ok(())
    }

    pub fn endpoint(&self, role: &str) -> Option<&Arc<Endpoint>> {
        self.endpoints.iter().find(|endpoint| endpoint.role == role)
    }

    pub fn prepare(&self) -> Result<PreparedPlan, PlanError> {
        self.verify()?;
        Ok(self.prepared())
    }

    fn prepared(&self) -> PreparedPlan {
        PreparedPlan {
            conversation: self.conversation.clone(),
            conversation_identity: self.conversation_identity.clone(),
            plan_identity: self.plan_identity.clone(),
            endpoints: self.endpoints.clone(),
        }
    }

    fn calculate_identity(&self) -> Result<String, PlanError> {
        #[derive(Serialize)]
        struct SemanticPlan<'a> {
            eve_plan: &'a str,
            conversation: &'a str,
            conversation_identity: &'a str,
            endpoints: &'a [Arc<Endpoint>],
        }

        let encoded = serde_json::to_vec(&SemanticPlan {
            eve_plan: &self.eve_plan,
            conversation: &self.conversation,
            conversation_identity: &self.conversation_identity,
            endpoints: &self.endpoints,
        })?;
        Ok(sha256_identity(&encoded))
    }
}

impl PreparedPlan {
    pub fn compile(conversation: &Conversation) -> Result<Self, PlanError> {
        Ok(EvePlan::compile(conversation)?.prepared())
    }

    pub fn conversation(&self) -> &str {
        &self.conversation
    }

    pub fn conversation_identity(&self) -> &str {
        &self.conversation_identity
    }

    pub fn plan_identity(&self) -> &str {
        &self.plan_identity
    }

    pub fn endpoints(&self) -> &[Arc<Endpoint>] {
        &self.endpoints
    }

    pub fn endpoint(&self, role: &str) -> Option<&Arc<Endpoint>> {
        self.endpoints.iter().find(|endpoint| endpoint.role == role)
    }
}

pub fn conversation_identity(conversation: &Conversation) -> Result<String, PlanError> {
    validate(conversation)?;
    conversation_identity_validated(conversation)
}

fn conversation_identity_validated(conversation: &Conversation) -> Result<String, PlanError> {
    let mut semantic = conversation.clone();
    semantic.schema = None;
    semantic.annotations.clear();
    Ok(sha256_identity(&serde_json::to_vec(&semantic)?))
}

fn verify_endpoint(
    endpoint: &Endpoint,
    conversation: &str,
    roles: &BTreeSet<&str>,
) -> Result<(), PlanError> {
    if endpoint.eve_endpoint != ENDPOINT_FORMAT {
        return Err(PlanError::Invalid(format!(
            "endpoint {} uses unsupported format {}",
            endpoint.role, endpoint.eve_endpoint
        )));
    }
    if !valid_id(&endpoint.role) || !valid_id(&endpoint.initial) {
        return Err(PlanError::Invalid(format!(
            "endpoint {} has an invalid role or initial state ID",
            endpoint.role
        )));
    }
    if endpoint.conversation != conversation {
        return Err(PlanError::Invalid(format!(
            "endpoint {} names conversation {}; expected {conversation}",
            endpoint.role, endpoint.conversation
        )));
    }
    if endpoint.states.is_empty() {
        return Err(PlanError::Invalid(format!(
            "endpoint {} has no states",
            endpoint.role
        )));
    }

    let state_ids = endpoint
        .states
        .iter()
        .map(EndpointState::id)
        .collect::<BTreeSet<_>>();
    if state_ids.len() != endpoint.states.len() {
        return Err(PlanError::Invalid(format!(
            "endpoint {} contains duplicate state IDs",
            endpoint.role
        )));
    }
    if !state_ids.contains(endpoint.initial.as_str()) {
        return Err(PlanError::Invalid(format!(
            "endpoint {} initial state {} does not exist",
            endpoint.role, endpoint.initial
        )));
    }

    for state in &endpoint.states {
        if !valid_id(state.id()) {
            return Err(PlanError::Invalid(format!(
                "endpoint {} contains invalid state ID {}",
                endpoint.role,
                state.id()
            )));
        }
        for target in endpoint_successors(state) {
            if !state_ids.contains(target) {
                return Err(PlanError::Invalid(format!(
                    "endpoint {} state {} targets unknown state {target}",
                    endpoint.role,
                    state.id()
                )));
            }
        }
        if let Some(peer) = endpoint_peer(state)
            && !roles.contains(peer)
        {
            return Err(PlanError::Invalid(format!(
                "endpoint {} state {} references unknown role {peer}",
                endpoint.role,
                state.id()
            )));
        }
    }
    Ok(())
}

fn endpoint_successors(state: &EndpointState) -> Vec<&str> {
    match state {
        EndpointState::Send {
            next, on_failure, ..
        }
        | EndpointState::Receive {
            next, on_failure, ..
        }
        | EndpointState::SendCancel {
            next, on_failure, ..
        }
        | EndpointState::ReceiveCancel {
            next, on_failure, ..
        } => std::iter::once(next.as_str())
            .chain(on_failure.values().map(String::as_str))
            .collect(),
        EndpointState::Select {
            branches,
            on_failure,
            ..
        }
        | EndpointState::Branch {
            branches,
            on_failure,
            ..
        } => branches
            .values()
            .chain(on_failure.values())
            .map(String::as_str)
            .collect(),
        EndpointState::End { .. } | EndpointState::Fail { .. } => Vec::new(),
    }
}

fn endpoint_peer(state: &EndpointState) -> Option<&str> {
    match state {
        EndpointState::Send { to, .. } | EndpointState::SendCancel { to, .. } => Some(to),
        EndpointState::Receive { from, .. }
        | EndpointState::Branch { from, .. }
        | EndpointState::ReceiveCancel { from, .. } => Some(from),
        EndpointState::Select { .. } | EndpointState::End { .. } | EndpointState::Fail { .. } => {
            None
        }
    }
}

fn sha256_identity(encoded: &[u8]) -> String {
    let digest = Sha256::digest(encoded);
    let mut identity = String::with_capacity(7 + digest.len() * 2);
    identity.push_str("sha256:");
    for byte in digest {
        write!(&mut identity, "{byte:02x}").expect("writing into a string cannot fail");
    }
    identity
}

fn valid_id(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '.' | '-')
        })
}

fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation() -> Conversation {
        serde_json::from_str(include_str!("../examples/generate.eveconv.json")).unwrap()
    }

    #[test]
    fn compiled_plan_round_trips_and_verifies() {
        let plan = EvePlan::compile(&conversation()).unwrap();
        let encoded = serde_json::to_vec(&plan).unwrap();
        let decoded: EvePlan = serde_json::from_slice(&encoded).unwrap();
        decoded.verify().unwrap();
        let prepared = decoded.prepare().unwrap();
        assert_eq!(decoded.plan_identity, plan.plan_identity);
        assert_eq!(decoded.endpoints.len(), 2);
        assert_eq!(prepared.plan_identity(), plan.plan_identity);
        assert!(Arc::ptr_eq(&decoded.endpoints[0], &prepared.endpoints()[0]));
    }

    #[test]
    fn plan_identity_rejects_tampering() {
        let mut plan = EvePlan::compile(&conversation()).unwrap();
        Arc::make_mut(&mut plan.endpoints[0]).initial = "end".to_string();
        let error = plan.verify().unwrap_err();
        assert!(matches!(error, PlanError::IdentityMismatch { .. }));
    }

    #[test]
    fn plan_identity_is_stable_across_projection_metadata() {
        let original = conversation();
        let mut edited = original.clone();
        edited.schema = Some("different/schema.json".to_string());
        edited.annotations.insert(
            "editor".to_string(),
            serde_json::Value::String("human".to_string()),
        );
        assert_eq!(
            EvePlan::compile(&original).unwrap().plan_identity,
            EvePlan::compile(&edited).unwrap().plan_identity
        );
    }
}
