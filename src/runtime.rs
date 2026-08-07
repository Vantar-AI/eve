use crate::plan::{CompactWirePlan, PlanError, PreparedPlan, WireOperation, WireTransition};
use crate::{Conversation, Endpoint, EndpointState, Frame};
use rcgen::CertifiedKey;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};
use std::time::Duration;
use thiserror::Error;

pub const WIRE_FORMAT: &str = "0.1.0";
pub const SESSION_FORMAT: &str = "0.1.0";
const MAX_ENVELOPE_BYTES: usize = 16 * 1024 * 1024;
const MAX_SESSION_PREFACE_BYTES: usize = 64 * 1024;
const SESSION_REJECTED: u8 = 0;
const SESSION_ACCEPTED: u8 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireEnvelope {
    pub eve_wire: String,
    pub conversation: String,
    pub conversation_identity: String,
    pub state: String,
    pub sequence: u64,
    pub frame: Frame,
}

/// Selects between the self-describing reference envelope and the plan-backed compact envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireEncoding {
    Reference,
    Compact,
}

impl WireEncoding {
    fn as_str(self) -> &'static str {
        match self {
            Self::Reference => "reference",
            Self::Compact => "compact",
        }
    }
}

/// The exact contract two network peers bind before exchanging Eve frames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionPreface {
    pub eve_session: String,
    pub conversation: String,
    pub conversation_identity: String,
    pub plan_identity: String,
    pub role: String,
    pub wire: WireEncoding,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompactEnvelope {
    #[serde(rename = "t")]
    transition: u16,
    #[serde(rename = "q")]
    sequence: u64,
    #[serde(rename = "p", default, skip_serializing_if = "Option::is_none")]
    payload: Option<Value>,
}

#[derive(Debug, Clone)]
struct CompactWireCodec {
    conversation: String,
    conversation_identity: String,
    plan_identity: String,
    wire: Arc<CompactWirePlan>,
}

#[derive(Debug, Clone)]
enum EnvelopeCodec {
    Reference,
    Compact(CompactWireCodec),
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Plan(#[from] PlanError),
    #[error("conversation has no projected endpoint for role {0}")]
    UnknownRole(String),
    #[error("wire codec error: {0}")]
    Codec(#[from] serde_json::Error),
    #[error("compact Eve Wire error: {0}")]
    CompactWire(String),
    #[error("{0} compact transport requires a verified Eve session preface")]
    SessionRequired(&'static str),
    #[error("Eve session mismatch for {field}: expected {expected}, received {actual}")]
    SessionMismatch {
        field: &'static str,
        expected: String,
        actual: String,
    },
    #[error("{0} peer rejected the Eve session preface")]
    SessionRejected(&'static str),
    #[error("transport I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0} transport closed before the conversation completed")]
    TransportClosed(&'static str),
    #[error("wire envelope is {actual} bytes; the limit is {limit} bytes")]
    EnvelopeTooLarge { actual: usize, limit: usize },
    #[error("Eve session preface is {actual} bytes; the limit is {limit} bytes")]
    SessionPrefaceTooLarge { actual: usize, limit: usize },
    #[error("unsupported Eve Wire version {actual}; expected {expected}")]
    WireVersion {
        actual: String,
        expected: &'static str,
    },
    #[error("endpoint {role} expected conversation {expected}, received {actual}")]
    ConversationMismatch {
        role: String,
        expected: String,
        actual: String,
    },
    #[error("endpoint {role} expected semantic identity {expected}, received {actual}")]
    IdentityMismatch {
        role: String,
        expected: String,
        actual: String,
    },
    #[error("endpoint {role} is at state {expected}, received frame for state {actual}")]
    StateMismatch {
        role: String,
        expected: String,
        actual: String,
    },
    #[error("endpoint {role} expected sequence {expected}, received {actual}")]
    SequenceMismatch {
        role: String,
        expected: u64,
        actual: u64,
    },
    #[error("endpoint {role} cannot {action} in state {state}: {detail}")]
    Protocol {
        role: String,
        state: String,
        action: &'static str,
        detail: String,
    },
    #[error("reference workload error: {0}")]
    Application(String),
    #[error("runtime worker panicked")]
    WorkerPanicked,
    #[error("QUIC transport error: {0}")]
    Quic(String),
    #[error("injected failure {failure} for {role} on {operation} operation {occurrence}")]
    InjectedFailure {
        role: String,
        operation: &'static str,
        occurrence: usize,
        failure: String,
    },
}

impl RuntimeError {
    fn transport_failure(&self) -> Option<&str> {
        match self {
            Self::Io(error) => Some(match error.kind() {
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                    "transport.timeout"
                }
                std::io::ErrorKind::ConnectionReset => "transport.reset",
                std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::AddrNotAvailable
                | std::io::ErrorKind::NotConnected => "transport.unreachable",
                _ => "transport.closed",
            }),
            Self::TransportClosed(_) | Self::Quic(_) => Some("transport.closed"),
            Self::InjectedFailure { failure, .. } => Some(failure),
            _ => None,
        }
    }
}

pub use crate::plan::conversation_identity;

#[derive(Debug)]
pub struct EndpointMachine {
    endpoint: Arc<Endpoint>,
    conversation_identity: String,
    plan_identity: String,
    current: String,
    sequence: u64,
}

impl EndpointMachine {
    pub fn for_role(conversation: &Conversation, role: &str) -> Result<Self, RuntimeError> {
        let plan = PreparedPlan::compile(conversation)?;
        Self::from_plan(&plan, role)
    }

    pub fn from_plan(plan: &PreparedPlan, role: &str) -> Result<Self, RuntimeError> {
        let endpoint = plan
            .endpoint(role)
            .cloned()
            .ok_or_else(|| RuntimeError::UnknownRole(role.to_string()))?;
        let current = endpoint.initial.clone();
        Ok(Self {
            endpoint,
            conversation_identity: plan.conversation_identity().to_string(),
            plan_identity: plan.plan_identity().to_string(),
            current,
            sequence: 0,
        })
    }

    pub fn role(&self) -> &str {
        &self.endpoint.role
    }

    pub fn conversation(&self) -> &str {
        &self.endpoint.conversation
    }

    pub fn identity(&self) -> &str {
        &self.conversation_identity
    }

    pub fn plan_identity(&self) -> &str {
        &self.plan_identity
    }

    pub fn current_state(&self) -> &str {
        &self.current
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn is_complete(&self) -> bool {
        matches!(
            self.state(),
            EndpointState::End { .. } | EndpointState::Fail { .. }
        )
    }

    pub fn is_successful(&self) -> bool {
        matches!(self.state(), EndpointState::End { .. })
    }

    pub fn failure(&self) -> Option<&str> {
        match self.state() {
            EndpointState::Fail { failure, .. } => Some(failure),
            _ => None,
        }
    }

    pub fn observe_failure(&mut self, failure: &str) -> Result<Frame, RuntimeError> {
        let state = self.current.clone();
        self.observe_failure_at(&state, failure)
    }

    fn observe_failure_at(&mut self, state_id: &str, failure: &str) -> Result<Frame, RuntimeError> {
        let was_current = self.current == state_id;
        let state = self
            .endpoint
            .states
            .iter()
            .find(|state| state.id() == state_id)
            .expect("a projected endpoint contains every validated state");
        let next = state
            .failure_target(failure)
            .ok_or_else(|| RuntimeError::Protocol {
                role: self.role().to_string(),
                state: state_id.to_string(),
                action: "observe failure",
                detail: format!("failure {failure} is not declared at this state"),
            })?;
        self.current = next.to_string();
        if was_current {
            self.sequence += 1;
        }
        Ok(Frame::Fault {
            observer: self.role().to_string(),
            failure: failure.to_string(),
        })
    }

    pub fn emit_data(&mut self, payload: Value) -> Result<WireEnvelope, RuntimeError> {
        match self.state().clone() {
            EndpointState::Send {
                to, message, next, ..
            } => {
                let frame = Frame::Data {
                    from: self.role().to_string(),
                    to,
                    message,
                    payload: Some(payload),
                };
                Ok(self.emit(frame, next))
            }
            state => Err(self.protocol_error("send data", format!("local action is {state:?}"))),
        }
    }

    pub fn emit_select(&mut self, label: &str) -> Result<WireEnvelope, RuntimeError> {
        match self.state().clone() {
            EndpointState::Select { branches, .. } => {
                let next = branches.get(label).cloned().ok_or_else(|| {
                    self.protocol_error(
                        "select branch",
                        format!(
                            "unknown branch {label}; expected one of {:?}",
                            branches.keys()
                        ),
                    )
                })?;
                let frame = Frame::Select {
                    by: self.role().to_string(),
                    label: label.to_string(),
                };
                Ok(self.emit(frame, next))
            }
            state => {
                Err(self.protocol_error("select branch", format!("local action is {state:?}")))
            }
        }
    }

    pub fn emit_cancel(&mut self) -> Result<WireEnvelope, RuntimeError> {
        match self.state().clone() {
            EndpointState::SendCancel {
                to, scope, next, ..
            } => {
                let frame = Frame::Cancel {
                    from: self.role().to_string(),
                    to,
                    scope,
                };
                Ok(self.emit(frame, next))
            }
            state => {
                Err(self.protocol_error("send cancellation", format!("local action is {state:?}")))
            }
        }
    }

    pub fn accept(&mut self, envelope: WireEnvelope) -> Result<Frame, RuntimeError> {
        self.validate_envelope(&envelope)?;
        let frame = envelope.frame;
        let next = match (self.state().clone(), &frame) {
            (
                EndpointState::Receive {
                    from,
                    message,
                    next,
                    ..
                },
                Frame::Data {
                    from: actual_from,
                    to,
                    message: actual_message,
                    ..
                },
            ) if from == *actual_from && self.role() == to && message == *actual_message => next,
            (EndpointState::Branch { from, branches, .. }, Frame::Select { by, label })
                if from == *by && branches.contains_key(label) =>
            {
                branches[label].clone()
            }
            (
                EndpointState::ReceiveCancel {
                    from, scope, next, ..
                },
                Frame::Cancel {
                    from: actual_from,
                    to,
                    scope: actual_scope,
                },
            ) if from == *actual_from && self.role() == to && scope == *actual_scope => next,
            (state, frame) => {
                return Err(self.protocol_error(
                    "receive frame",
                    format!("local action is {state:?}; received {frame:?}"),
                ));
            }
        };
        self.current = next;
        self.sequence += 1;
        Ok(frame)
    }

    fn state(&self) -> &EndpointState {
        self.endpoint
            .states
            .iter()
            .find(|state| state.id() == self.current)
            .expect("a projected endpoint contains every validated state")
    }

    fn emit(&mut self, frame: Frame, next: String) -> WireEnvelope {
        let envelope = WireEnvelope {
            eve_wire: WIRE_FORMAT.to_string(),
            conversation: self.conversation().to_string(),
            conversation_identity: self.identity().to_string(),
            state: self.current.clone(),
            sequence: self.sequence,
            frame,
        };
        self.current = next;
        self.sequence += 1;
        envelope
    }

    fn validate_envelope(&self, envelope: &WireEnvelope) -> Result<(), RuntimeError> {
        if envelope.eve_wire != WIRE_FORMAT {
            return Err(RuntimeError::WireVersion {
                actual: envelope.eve_wire.clone(),
                expected: WIRE_FORMAT,
            });
        }
        if envelope.conversation != self.conversation() {
            return Err(RuntimeError::ConversationMismatch {
                role: self.role().to_string(),
                expected: self.conversation().to_string(),
                actual: envelope.conversation.clone(),
            });
        }
        if envelope.conversation_identity != self.identity() {
            return Err(RuntimeError::IdentityMismatch {
                role: self.role().to_string(),
                expected: self.identity().to_string(),
                actual: envelope.conversation_identity.clone(),
            });
        }
        if envelope.state != self.current {
            return Err(RuntimeError::StateMismatch {
                role: self.role().to_string(),
                expected: self.current.clone(),
                actual: envelope.state.clone(),
            });
        }
        if envelope.sequence != self.sequence {
            return Err(RuntimeError::SequenceMismatch {
                role: self.role().to_string(),
                expected: self.sequence,
                actual: envelope.sequence,
            });
        }
        Ok(())
    }

    fn protocol_error(&self, action: &'static str, detail: String) -> RuntimeError {
        RuntimeError::Protocol {
            role: self.role().to_string(),
            state: self.current.clone(),
            action,
            detail,
        }
    }
}

pub trait Transport {
    fn plan(&self) -> &'static str;
    fn send(&mut self, envelope: &WireEnvelope) -> Result<(), RuntimeError>;
    fn receive(&mut self) -> Result<WireEnvelope, RuntimeError>;
    fn finish(&mut self, _role: &str) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn abort(&mut self) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultOperation {
    Send,
    Receive,
}

impl FaultOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Receive => "receive",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultPlan {
    pub role: String,
    pub operation: FaultOperation,
    pub occurrence: usize,
    pub failure: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_failure: Option<String>,
}

pub struct FaultInjectingTransport<T> {
    inner: T,
    role: String,
    plan: FaultPlan,
    sends: usize,
    receives: usize,
    fired: Arc<AtomicBool>,
}

impl<T> FaultInjectingTransport<T> {
    pub fn new(inner: T, role: &str, plan: FaultPlan) -> Result<Self, RuntimeError> {
        Self::with_fired(inner, role, plan, Arc::new(AtomicBool::new(false)))
    }

    fn with_fired(
        inner: T,
        role: &str,
        plan: FaultPlan,
        fired: Arc<AtomicBool>,
    ) -> Result<Self, RuntimeError> {
        if plan.occurrence == 0 {
            return Err(RuntimeError::Application(
                "fault occurrence must be at least one".to_string(),
            ));
        }
        Ok(Self {
            inner,
            role: role.to_string(),
            plan,
            sends: 0,
            receives: 0,
            fired,
        })
    }

    pub fn fired(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.fired)
    }

    fn should_inject(&mut self, operation: FaultOperation) -> bool {
        let occurrence = match operation {
            FaultOperation::Send => {
                self.sends += 1;
                self.sends
            }
            FaultOperation::Receive => {
                self.receives += 1;
                self.receives
            }
        };
        let should_inject = !self.fired.load(Ordering::Relaxed)
            && self.role == self.plan.role
            && operation == self.plan.operation
            && occurrence == self.plan.occurrence;
        if should_inject {
            self.fired.store(true, Ordering::Relaxed);
        }
        should_inject
    }

    fn injected_error(&self, operation: FaultOperation, failure: &str) -> RuntimeError {
        let occurrence = match operation {
            FaultOperation::Send => self.sends,
            FaultOperation::Receive => self.receives,
        };
        RuntimeError::InjectedFailure {
            role: self.role.clone(),
            operation: operation.as_str(),
            occurrence,
            failure: failure.to_string(),
        }
    }

    fn map_peer_error<U>(
        &self,
        operation: FaultOperation,
        result: Result<U, RuntimeError>,
    ) -> Result<U, RuntimeError> {
        match result {
            Err(error)
                if self.role != self.plan.role
                    && self.fired.load(Ordering::Relaxed)
                    && error.transport_failure().is_some() =>
            {
                if let Some(failure) = &self.plan.peer_failure {
                    return Err(self.injected_error(operation, failure));
                }
                Err(error)
            }
            other => other,
        }
    }
}

impl<T: Transport> Transport for FaultInjectingTransport<T> {
    fn plan(&self) -> &'static str {
        self.inner.plan()
    }

    fn send(&mut self, envelope: &WireEnvelope) -> Result<(), RuntimeError> {
        if self.should_inject(FaultOperation::Send) {
            self.inner.abort();
            return Err(self.injected_error(FaultOperation::Send, &self.plan.failure));
        }
        let result = self.inner.send(envelope);
        self.map_peer_error(FaultOperation::Send, result)
    }

    fn receive(&mut self) -> Result<WireEnvelope, RuntimeError> {
        if self.should_inject(FaultOperation::Receive) {
            self.inner.abort();
            return Err(self.injected_error(FaultOperation::Receive, &self.plan.failure));
        }
        let result = self.inner.receive();
        self.map_peer_error(FaultOperation::Receive, result)
    }

    fn finish(&mut self, role: &str) -> Result<(), RuntimeError> {
        self.inner.finish(role)
    }

    fn abort(&mut self) {
        self.inner.abort();
    }
}

impl SessionPreface {
    fn for_plan(plan: &PreparedPlan, role: &str, wire: WireEncoding) -> Result<Self, RuntimeError> {
        if plan.endpoint(role).is_none() {
            return Err(RuntimeError::UnknownRole(role.to_string()));
        }
        Ok(Self {
            eve_session: SESSION_FORMAT.to_string(),
            conversation: plan.conversation().to_string(),
            conversation_identity: plan.conversation_identity().to_string(),
            plan_identity: plan.plan_identity().to_string(),
            role: role.to_string(),
            wire,
        })
    }

    fn verify_peer(
        &self,
        plan: &PreparedPlan,
        expected_role: &str,
        expected_wire: WireEncoding,
    ) -> Result<(), RuntimeError> {
        if plan.endpoint(expected_role).is_none() {
            return Err(RuntimeError::UnknownRole(expected_role.to_string()));
        }
        require_session_field("eve_session", SESSION_FORMAT, &self.eve_session)?;
        require_session_field("conversation", plan.conversation(), &self.conversation)?;
        require_session_field(
            "conversation_identity",
            plan.conversation_identity(),
            &self.conversation_identity,
        )?;
        require_session_field("plan_identity", plan.plan_identity(), &self.plan_identity)?;
        require_session_field("role", expected_role, &self.role)?;
        require_session_field("wire", expected_wire.as_str(), self.wire.as_str())
    }
}

fn local_session_preface(
    codec: &EnvelopeCodec,
    plan: &PreparedPlan,
    local_role: &str,
    peer_role: &str,
) -> Result<SessionPreface, RuntimeError> {
    codec.verify_plan(plan)?;
    if plan.endpoint(peer_role).is_none() {
        return Err(RuntimeError::UnknownRole(peer_role.to_string()));
    }
    if local_role == peer_role {
        return Err(RuntimeError::SessionMismatch {
            field: "role_pair",
            expected: "distinct local and peer roles".to_string(),
            actual: local_role.to_string(),
        });
    }
    SessionPreface::for_plan(plan, local_role, codec.encoding())
}

fn require_session_field(
    field: &'static str,
    expected: &str,
    actual: &str,
) -> Result<(), RuntimeError> {
    if expected != actual {
        return Err(RuntimeError::SessionMismatch {
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

fn encode_session_preface(preface: &SessionPreface) -> Result<Vec<u8>, RuntimeError> {
    let encoded = serde_json::to_vec(preface)?;
    if encoded.len() > MAX_SESSION_PREFACE_BYTES {
        return Err(RuntimeError::SessionPrefaceTooLarge {
            actual: encoded.len(),
            limit: MAX_SESSION_PREFACE_BYTES,
        });
    }
    Ok(encoded)
}

fn decode_session_preface(encoded: &[u8]) -> Result<SessionPreface, RuntimeError> {
    if encoded.len() > MAX_SESSION_PREFACE_BYTES {
        return Err(RuntimeError::SessionPrefaceTooLarge {
            actual: encoded.len(),
            limit: MAX_SESSION_PREFACE_BYTES,
        });
    }
    Ok(serde_json::from_slice(encoded)?)
}

pub struct MemoryTransport {
    outbound: Option<Sender<Vec<u8>>>,
    inbound: Option<Receiver<Vec<u8>>>,
    codec: EnvelopeCodec,
}

pub fn memory_pair() -> (MemoryTransport, MemoryTransport) {
    memory_pair_with_codec(EnvelopeCodec::Reference)
}

pub fn memory_pair_compact(plan: &PreparedPlan) -> (MemoryTransport, MemoryTransport) {
    memory_pair_with_codec(EnvelopeCodec::for_plan(WireEncoding::Compact, plan))
}

fn memory_pair_with_codec(codec: EnvelopeCodec) -> (MemoryTransport, MemoryTransport) {
    let (client_to_server_tx, client_to_server_rx) = mpsc::channel();
    let (server_to_client_tx, server_to_client_rx) = mpsc::channel();
    (
        MemoryTransport {
            outbound: Some(client_to_server_tx),
            inbound: Some(server_to_client_rx),
            codec: codec.clone(),
        },
        MemoryTransport {
            outbound: Some(server_to_client_tx),
            inbound: Some(client_to_server_rx),
            codec,
        },
    )
}

impl Transport for MemoryTransport {
    fn plan(&self) -> &'static str {
        match self.codec.encoding() {
            WireEncoding::Reference => "memory",
            WireEncoding::Compact => "memory+compact",
        }
    }

    fn send(&mut self, envelope: &WireEnvelope) -> Result<(), RuntimeError> {
        let encoded = self.codec.encode(envelope)?;
        self.outbound
            .as_ref()
            .ok_or(RuntimeError::TransportClosed("memory"))?
            .send(encoded)
            .map_err(|_| RuntimeError::TransportClosed("memory"))
    }

    fn receive(&mut self) -> Result<WireEnvelope, RuntimeError> {
        let encoded = self
            .inbound
            .as_ref()
            .ok_or(RuntimeError::TransportClosed("memory"))?
            .recv()
            .map_err(|_| RuntimeError::TransportClosed("memory"))?;
        self.codec.decode(&encoded)
    }

    fn abort(&mut self) {
        self.outbound.take();
        self.inbound.take();
    }
}

pub struct TcpTransport {
    stream: TcpStream,
    codec: EnvelopeCodec,
    session_established: bool,
}

impl TcpTransport {
    pub fn connect(address: SocketAddr) -> Result<Self, RuntimeError> {
        Self::from_stream(TcpStream::connect(address)?)
    }

    pub fn connect_compact(address: SocketAddr, plan: &PreparedPlan) -> Result<Self, RuntimeError> {
        Self::from_stream_with_codec(
            TcpStream::connect(address)?,
            EnvelopeCodec::for_plan(WireEncoding::Compact, plan),
        )
    }

    pub fn from_stream(stream: TcpStream) -> Result<Self, RuntimeError> {
        Self::from_stream_with_codec(stream, EnvelopeCodec::Reference)
    }

    pub fn from_stream_compact(
        stream: TcpStream,
        plan: &PreparedPlan,
    ) -> Result<Self, RuntimeError> {
        Self::from_stream_with_codec(stream, EnvelopeCodec::for_plan(WireEncoding::Compact, plan))
    }

    fn from_stream_with_codec(
        stream: TcpStream,
        codec: EnvelopeCodec,
    ) -> Result<Self, RuntimeError> {
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        Ok(Self {
            stream,
            codec,
            session_established: false,
        })
    }

    pub fn establish_session(
        &mut self,
        plan: &PreparedPlan,
        local_role: &str,
        peer_role: &str,
    ) -> Result<SessionPreface, RuntimeError> {
        let local = local_session_preface(&self.codec, plan, local_role, peer_role)?;
        let encoded = encode_session_preface(&local)?;
        let length =
            u32::try_from(encoded.len()).map_err(|_| RuntimeError::SessionPrefaceTooLarge {
                actual: encoded.len(),
                limit: u32::MAX as usize,
            })?;
        self.stream.write_all(&length.to_be_bytes())?;
        self.stream.write_all(&encoded)?;
        self.stream.flush()?;

        let mut length = [0_u8; 4];
        self.stream.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        if length > MAX_SESSION_PREFACE_BYTES {
            return Err(RuntimeError::SessionPrefaceTooLarge {
                actual: length,
                limit: MAX_SESSION_PREFACE_BYTES,
            });
        }
        let mut encoded = vec![0; length];
        self.stream.read_exact(&mut encoded)?;
        let peer = decode_session_preface(&encoded).and_then(|peer| {
            peer.verify_peer(plan, peer_role, self.codec.encoding())?;
            Ok(peer)
        });
        let peer = match peer {
            Ok(peer) => peer,
            Err(error) => {
                let _ = self.stream.write_all(&[SESSION_REJECTED]);
                let _ = self.stream.flush();
                self.abort();
                return Err(error);
            }
        };
        let mut peer_status = [SESSION_REJECTED];
        let status_exchange = (|| -> std::io::Result<()> {
            self.stream.write_all(&[SESSION_ACCEPTED])?;
            self.stream.flush()?;
            self.stream.read_exact(&mut peer_status)
        })();
        if let Err(error) = status_exchange {
            self.abort();
            return Err(RuntimeError::Io(error));
        }
        if peer_status[0] != SESSION_ACCEPTED {
            self.abort();
            return Err(RuntimeError::SessionRejected("TCP"));
        }
        self.session_established = true;
        Ok(peer)
    }

    pub fn session_established(&self) -> bool {
        self.session_established
    }

    fn require_compact_session(&self) -> Result<(), RuntimeError> {
        if self.codec.encoding() == WireEncoding::Compact && !self.session_established {
            return Err(RuntimeError::SessionRequired("TCP"));
        }
        Ok(())
    }
}

impl Transport for TcpTransport {
    fn plan(&self) -> &'static str {
        match self.codec.encoding() {
            WireEncoding::Reference => "tcp",
            WireEncoding::Compact => "tcp+compact",
        }
    }

    fn send(&mut self, envelope: &WireEnvelope) -> Result<(), RuntimeError> {
        self.require_compact_session()?;
        let encoded = self.codec.encode(envelope)?;
        let length = u32::try_from(encoded.len()).map_err(|_| RuntimeError::EnvelopeTooLarge {
            actual: encoded.len(),
            limit: u32::MAX as usize,
        })?;
        self.stream.write_all(&length.to_be_bytes())?;
        self.stream.write_all(&encoded)?;
        self.stream.flush()?;
        Ok(())
    }

    fn receive(&mut self) -> Result<WireEnvelope, RuntimeError> {
        self.require_compact_session()?;
        let mut length = [0_u8; 4];
        self.stream.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        if length > MAX_ENVELOPE_BYTES {
            return Err(RuntimeError::EnvelopeTooLarge {
                actual: length,
                limit: MAX_ENVELOPE_BYTES,
            });
        }
        let mut encoded = vec![0; length];
        self.stream.read_exact(&mut encoded)?;
        self.codec.decode(&encoded)
    }

    fn abort(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

pub struct QuicListener {
    endpoint: quinn::Endpoint,
    certificate: CertificateDer<'static>,
    runtime: tokio::runtime::Runtime,
}

impl QuicListener {
    pub fn bind(address: SocketAddr) -> Result<Self, RuntimeError> {
        let CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
                .map_err(|error| RuntimeError::Quic(error.to_string()))?;
        let certificate = CertificateDer::from(cert);
        let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config =
            quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())
                .map_err(|error| RuntimeError::Quic(error.to_string()))?;
        let runtime = quic_runtime()?;
        let endpoint = {
            let _guard = runtime.enter();
            quinn::Endpoint::server(server_config, address)?
        };
        Ok(Self {
            endpoint,
            certificate,
            runtime,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, RuntimeError> {
        Ok(self.endpoint.local_addr()?)
    }

    pub fn certificate_der(&self) -> &[u8] {
        self.certificate.as_ref()
    }

    pub fn accept(self) -> Result<QuicTransport, RuntimeError> {
        self.accept_with_codec(EnvelopeCodec::Reference)
    }

    pub fn accept_compact(self, plan: &PreparedPlan) -> Result<QuicTransport, RuntimeError> {
        self.accept_with_codec(EnvelopeCodec::for_plan(WireEncoding::Compact, plan))
    }

    fn accept_with_codec(self, codec: EnvelopeCodec) -> Result<QuicTransport, RuntimeError> {
        let Self {
            endpoint,
            certificate: _,
            runtime,
        } = self;
        let incoming = runtime
            .block_on(endpoint.accept())
            .ok_or_else(|| RuntimeError::Quic("QUIC listener closed".to_string()))?;
        let connection = runtime
            .block_on(async { incoming.await })
            .map_err(|error| RuntimeError::Quic(error.to_string()))?;
        let (send, receive) = runtime
            .block_on(connection.accept_bi())
            .map_err(|error| RuntimeError::Quic(error.to_string()))?;
        Ok(QuicTransport {
            _endpoint: endpoint,
            connection,
            send,
            receive,
            finished: false,
            runtime,
            codec,
            session_established: false,
        })
    }
}

pub struct QuicTransport {
    _endpoint: quinn::Endpoint,
    connection: quinn::Connection,
    send: quinn::SendStream,
    receive: quinn::RecvStream,
    finished: bool,
    runtime: tokio::runtime::Runtime,
    codec: EnvelopeCodec,
    session_established: bool,
}

impl QuicTransport {
    pub fn connect(address: SocketAddr, trusted_certificate: &[u8]) -> Result<Self, RuntimeError> {
        Self::connect_with_codec(address, trusted_certificate, EnvelopeCodec::Reference)
    }

    pub fn connect_compact(
        address: SocketAddr,
        trusted_certificate: &[u8],
        plan: &PreparedPlan,
    ) -> Result<Self, RuntimeError> {
        Self::connect_with_codec(
            address,
            trusted_certificate,
            EnvelopeCodec::for_plan(WireEncoding::Compact, plan),
        )
    }

    fn connect_with_codec(
        address: SocketAddr,
        trusted_certificate: &[u8],
        codec: EnvelopeCodec,
    ) -> Result<Self, RuntimeError> {
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(CertificateDer::from(trusted_certificate.to_vec()))
            .map_err(|error| RuntimeError::Quic(error.to_string()))?;
        let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots))
            .map_err(|error| RuntimeError::Quic(error.to_string()))?;
        let runtime = quic_runtime()?;
        let endpoint = {
            let _guard = runtime.enter();
            let mut endpoint =
                quinn::Endpoint::client("0.0.0.0:0".parse().expect("valid address"))?;
            endpoint.set_default_client_config(client_config);
            endpoint
        };
        let connection = runtime.block_on(async {
            endpoint
                .connect(address, "localhost")
                .map_err(|error| RuntimeError::Quic(error.to_string()))?
                .await
                .map_err(|error| RuntimeError::Quic(error.to_string()))
        })?;
        let (send, receive) = runtime
            .block_on(connection.open_bi())
            .map_err(|error| RuntimeError::Quic(error.to_string()))?;
        Ok(Self {
            _endpoint: endpoint,
            connection,
            send,
            receive,
            finished: false,
            runtime,
            codec,
            session_established: false,
        })
    }

    pub fn establish_session(
        &mut self,
        plan: &PreparedPlan,
        local_role: &str,
        peer_role: &str,
    ) -> Result<SessionPreface, RuntimeError> {
        let local = local_session_preface(&self.codec, plan, local_role, peer_role)?;
        let encoded = encode_session_preface(&local)?;
        let length =
            u32::try_from(encoded.len()).map_err(|_| RuntimeError::SessionPrefaceTooLarge {
                actual: encoded.len(),
                limit: u32::MAX as usize,
            })?;
        let send = &mut self.send;
        let receive = &mut self.receive;
        let peer_length = self
            .runtime
            .block_on(async {
                send.write_all(&length.to_be_bytes())
                    .await
                    .map_err(|error| error.to_string())?;
                send.write_all(&encoded)
                    .await
                    .map_err(|error| error.to_string())?;
                let mut peer_length = [0_u8; 4];
                receive
                    .read_exact(&mut peer_length)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>(u32::from_be_bytes(peer_length) as usize)
            })
            .map_err(RuntimeError::Quic)?;
        if peer_length > MAX_SESSION_PREFACE_BYTES {
            return Err(RuntimeError::SessionPrefaceTooLarge {
                actual: peer_length,
                limit: MAX_SESSION_PREFACE_BYTES,
            });
        }
        let mut peer_encoded = vec![0; peer_length];
        self.runtime
            .block_on(self.receive.read_exact(&mut peer_encoded))
            .map_err(|error| RuntimeError::Quic(error.to_string()))?;
        let peer = decode_session_preface(&peer_encoded).and_then(|peer| {
            peer.verify_peer(plan, peer_role, self.codec.encoding())?;
            Ok(peer)
        });
        let peer = match peer {
            Ok(peer) => peer,
            Err(error) => {
                let send = &mut self.send;
                let _ = self.runtime.block_on(send.write_all(&[SESSION_REJECTED]));
                self.abort();
                return Err(error);
            }
        };
        let mut peer_status = [SESSION_REJECTED];
        let send = &mut self.send;
        let receive = &mut self.receive;
        let status_exchange = self.runtime.block_on(async {
            send.write_all(&[SESSION_ACCEPTED])
                .await
                .map_err(|error| error.to_string())?;
            receive
                .read_exact(&mut peer_status)
                .await
                .map_err(|error| error.to_string())
        });
        if let Err(error) = status_exchange {
            self.abort();
            return Err(RuntimeError::Quic(error));
        }
        if peer_status[0] != SESSION_ACCEPTED {
            self.abort();
            return Err(RuntimeError::SessionRejected("QUIC"));
        }
        self.session_established = true;
        Ok(peer)
    }

    pub fn session_established(&self) -> bool {
        self.session_established
    }

    fn require_compact_session(&self) -> Result<(), RuntimeError> {
        if self.codec.encoding() == WireEncoding::Compact && !self.session_established {
            return Err(RuntimeError::SessionRequired("QUIC"));
        }
        Ok(())
    }
}

impl Transport for QuicTransport {
    fn plan(&self) -> &'static str {
        match self.codec.encoding() {
            WireEncoding::Reference => "quic",
            WireEncoding::Compact => "quic+compact",
        }
    }

    fn send(&mut self, envelope: &WireEnvelope) -> Result<(), RuntimeError> {
        self.require_compact_session()?;
        let encoded = self.codec.encode(envelope)?;
        let length = u32::try_from(encoded.len()).map_err(|_| RuntimeError::EnvelopeTooLarge {
            actual: encoded.len(),
            limit: u32::MAX as usize,
        })?;
        let send = &mut self.send;
        self.runtime
            .block_on(async {
                send.write_all(&length.to_be_bytes()).await?;
                send.write_all(&encoded).await
            })
            .map_err(|error| RuntimeError::Quic(error.to_string()))
    }

    fn receive(&mut self) -> Result<WireEnvelope, RuntimeError> {
        self.require_compact_session()?;
        let receive = &mut self.receive;
        let mut length = [0_u8; 4];
        self.runtime
            .block_on(receive.read_exact(&mut length))
            .map_err(|error| RuntimeError::Quic(error.to_string()))?;
        let length = u32::from_be_bytes(length) as usize;
        if length > MAX_ENVELOPE_BYTES {
            return Err(RuntimeError::EnvelopeTooLarge {
                actual: length,
                limit: MAX_ENVELOPE_BYTES,
            });
        }
        let mut encoded = vec![0; length];
        self.runtime
            .block_on(receive.read_exact(&mut encoded))
            .map_err(|error| RuntimeError::Quic(error.to_string()))?;
        self.codec.decode(&encoded)
    }

    fn finish(&mut self, role: &str) -> Result<(), RuntimeError> {
        if self.finished {
            return Ok(());
        }
        let send = &mut self.send;
        let receive = &mut self.receive;
        match role {
            "client" => {
                self.runtime
                    .block_on(async {
                        send.write_all(&0_u32.to_be_bytes())
                            .await
                            .map_err(|error| error.to_string())?;
                        read_quic_close_marker(receive).await
                    })
                    .map_err(RuntimeError::Quic)?;
                send.finish()
                    .map_err(|error| RuntimeError::Quic(error.to_string()))?;
                self.runtime
                    .block_on(receive.read_to_end(0))
                    .map_err(|error| RuntimeError::Quic(error.to_string()))?;
            }
            "server" => {
                self.runtime
                    .block_on(async {
                        read_quic_close_marker(receive).await?;
                        send.write_all(&0_u32.to_be_bytes())
                            .await
                            .map_err(|error| error.to_string())?;
                        receive
                            .read_to_end(0)
                            .await
                            .map_err(|error| error.to_string())
                    })
                    .map_err(RuntimeError::Quic)?;
                send.finish()
                    .map_err(|error| RuntimeError::Quic(error.to_string()))?;
            }
            role => {
                return Err(RuntimeError::Quic(format!(
                    "QUIC close handshake does not support role {role}"
                )));
            }
        }
        self.finished = true;
        Ok(())
    }

    fn abort(&mut self) {
        self.connection
            .close(quinn::VarInt::from_u32(1), b"eve typed failure");
    }
}

async fn read_quic_close_marker(receive: &mut quinn::RecvStream) -> Result<(), String> {
    let mut marker = [0_u8; 4];
    receive
        .read_exact(&mut marker)
        .await
        .map_err(|error| error.to_string())?;
    if u32::from_be_bytes(marker) != 0 {
        return Err("expected Eve QUIC close marker".to_string());
    }
    Ok(())
}

fn quic_runtime() -> Result<tokio::runtime::Runtime, RuntimeError> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?)
}

impl EnvelopeCodec {
    fn for_plan(encoding: WireEncoding, plan: &PreparedPlan) -> Self {
        match encoding {
            WireEncoding::Reference => Self::Reference,
            WireEncoding::Compact => Self::Compact(CompactWireCodec {
                conversation: plan.conversation().to_string(),
                conversation_identity: plan.conversation_identity().to_string(),
                plan_identity: plan.plan_identity().to_string(),
                wire: plan.shared_wire_plan(),
            }),
        }
    }

    fn encoding(&self) -> WireEncoding {
        match self {
            Self::Reference => WireEncoding::Reference,
            Self::Compact(_) => WireEncoding::Compact,
        }
    }

    fn verify_plan(&self, plan: &PreparedPlan) -> Result<(), RuntimeError> {
        let Self::Compact(codec) = self else {
            return Ok(());
        };
        require_session_field("conversation", &codec.conversation, plan.conversation())?;
        require_session_field(
            "conversation_identity",
            &codec.conversation_identity,
            plan.conversation_identity(),
        )?;
        require_session_field("plan_identity", &codec.plan_identity, plan.plan_identity())
    }

    fn encode(&self, envelope: &WireEnvelope) -> Result<Vec<u8>, RuntimeError> {
        match self {
            Self::Reference => encode_envelope(envelope),
            Self::Compact(codec) => codec.encode(envelope),
        }
    }

    fn decode(&self, encoded: &[u8]) -> Result<WireEnvelope, RuntimeError> {
        match self {
            Self::Reference => decode_envelope(encoded),
            Self::Compact(codec) => codec.decode(encoded),
        }
    }
}

impl CompactWireCodec {
    fn encode(&self, envelope: &WireEnvelope) -> Result<Vec<u8>, RuntimeError> {
        if envelope.eve_wire != WIRE_FORMAT {
            return Err(RuntimeError::WireVersion {
                actual: envelope.eve_wire.clone(),
                expected: WIRE_FORMAT,
            });
        }
        if envelope.conversation != self.conversation {
            return Err(RuntimeError::CompactWire(format!(
                "codec is prepared for conversation {}, received {}",
                self.conversation, envelope.conversation
            )));
        }
        if envelope.conversation_identity != self.conversation_identity {
            return Err(RuntimeError::CompactWire(format!(
                "codec is prepared for identity {}, received {}",
                self.conversation_identity, envelope.conversation_identity
            )));
        }

        let transition = self
            .wire
            .transitions
            .iter()
            .find(|transition| transition_matches(transition, envelope))
            .ok_or_else(|| {
                RuntimeError::CompactWire(format!(
                    "state {} and frame {:?} have no transition ID in the prepared plan",
                    envelope.state, envelope.frame
                ))
            })?;
        let payload = match &envelope.frame {
            Frame::Data { payload, .. } => payload.clone(),
            Frame::Select { .. } | Frame::Cancel { .. } => None,
            Frame::Fault { .. } => {
                return Err(RuntimeError::CompactWire(
                    "local fault observations are not transmitted".to_string(),
                ));
            }
        };
        encode_compact_envelope(&CompactEnvelope {
            transition: transition.id,
            sequence: envelope.sequence,
            payload,
        })
    }

    fn decode(&self, encoded: &[u8]) -> Result<WireEnvelope, RuntimeError> {
        if encoded.len() > MAX_ENVELOPE_BYTES {
            return Err(RuntimeError::EnvelopeTooLarge {
                actual: encoded.len(),
                limit: MAX_ENVELOPE_BYTES,
            });
        }
        let compact: CompactEnvelope = serde_json::from_slice(encoded)?;
        let transition = compact
            .transition
            .checked_sub(1)
            .and_then(|index| self.wire.transitions.get(index as usize))
            .filter(|transition| transition.id == compact.transition)
            .ok_or_else(|| {
                RuntimeError::CompactWire(format!("unknown transition ID {}", compact.transition))
            })?;
        let frame = match &transition.operation {
            WireOperation::Data { from, to, message } => Frame::Data {
                from: from.clone(),
                to: to.clone(),
                message: message.clone(),
                payload: compact.payload,
            },
            WireOperation::Select { by, label } => {
                reject_control_payload(&compact)?;
                Frame::Select {
                    by: by.clone(),
                    label: label.clone(),
                }
            }
            WireOperation::Cancel { from, to, scope } => {
                reject_control_payload(&compact)?;
                Frame::Cancel {
                    from: from.clone(),
                    to: to.clone(),
                    scope: scope.clone(),
                }
            }
        };
        Ok(WireEnvelope {
            eve_wire: WIRE_FORMAT.to_string(),
            conversation: self.conversation.clone(),
            conversation_identity: self.conversation_identity.clone(),
            state: transition.state.clone(),
            sequence: compact.sequence,
            frame,
        })
    }
}

fn transition_matches(transition: &WireTransition, envelope: &WireEnvelope) -> bool {
    if transition.state != envelope.state {
        return false;
    }
    match (&transition.operation, &envelope.frame) {
        (
            WireOperation::Data { from, to, message },
            Frame::Data {
                from: actual_from,
                to: actual_to,
                message: actual_message,
                ..
            },
        ) => from == actual_from && to == actual_to && message == actual_message,
        (
            WireOperation::Select { by, label },
            Frame::Select {
                by: actual_by,
                label: actual_label,
            },
        ) => by == actual_by && label == actual_label,
        (
            WireOperation::Cancel { from, to, scope },
            Frame::Cancel {
                from: actual_from,
                to: actual_to,
                scope: actual_scope,
            },
        ) => from == actual_from && to == actual_to && scope == actual_scope,
        _ => false,
    }
}

fn reject_control_payload(compact: &CompactEnvelope) -> Result<(), RuntimeError> {
    if compact.payload.is_some() {
        return Err(RuntimeError::CompactWire(
            "select and cancel transitions cannot carry a payload".to_string(),
        ));
    }
    Ok(())
}

fn encode_compact_envelope(envelope: &CompactEnvelope) -> Result<Vec<u8>, RuntimeError> {
    let encoded = serde_json::to_vec(envelope)?;
    if encoded.len() > MAX_ENVELOPE_BYTES {
        return Err(RuntimeError::EnvelopeTooLarge {
            actual: encoded.len(),
            limit: MAX_ENVELOPE_BYTES,
        });
    }
    Ok(encoded)
}

pub(crate) struct PreparedCompactRoundTrip {
    codec: EnvelopeCodec,
}

impl PreparedCompactRoundTrip {
    pub(crate) fn new(plan: &PreparedPlan) -> Self {
        Self {
            codec: EnvelopeCodec::for_plan(WireEncoding::Compact, plan),
        }
    }

    pub(crate) fn execute(&self, envelope: &WireEnvelope) -> Result<WireEnvelope, RuntimeError> {
        self.codec.decode(&self.codec.encode(envelope)?)
    }
}

fn encode_envelope(envelope: &WireEnvelope) -> Result<Vec<u8>, RuntimeError> {
    let encoded = serde_json::to_vec(envelope)?;
    if encoded.len() > MAX_ENVELOPE_BYTES {
        return Err(RuntimeError::EnvelopeTooLarge {
            actual: encoded.len(),
            limit: MAX_ENVELOPE_BYTES,
        });
    }
    Ok(encoded)
}

fn decode_envelope(encoded: &[u8]) -> Result<WireEnvelope, RuntimeError> {
    if encoded.len() > MAX_ENVELOPE_BYTES {
        return Err(RuntimeError::EnvelopeTooLarge {
            actual: encoded.len(),
            limit: MAX_ENVELOPE_BYTES,
        });
    }
    Ok(serde_json::from_slice(encoded)?)
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionReport {
    pub role: String,
    pub transport: String,
    pub conversation: String,
    pub conversation_identity: String,
    pub plan_identity: String,
    pub semantic_trace_identity: String,
    pub frames: u64,
    pub tokens: Vec<u64>,
    pub completed: bool,
    pub successful: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    pub final_state: String,
    #[serde(skip_serializing)]
    pub semantic_trace: Vec<Frame>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DemoReport {
    pub transport_plan: String,
    pub conversation_identity: String,
    pub plan_identity: String,
    pub semantic_trace_equivalent: bool,
    pub outcome_equivalent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault_plan: Option<FaultPlan>,
    pub client: ExecutionReport,
    pub server: ExecutionReport,
}

pub fn run_generate_client<T: Transport>(
    conversation: &Conversation,
    transport: &mut T,
    prompt: &str,
    cancel_after: Option<usize>,
) -> Result<ExecutionReport, RuntimeError> {
    let plan = PreparedPlan::compile(conversation)?;
    run_generate_client_plan(&plan, transport, prompt, cancel_after)
}

pub fn run_generate_client_plan<T: Transport>(
    plan: &PreparedPlan,
    transport: &mut T,
    prompt: &str,
    cancel_after: Option<usize>,
) -> Result<ExecutionReport, RuntimeError> {
    if cancel_after == Some(0) {
        return Err(RuntimeError::Application(
            "cancel_after must be at least one token".to_string(),
        ));
    }
    let transport_plan = transport.plan().to_string();
    let mut machine = EndpointMachine::from_plan(plan, "client")?;
    let prompt = machine.emit_data(json!({ "text": prompt }))?;
    let mut trace = Vec::new();
    let mut tokens = Vec::new();
    if !send_and_record(&mut machine, transport, &prompt, &mut trace)? {
        return Ok(report(&machine, transport_plan, tokens, trace));
    }

    'conversation: while !machine.is_complete() {
        let Some(frame) = receive_and_record(&mut machine, transport, &mut trace)? else {
            break;
        };
        match frame {
            Frame::Select { label, .. } if label == "done" => {}
            Frame::Select { label, .. } if label == "token" => {
                let Some(token) = receive_and_record(&mut machine, transport, &mut trace)? else {
                    break;
                };
                let token_id = token_payload(&token)?;
                tokens.push(token_id);

                if cancel_after.is_some_and(|limit| tokens.len() >= limit) {
                    let selection = machine.emit_select("cancel")?;
                    if !send_and_record(&mut machine, transport, &selection, &mut trace)? {
                        break 'conversation;
                    }
                    let cancellation = machine.emit_cancel()?;
                    if !send_and_record(&mut machine, transport, &cancellation, &mut trace)? {
                        break 'conversation;
                    }
                } else {
                    let selection = machine.emit_select("continue")?;
                    if !send_and_record(&mut machine, transport, &selection, &mut trace)? {
                        break 'conversation;
                    }
                }
            }
            frame => {
                return Err(RuntimeError::Application(format!(
                    "client received unexpected frame {frame:?}"
                )));
            }
        }
    }

    if machine.is_successful() {
        transport.finish(machine.role())?;
    }
    Ok(report(&machine, transport_plan, tokens, trace))
}

pub fn run_generate_server<T: Transport>(
    conversation: &Conversation,
    transport: &mut T,
    token_limit: usize,
) -> Result<ExecutionReport, RuntimeError> {
    let plan = PreparedPlan::compile(conversation)?;
    run_generate_server_plan(&plan, transport, token_limit)
}

pub fn run_generate_server_plan<T: Transport>(
    plan: &PreparedPlan,
    transport: &mut T,
    token_limit: usize,
) -> Result<ExecutionReport, RuntimeError> {
    let transport_plan = transport.plan().to_string();
    let mut machine = EndpointMachine::from_plan(plan, "server")?;
    let mut trace = Vec::new();
    let Some(prompt) = receive_and_record(&mut machine, transport, &mut trace)? else {
        return Ok(report(&machine, transport_plan, Vec::new(), trace));
    };
    prompt_payload(&prompt)?;
    let mut tokens = Vec::new();

    'conversation: while !machine.is_complete() {
        if tokens.len() >= token_limit {
            let done = machine.emit_select("done")?;
            if !send_and_record(&mut machine, transport, &done, &mut trace)? {
                break;
            }
            continue;
        }

        let selection = machine.emit_select("token")?;
        if !send_and_record(&mut machine, transport, &selection, &mut trace)? {
            break;
        }
        let token_id = tokens.len() as u64 + 1;
        let token = machine.emit_data(json!({ "id": token_id }))?;
        if !send_and_record(&mut machine, transport, &token, &mut trace)? {
            break;
        }
        tokens.push(token_id);

        let Some(frame) = receive_and_record(&mut machine, transport, &mut trace)? else {
            break;
        };
        match frame {
            Frame::Select { label, .. } if label == "continue" => {}
            Frame::Select { label, .. } if label == "cancel" => {
                let Some(frame) = receive_and_record(&mut machine, transport, &mut trace)? else {
                    break 'conversation;
                };
                match frame {
                    Frame::Cancel { .. } => {}
                    frame => {
                        return Err(RuntimeError::Application(format!(
                            "server expected cancellation, received {frame:?}"
                        )));
                    }
                }
            }
            frame => {
                return Err(RuntimeError::Application(format!(
                    "server received unexpected frame {frame:?}"
                )));
            }
        }
    }

    if machine.is_successful() {
        transport.finish(machine.role())?;
    }
    Ok(report(&machine, transport_plan, tokens, trace))
}

pub fn run_memory_demo(
    conversation: &Conversation,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
) -> Result<DemoReport, RuntimeError> {
    let plan = PreparedPlan::compile(conversation)?;
    run_memory_plan_demo(&plan, prompt, token_limit, cancel_after)
}

pub fn run_memory_plan_demo(
    plan: &PreparedPlan,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
) -> Result<DemoReport, RuntimeError> {
    run_memory_plan_demo_with_encoding(
        plan,
        prompt,
        token_limit,
        cancel_after,
        WireEncoding::Reference,
    )
}

pub fn run_memory_plan_demo_with_encoding(
    plan: &PreparedPlan,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
    encoding: WireEncoding,
) -> Result<DemoReport, RuntimeError> {
    let (mut client_transport, mut server_transport) = match encoding {
        WireEncoding::Reference => memory_pair(),
        WireEncoding::Compact => memory_pair_compact(plan),
    };
    let prompt = prompt.to_string();
    let (client, server) = std::thread::scope(|scope| {
        let server_worker =
            scope.spawn(move || run_generate_server_plan(plan, &mut server_transport, token_limit));
        let client_worker = scope.spawn(move || {
            run_generate_client_plan(plan, &mut client_transport, &prompt, cancel_after)
        });
        let client = client_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        let server = server_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        Ok::<_, RuntimeError>((client, server))
    })?;
    let transport_plan = match encoding {
        WireEncoding::Reference => "memory",
        WireEncoding::Compact => "memory+compact",
    };
    Ok(demo_report(transport_plan, client, server))
}

pub fn run_memory_fault_demo(
    conversation: &Conversation,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
    fault: FaultPlan,
) -> Result<DemoReport, RuntimeError> {
    let plan = PreparedPlan::compile(conversation)?;
    run_memory_plan_fault_demo(&plan, prompt, token_limit, cancel_after, fault)
}

pub fn run_memory_plan_fault_demo(
    plan: &PreparedPlan,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
    fault: FaultPlan,
) -> Result<DemoReport, RuntimeError> {
    if plan.endpoint(&fault.role).is_none() {
        return Err(RuntimeError::UnknownRole(fault.role));
    }
    if !plan
        .endpoints()
        .iter()
        .flat_map(|endpoint| &endpoint.states)
        .any(|state| state.failure_target(&fault.failure).is_some())
    {
        return Err(RuntimeError::Application(format!(
            "fault plan references undeclared failure {}",
            fault.failure
        )));
    }
    if let Some(peer_failure) = &fault.peer_failure
        && !plan
            .endpoints()
            .iter()
            .flat_map(|endpoint| &endpoint.states)
            .any(|state| state.failure_target(peer_failure).is_some())
    {
        return Err(RuntimeError::Application(format!(
            "fault plan references undeclared peer failure {peer_failure}"
        )));
    }
    let fault_plan = fault.clone();
    let (client_transport, server_transport) = memory_pair();
    let fired = Arc::new(AtomicBool::new(false));
    let mut client_transport = FaultInjectingTransport::with_fired(
        client_transport,
        "client",
        fault.clone(),
        Arc::clone(&fired),
    )?;
    let mut server_transport =
        FaultInjectingTransport::with_fired(server_transport, "server", fault, Arc::clone(&fired))?;
    let prompt = prompt.to_string();
    let (client, server) = std::thread::scope(|scope| {
        let server_worker =
            scope.spawn(move || run_generate_server_plan(plan, &mut server_transport, token_limit));
        let client_worker = scope.spawn(move || {
            run_generate_client_plan(plan, &mut client_transport, &prompt, cancel_after)
        });
        let client = client_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        let server = server_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        Ok::<_, RuntimeError>((client, server))
    })?;
    if !fired.load(Ordering::Relaxed) {
        return Err(RuntimeError::Application(format!(
            "fault plan did not fire: {} {} occurrence {} was not reached",
            fault_plan.role,
            fault_plan.operation.as_str(),
            fault_plan.occurrence
        )));
    }
    let mut report = demo_report("memory+fault", client, server);
    report.fault_plan = Some(fault_plan);
    Ok(report)
}

pub fn run_tcp_demo(
    conversation: &Conversation,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
) -> Result<DemoReport, RuntimeError> {
    let plan = PreparedPlan::compile(conversation)?;
    run_tcp_plan_demo(&plan, prompt, token_limit, cancel_after)
}

pub fn run_tcp_plan_demo(
    plan: &PreparedPlan,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
) -> Result<DemoReport, RuntimeError> {
    run_tcp_plan_demo_with_encoding(
        plan,
        prompt,
        token_limit,
        cancel_after,
        WireEncoding::Reference,
    )
}

pub fn run_tcp_plan_demo_with_encoding(
    plan: &PreparedPlan,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
    encoding: WireEncoding,
) -> Result<DemoReport, RuntimeError> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let prompt = prompt.to_string();
    let (client, server) = std::thread::scope(|scope| {
        let server_worker = scope.spawn(move || {
            let (stream, _) = listener.accept()?;
            let mut transport = match encoding {
                WireEncoding::Reference => TcpTransport::from_stream(stream)?,
                WireEncoding::Compact => TcpTransport::from_stream_compact(stream, plan)?,
            };
            transport.establish_session(plan, "server", "client")?;
            run_generate_server_plan(plan, &mut transport, token_limit)
        });
        let client_worker = scope.spawn(move || {
            let mut transport = match encoding {
                WireEncoding::Reference => TcpTransport::connect(address)?,
                WireEncoding::Compact => TcpTransport::connect_compact(address, plan)?,
            };
            transport.establish_session(plan, "client", "server")?;
            run_generate_client_plan(plan, &mut transport, &prompt, cancel_after)
        });
        let client = client_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        let server = server_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        Ok::<_, RuntimeError>((client, server))
    })?;
    let transport_plan = match encoding {
        WireEncoding::Reference => "tcp",
        WireEncoding::Compact => "tcp+compact",
    };
    Ok(demo_report(transport_plan, client, server))
}

pub fn run_quic_demo(
    conversation: &Conversation,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
) -> Result<DemoReport, RuntimeError> {
    let plan = PreparedPlan::compile(conversation)?;
    run_quic_plan_demo(&plan, prompt, token_limit, cancel_after)
}

pub fn run_quic_plan_demo(
    plan: &PreparedPlan,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
) -> Result<DemoReport, RuntimeError> {
    run_quic_plan_demo_with_encoding(
        plan,
        prompt,
        token_limit,
        cancel_after,
        WireEncoding::Reference,
    )
}

pub fn run_quic_plan_demo_with_encoding(
    plan: &PreparedPlan,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
    encoding: WireEncoding,
) -> Result<DemoReport, RuntimeError> {
    let listener = QuicListener::bind("127.0.0.1:0".parse().expect("valid address"))?;
    let address = listener.local_addr()?;
    let trusted_certificate = listener.certificate_der().to_vec();
    let prompt = prompt.to_string();
    let (client, server) = std::thread::scope(|scope| {
        let server_worker = scope.spawn(move || {
            let mut transport = match encoding {
                WireEncoding::Reference => listener.accept()?,
                WireEncoding::Compact => listener.accept_compact(plan)?,
            };
            transport.establish_session(plan, "server", "client")?;
            run_generate_server_plan(plan, &mut transport, token_limit)
        });
        let client_worker = scope.spawn(move || {
            let mut transport = match encoding {
                WireEncoding::Reference => QuicTransport::connect(address, &trusted_certificate)?,
                WireEncoding::Compact => {
                    QuicTransport::connect_compact(address, &trusted_certificate, plan)?
                }
            };
            transport.establish_session(plan, "client", "server")?;
            run_generate_client_plan(plan, &mut transport, &prompt, cancel_after)
        });
        let client = client_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        let server = server_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        Ok::<_, RuntimeError>((client, server))
    })?;
    let transport_plan = match encoding {
        WireEncoding::Reference => "quic",
        WireEncoding::Compact => "quic+compact",
    };
    Ok(demo_report(transport_plan, client, server))
}

fn token_payload(frame: &Frame) -> Result<u64, RuntimeError> {
    match frame {
        Frame::Data {
            message,
            payload: Some(payload),
            ..
        } if message == "token" => payload.get("id").and_then(Value::as_u64).ok_or_else(|| {
            RuntimeError::Application("token payload is missing u64 id".to_string())
        }),
        _ => Err(RuntimeError::Application(format!(
            "expected token data, received {frame:?}"
        ))),
    }
}

fn prompt_payload(frame: &Frame) -> Result<&str, RuntimeError> {
    match frame {
        Frame::Data {
            message,
            payload: Some(payload),
            ..
        } if message == "prompt" => payload
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| RuntimeError::Application("prompt payload is missing text".to_string())),
        _ => Err(RuntimeError::Application(format!(
            "expected prompt data, received {frame:?}"
        ))),
    }
}

fn send_and_record<T: Transport>(
    machine: &mut EndpointMachine,
    transport: &mut T,
    envelope: &WireEnvelope,
    trace: &mut Vec<Frame>,
) -> Result<bool, RuntimeError> {
    match transport.send(envelope) {
        Ok(()) => {
            trace.push(envelope.frame.clone());
            Ok(true)
        }
        Err(error) => {
            let Some(failure) = error.transport_failure().map(str::to_string) else {
                return Err(error);
            };
            transport.abort();
            let fault = machine.observe_failure_at(&envelope.state, &failure)?;
            trace.push(fault);
            Ok(false)
        }
    }
}

fn receive_and_record<T: Transport>(
    machine: &mut EndpointMachine,
    transport: &mut T,
    trace: &mut Vec<Frame>,
) -> Result<Option<Frame>, RuntimeError> {
    match transport.receive() {
        Ok(envelope) => {
            let frame = machine.accept(envelope)?;
            trace.push(frame.clone());
            Ok(Some(frame))
        }
        Err(error) => {
            let Some(failure) = error.transport_failure().map(str::to_string) else {
                return Err(error);
            };
            transport.abort();
            let fault = machine.observe_failure(&failure)?;
            trace.push(fault);
            Ok(None)
        }
    }
}

fn trace_identity(trace: &[Frame]) -> String {
    let encoded = serde_json::to_vec(trace).expect("Eve frames always serialize");
    let digest = Sha256::digest(encoded);
    let mut identity = String::with_capacity(7 + digest.len() * 2);
    identity.push_str("sha256:");
    for byte in digest {
        write!(&mut identity, "{byte:02x}").expect("writing into a string cannot fail");
    }
    identity
}

fn report(
    machine: &EndpointMachine,
    transport: String,
    tokens: Vec<u64>,
    semantic_trace: Vec<Frame>,
) -> ExecutionReport {
    ExecutionReport {
        role: machine.role().to_string(),
        transport,
        conversation: machine.conversation().to_string(),
        conversation_identity: machine.identity().to_string(),
        plan_identity: machine.plan_identity().to_string(),
        semantic_trace_identity: trace_identity(&semantic_trace),
        frames: machine.sequence(),
        tokens,
        completed: machine.is_complete(),
        successful: machine.is_successful(),
        failure: machine.failure().map(str::to_string),
        final_state: machine.current_state().to_string(),
        semantic_trace,
    }
}

fn demo_report(
    transport_plan: &str,
    client: ExecutionReport,
    server: ExecutionReport,
) -> DemoReport {
    let semantic_trace_equivalent = client.semantic_trace == server.semantic_trace;
    let outcome_equivalent = client.completed
        && server.completed
        && client.successful == server.successful
        && client.failure == server.failure
        && client.final_state == server.final_state;
    DemoReport {
        transport_plan: transport_plan.to_string(),
        conversation_identity: client.conversation_identity.clone(),
        plan_identity: client.plan_identity.clone(),
        semantic_trace_equivalent,
        outcome_equivalent,
        fault_plan: None,
        client,
        server,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::EvePlan;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    // Each QUIC demo owns and tears down its own Tokio runtime and endpoints. Keep
    // those lifecycle tests from overlapping; independent connection concurrency belongs in a
    // long-lived runtime test rather than this one-conversation harness.
    fn quic_test_guard() -> MutexGuard<'static, ()> {
        static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
        GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn conversation() -> Conversation {
        serde_json::from_str(include_str!("../examples/generate.eveconv.json")).unwrap()
    }

    #[test]
    fn semantic_identity_ignores_projection_metadata() {
        let original = conversation();
        let mut edited = original.clone();
        edited.schema = Some("somewhere-else.json".to_string());
        edited
            .annotations
            .insert("editor".to_string(), Value::String("human".to_string()));
        assert_eq!(
            conversation_identity(&original).unwrap(),
            conversation_identity(&edited).unwrap()
        );
    }

    #[test]
    fn semantic_identity_changes_with_the_conversation() {
        let original = conversation();
        let mut edited = original.clone();
        if let crate::GlobalState::Send { deadline, .. } = &mut edited.states[0] {
            *deadline = Some("25ms".to_string());
        }
        assert_ne!(
            conversation_identity(&original).unwrap(),
            conversation_identity(&edited).unwrap()
        );
    }

    #[test]
    fn memory_plan_completes_through_explicit_cancellation() {
        let report = run_memory_demo(&conversation(), "hello", 5, Some(2)).unwrap();
        assert_eq!(report.transport_plan, "memory");
        assert_eq!(report.client.tokens, vec![1, 2]);
        assert_eq!(report.server.tokens, vec![1, 2]);
        assert!(report.client.completed);
        assert!(report.server.completed);
        assert!(report.semantic_trace_equivalent);
        assert_eq!(
            report.client.conversation_identity,
            report.server.conversation_identity
        );
    }

    #[test]
    fn plan_sessions_share_the_projected_endpoint_graph() {
        let artifact = EvePlan::compile(&conversation()).unwrap();
        let plan = artifact.prepare().unwrap();
        let first = EndpointMachine::from_plan(&plan, "client").unwrap();
        let second = EndpointMachine::from_plan(&plan, "client").unwrap();
        assert!(Arc::ptr_eq(&first.endpoint, &second.endpoint));
        assert_eq!(first.plan_identity(), plan.plan_identity());
    }

    #[test]
    fn compact_envelope_round_trips_to_the_same_semantic_frame() {
        let plan = PreparedPlan::compile(&conversation()).unwrap();
        let mut client = EndpointMachine::from_plan(&plan, "client").unwrap();
        let envelope = client.emit_data(json!({ "text": "hello" })).unwrap();
        let codec = EnvelopeCodec::for_plan(WireEncoding::Compact, &plan);
        let compact = codec.encode(&envelope).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&compact).unwrap(),
            json!({ "t": 7, "q": 0, "p": { "text": "hello" } })
        );
        assert!(compact.len() < encode_envelope(&envelope).unwrap().len());
        assert_eq!(codec.decode(&compact).unwrap(), envelope);
    }

    #[test]
    fn compact_codec_rejects_an_unknown_transition_id() {
        let plan = PreparedPlan::compile(&conversation()).unwrap();
        let codec = EnvelopeCodec::for_plan(WireEncoding::Compact, &plan);
        let error = codec.decode(br#"{"t":65535,"q":0}"#).unwrap_err();
        assert!(matches!(error, RuntimeError::CompactWire(_)));
    }

    #[test]
    fn session_preface_rejects_plan_role_and_wire_mismatches() {
        let plan = PreparedPlan::compile(&conversation()).unwrap();
        let mut peer = SessionPreface::for_plan(&plan, "server", WireEncoding::Compact).unwrap();

        peer.plan_identity =
            "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
        let error = peer
            .verify_peer(&plan, "server", WireEncoding::Compact)
            .unwrap_err();
        assert!(matches!(
            error,
            RuntimeError::SessionMismatch {
                field: "plan_identity",
                ..
            }
        ));

        let peer = SessionPreface::for_plan(&plan, "client", WireEncoding::Compact).unwrap();
        let error = peer
            .verify_peer(&plan, "server", WireEncoding::Compact)
            .unwrap_err();
        assert!(matches!(
            error,
            RuntimeError::SessionMismatch { field: "role", .. }
        ));

        let peer = SessionPreface::for_plan(&plan, "server", WireEncoding::Reference).unwrap();
        let error = peer
            .verify_peer(&plan, "server", WireEncoding::Compact)
            .unwrap_err();
        assert!(matches!(
            error,
            RuntimeError::SessionMismatch { field: "wire", .. }
        ));
    }

    #[test]
    fn session_preface_fixture_matches_the_compiled_plan() {
        let plan = PreparedPlan::compile(&conversation()).unwrap();
        let fixture: SessionPreface =
            serde_json::from_str(include_str!("../examples/session-preface.compact.json")).unwrap();
        assert_eq!(
            fixture,
            SessionPreface::for_plan(&plan, "client", WireEncoding::Compact).unwrap()
        );
        assert_eq!(encode_session_preface(&fixture).unwrap().len(), 278);
    }

    #[test]
    fn compact_tcp_rejects_frames_before_the_session_preface() {
        let plan = PreparedPlan::compile(&conversation()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client_stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server_stream, _) = listener.accept().unwrap();
        drop(server_stream);
        let mut transport = TcpTransport::from_stream_compact(client_stream, &plan).unwrap();
        let mut client = EndpointMachine::from_plan(&plan, "client").unwrap();
        let envelope = client.emit_data(json!({ "text": "blocked" })).unwrap();
        let error = transport.send(&envelope).unwrap_err();
        assert!(matches!(error, RuntimeError::SessionRequired("TCP")));
        assert!(!transport.session_established());
    }

    #[test]
    fn compact_memory_preserves_explicit_cancellation() {
        let plan = PreparedPlan::compile(&conversation()).unwrap();
        let report = run_memory_plan_demo_with_encoding(
            &plan,
            "cancel compactly",
            5,
            Some(2),
            WireEncoding::Compact,
        )
        .unwrap();
        assert_eq!(report.transport_plan, "memory+compact");
        assert_eq!(report.client.tokens, vec![1, 2]);
        assert_eq!(report.server.tokens, vec![1, 2]);
        assert!(report.semantic_trace_equivalent);
        assert!(report.outcome_equivalent);
    }

    #[test]
    fn deterministic_transport_fault_reaches_typed_failure_on_both_roles() {
        let report = run_memory_fault_demo(
            &conversation(),
            "hello",
            5,
            None,
            FaultPlan {
                role: "server".to_string(),
                operation: FaultOperation::Send,
                occurrence: 2,
                failure: "transport.closed".to_string(),
                peer_failure: None,
            },
        )
        .unwrap();
        assert!(report.client.completed);
        assert!(report.server.completed);
        assert!(!report.client.successful);
        assert!(!report.server.successful);
        assert_eq!(report.client.failure.as_deref(), Some("transport.closed"));
        assert_eq!(report.server.failure.as_deref(), Some("transport.closed"));
        assert!(report.outcome_equivalent);
        assert!(!report.semantic_trace_equivalent);
        assert_eq!(
            report.fault_plan.as_ref().map(|plan| plan.occurrence),
            Some(2)
        );
    }

    #[test]
    fn deterministic_receive_fault_reaches_typed_failure_on_both_roles() {
        let report = run_memory_fault_demo(
            &conversation(),
            "hello",
            5,
            None,
            FaultPlan {
                role: "client".to_string(),
                operation: FaultOperation::Receive,
                occurrence: 2,
                failure: "transport.closed".to_string(),
                peer_failure: None,
            },
        )
        .unwrap();
        assert!(report.client.completed);
        assert!(report.server.completed);
        assert!(report.outcome_equivalent);
        assert_eq!(report.client.failure, report.server.failure);
    }

    #[test]
    fn asymmetric_fault_preserves_each_endpoints_local_truth() {
        let report = run_memory_fault_demo(
            &conversation(),
            "hello",
            5,
            None,
            FaultPlan {
                role: "server".to_string(),
                operation: FaultOperation::Send,
                occurrence: 2,
                failure: "transport.timeout".to_string(),
                peer_failure: Some("transport.uncertain".to_string()),
            },
        )
        .unwrap();
        assert_eq!(report.server.failure.as_deref(), Some("transport.timeout"));
        assert_eq!(
            report.client.failure.as_deref(),
            Some("transport.uncertain")
        );
        assert_eq!(report.server.final_state, "transport-timeout");
        assert_eq!(report.client.final_state, "transport-uncertain");
        assert!(!report.outcome_equivalent);
        assert!(!report.semantic_trace_equivalent);
    }

    #[test]
    fn io_errors_map_to_specific_declared_failures() {
        let cases = [
            (std::io::ErrorKind::TimedOut, "transport.timeout"),
            (std::io::ErrorKind::ConnectionReset, "transport.reset"),
            (
                std::io::ErrorKind::ConnectionRefused,
                "transport.unreachable",
            ),
            (std::io::ErrorKind::UnexpectedEof, "transport.closed"),
        ];
        for (kind, expected) in cases {
            let error = RuntimeError::Io(std::io::Error::from(kind));
            assert_eq!(error.transport_failure(), Some(expected));
        }
    }

    #[test]
    fn fault_demo_rejects_an_unreached_occurrence() {
        let error = run_memory_fault_demo(
            &conversation(),
            "hello",
            1,
            None,
            FaultPlan {
                role: "server".to_string(),
                operation: FaultOperation::Send,
                occurrence: 99,
                failure: "transport.closed".to_string(),
                peer_failure: None,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("fault plan did not fire"));
    }

    #[test]
    fn tcp_plan_runs_the_same_conversation_to_completion() {
        let report = run_tcp_demo(&conversation(), "hello", 3, None).unwrap();
        assert_eq!(report.transport_plan, "tcp");
        assert_eq!(report.client.tokens, vec![1, 2, 3]);
        assert_eq!(report.server.tokens, vec![1, 2, 3]);
        assert!(report.client.completed);
        assert!(report.server.completed);
        assert!(report.semantic_trace_equivalent);
    }

    #[test]
    fn quic_plan_runs_the_same_conversation_to_completion() {
        let _guard = quic_test_guard();
        let report = run_quic_demo(&conversation(), "hello", 3, None).unwrap();
        assert_eq!(report.transport_plan, "quic");
        assert_eq!(report.client.tokens, vec![1, 2, 3]);
        assert_eq!(report.server.tokens, vec![1, 2, 3]);
        assert!(report.client.completed);
        assert!(report.server.completed);
        assert!(report.semantic_trace_equivalent);
    }

    #[test]
    fn quic_rejects_an_untrusted_server_certificate() {
        let _guard = quic_test_guard();
        let listener = QuicListener::bind("127.0.0.1:0".parse().expect("valid address")).unwrap();
        let address = listener.local_addr().unwrap();
        let untrusted_listener =
            QuicListener::bind("127.0.0.1:0".parse().expect("valid address")).unwrap();
        let untrusted_certificate = untrusted_listener.certificate_der().to_vec();
        drop(untrusted_listener);

        std::thread::scope(|scope| {
            let server = scope.spawn(move || listener.accept());
            let client = QuicTransport::connect(address, &untrusted_certificate);
            assert!(matches!(client, Err(RuntimeError::Quic(_))));
            assert!(server.join().unwrap().is_err());
        });
    }

    #[test]
    fn authenticated_quic_preface_rejects_a_different_plan() {
        let _guard = quic_test_guard();
        let server_plan = PreparedPlan::compile(&conversation()).unwrap();
        let mut edited = conversation();
        if let crate::GlobalState::Send { deadline, .. } = &mut edited.states[0] {
            *deadline = Some("25ms".to_string());
        }
        let client_plan = PreparedPlan::compile(&edited).unwrap();
        let listener = QuicListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let address = listener.local_addr().unwrap();
        let certificate = listener.certificate_der().to_vec();

        std::thread::scope(|scope| {
            let server = scope.spawn(move || {
                let mut transport = listener.accept_compact(&server_plan)?;
                transport.establish_session(&server_plan, "server", "client")
            });
            let client = scope.spawn(move || {
                let mut transport =
                    QuicTransport::connect_compact(address, &certificate, &client_plan)?;
                transport.establish_session(&client_plan, "client", "server")
            });
            let client = client.join().unwrap();
            let server = server.join().unwrap();
            assert!(client.is_err());
            assert!(server.is_err());
            assert!(
                matches!(client, Err(RuntimeError::SessionMismatch { .. }))
                    || matches!(server, Err(RuntimeError::SessionMismatch { .. }))
            );
        });
    }

    #[test]
    fn all_transport_plans_preserve_the_same_semantic_trace() {
        let _guard = quic_test_guard();
        let memory = run_memory_demo(&conversation(), "same input", 3, None).unwrap();
        let tcp = run_tcp_demo(&conversation(), "same input", 3, None).unwrap();
        let quic = run_quic_demo(&conversation(), "same input", 3, None).unwrap();
        assert_eq!(memory.conversation_identity, tcp.conversation_identity);
        assert_eq!(memory.conversation_identity, quic.conversation_identity);
        assert_eq!(memory.plan_identity, tcp.plan_identity);
        assert_eq!(memory.plan_identity, quic.plan_identity);
        assert_eq!(
            memory.client.semantic_trace_identity,
            tcp.client.semantic_trace_identity
        );
        assert_eq!(
            memory.client.semantic_trace_identity,
            quic.client.semantic_trace_identity
        );
        assert_eq!(memory.client.semantic_trace, tcp.client.semantic_trace);
        assert_eq!(memory.client.semantic_trace, quic.client.semantic_trace);
    }

    #[test]
    fn compact_wire_preserves_reference_semantics_on_every_transport() {
        let _guard = quic_test_guard();
        let plan = PreparedPlan::compile(&conversation()).unwrap();
        let reference = run_memory_plan_demo(&plan, "same compact input", 3, None).unwrap();
        let compact_memory = run_memory_plan_demo_with_encoding(
            &plan,
            "same compact input",
            3,
            None,
            WireEncoding::Compact,
        )
        .unwrap();
        let compact_tcp = run_tcp_plan_demo_with_encoding(
            &plan,
            "same compact input",
            3,
            None,
            WireEncoding::Compact,
        )
        .unwrap();
        let compact_quic = run_quic_plan_demo_with_encoding(
            &plan,
            "same compact input",
            3,
            None,
            WireEncoding::Compact,
        )
        .unwrap();

        for compact in [&compact_memory, &compact_tcp, &compact_quic] {
            assert!(compact.semantic_trace_equivalent);
            assert!(compact.outcome_equivalent);
            assert_eq!(
                compact.client.semantic_trace_identity,
                reference.client.semantic_trace_identity
            );
            assert_eq!(
                compact.client.semantic_trace,
                reference.client.semantic_trace
            );
        }
        assert_eq!(compact_memory.transport_plan, "memory+compact");
        assert_eq!(compact_tcp.transport_plan, "tcp+compact");
        assert_eq!(compact_quic.transport_plan, "quic+compact");
    }

    #[test]
    fn endpoint_rejects_a_different_semantic_graph() {
        let conversation = conversation();
        let mut client = EndpointMachine::for_role(&conversation, "client").unwrap();
        let mut server = EndpointMachine::for_role(&conversation, "server").unwrap();
        let mut envelope = client.emit_data(json!({ "text": "hello" })).unwrap();
        envelope.conversation_identity = "sha256:different".to_string();
        let error = server.accept(envelope).unwrap_err();
        assert!(matches!(error, RuntimeError::IdentityMismatch { .. }));
    }

    #[test]
    fn endpoint_rejects_a_frame_for_the_wrong_state() {
        let conversation = conversation();
        let mut client = EndpointMachine::for_role(&conversation, "client").unwrap();
        let mut server = EndpointMachine::for_role(&conversation, "server").unwrap();
        let prompt = client.emit_data(json!({ "text": "hello" })).unwrap();
        server.accept(prompt).unwrap();
        let wrong = WireEnvelope {
            eve_wire: WIRE_FORMAT.to_string(),
            conversation: server.conversation().to_string(),
            conversation_identity: server.identity().to_string(),
            state: server.current_state().to_string(),
            sequence: server.sequence(),
            frame: Frame::Data {
                from: "client".to_string(),
                to: "server".to_string(),
                message: "token".to_string(),
                payload: Some(json!({ "id": 1 })),
            },
        };
        let error = server.accept(wrong).unwrap_err();
        assert!(matches!(error, RuntimeError::Protocol { .. }));
    }
}
