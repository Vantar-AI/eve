use crate::{Conversation, Endpoint, EndpointState, Frame, ValidationErrors, project, validate};
use rcgen::CertifiedKey;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc,
    mpsc::{self, Receiver, Sender},
};
use std::time::Duration;
use thiserror::Error;

pub const WIRE_FORMAT: &str = "0.1.0";
const MAX_ENVELOPE_BYTES: usize = 16 * 1024 * 1024;

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

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    InvalidConversation(#[from] ValidationErrors),
    #[error("conversation has no projected endpoint for role {0}")]
    UnknownRole(String),
    #[error("wire codec error: {0}")]
    Codec(#[from] serde_json::Error),
    #[error("transport I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0} transport closed before the conversation completed")]
    TransportClosed(&'static str),
    #[error("wire envelope is {actual} bytes; the limit is {limit} bytes")]
    EnvelopeTooLarge { actual: usize, limit: usize },
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
}

pub fn conversation_identity(conversation: &Conversation) -> Result<String, RuntimeError> {
    validate(conversation)?;
    let mut semantic = conversation.clone();
    semantic.schema = None;
    semantic.annotations.clear();
    let encoded = serde_json::to_vec(&semantic)?;
    let digest = Sha256::digest(encoded);
    let mut identity = String::with_capacity(7 + digest.len() * 2);
    identity.push_str("sha256:");
    for byte in digest {
        write!(&mut identity, "{byte:02x}").expect("writing into a string cannot fail");
    }
    Ok(identity)
}

#[derive(Debug)]
pub struct EndpointMachine {
    endpoint: Endpoint,
    conversation_identity: String,
    current: String,
    sequence: u64,
}

impl EndpointMachine {
    pub fn for_role(conversation: &Conversation, role: &str) -> Result<Self, RuntimeError> {
        let identity = conversation_identity(conversation)?;
        let endpoint = project(conversation)?
            .into_iter()
            .find(|candidate| candidate.role == role)
            .ok_or_else(|| RuntimeError::UnknownRole(role.to_string()))?;
        let current = endpoint.initial.clone();
        Ok(Self {
            endpoint,
            conversation_identity: identity,
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

    pub fn current_state(&self) -> &str {
        &self.current
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn is_complete(&self) -> bool {
        matches!(self.state(), EndpointState::End { .. })
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
}

pub struct MemoryTransport {
    outbound: Sender<Vec<u8>>,
    inbound: Receiver<Vec<u8>>,
}

pub fn memory_pair() -> (MemoryTransport, MemoryTransport) {
    let (client_to_server_tx, client_to_server_rx) = mpsc::channel();
    let (server_to_client_tx, server_to_client_rx) = mpsc::channel();
    (
        MemoryTransport {
            outbound: client_to_server_tx,
            inbound: server_to_client_rx,
        },
        MemoryTransport {
            outbound: server_to_client_tx,
            inbound: client_to_server_rx,
        },
    )
}

impl Transport for MemoryTransport {
    fn plan(&self) -> &'static str {
        "memory"
    }

    fn send(&mut self, envelope: &WireEnvelope) -> Result<(), RuntimeError> {
        let encoded = encode_envelope(envelope)?;
        self.outbound
            .send(encoded)
            .map_err(|_| RuntimeError::TransportClosed("memory"))
    }

    fn receive(&mut self) -> Result<WireEnvelope, RuntimeError> {
        let encoded = self
            .inbound
            .recv()
            .map_err(|_| RuntimeError::TransportClosed("memory"))?;
        decode_envelope(&encoded)
    }
}

pub struct TcpTransport {
    stream: TcpStream,
}

impl TcpTransport {
    pub fn connect(address: SocketAddr) -> Result<Self, RuntimeError> {
        Self::from_stream(TcpStream::connect(address)?)
    }

    pub fn from_stream(stream: TcpStream) -> Result<Self, RuntimeError> {
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        Ok(Self { stream })
    }
}

impl Transport for TcpTransport {
    fn plan(&self) -> &'static str {
        "tcp"
    }

    fn send(&mut self, envelope: &WireEnvelope) -> Result<(), RuntimeError> {
        let encoded = encode_envelope(envelope)?;
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
        decode_envelope(&encoded)
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
            _connection: connection,
            send,
            receive,
            finished: false,
            runtime,
        })
    }
}

pub struct QuicTransport {
    _endpoint: quinn::Endpoint,
    _connection: quinn::Connection,
    send: quinn::SendStream,
    receive: quinn::RecvStream,
    finished: bool,
    runtime: tokio::runtime::Runtime,
}

impl QuicTransport {
    pub fn connect(address: SocketAddr, trusted_certificate: &[u8]) -> Result<Self, RuntimeError> {
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
            _connection: connection,
            send,
            receive,
            finished: false,
            runtime,
        })
    }
}

impl Transport for QuicTransport {
    fn plan(&self) -> &'static str {
        "quic"
    }

    fn send(&mut self, envelope: &WireEnvelope) -> Result<(), RuntimeError> {
        let encoded = encode_envelope(envelope)?;
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
        decode_envelope(&encoded)
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
    pub semantic_trace_identity: String,
    pub frames: u64,
    pub tokens: Vec<u64>,
    pub completed: bool,
    pub final_state: String,
    #[serde(skip_serializing)]
    pub semantic_trace: Vec<Frame>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DemoReport {
    pub transport_plan: String,
    pub conversation_identity: String,
    pub semantic_trace_equivalent: bool,
    pub client: ExecutionReport,
    pub server: ExecutionReport,
}

pub fn run_generate_client<T: Transport>(
    conversation: &Conversation,
    transport: &mut T,
    prompt: &str,
    cancel_after: Option<usize>,
) -> Result<ExecutionReport, RuntimeError> {
    if cancel_after == Some(0) {
        return Err(RuntimeError::Application(
            "cancel_after must be at least one token".to_string(),
        ));
    }
    let plan = transport.plan().to_string();
    let mut machine = EndpointMachine::for_role(conversation, "client")?;
    let prompt = machine.emit_data(json!({ "text": prompt }))?;
    let mut trace = Vec::new();
    send_and_record(transport, &prompt, &mut trace)?;
    let mut tokens = Vec::new();

    while !machine.is_complete() {
        let frame = receive_and_record(&mut machine, transport, &mut trace)?;
        match frame {
            Frame::Select { label, .. } if label == "done" => {}
            Frame::Select { label, .. } if label == "token" => {
                let token = receive_and_record(&mut machine, transport, &mut trace)?;
                let token_id = token_payload(&token)?;
                tokens.push(token_id);

                if cancel_after.is_some_and(|limit| tokens.len() >= limit) {
                    let selection = machine.emit_select("cancel")?;
                    send_and_record(transport, &selection, &mut trace)?;
                    let cancellation = machine.emit_cancel()?;
                    send_and_record(transport, &cancellation, &mut trace)?;
                } else {
                    let selection = machine.emit_select("continue")?;
                    send_and_record(transport, &selection, &mut trace)?;
                }
            }
            frame => {
                return Err(RuntimeError::Application(format!(
                    "client received unexpected frame {frame:?}"
                )));
            }
        }
    }

    transport.finish(machine.role())?;
    Ok(report(&machine, plan, tokens, trace))
}

pub fn run_generate_server<T: Transport>(
    conversation: &Conversation,
    transport: &mut T,
    token_limit: usize,
) -> Result<ExecutionReport, RuntimeError> {
    let plan = transport.plan().to_string();
    let mut machine = EndpointMachine::for_role(conversation, "server")?;
    let mut trace = Vec::new();
    let prompt = receive_and_record(&mut machine, transport, &mut trace)?;
    prompt_payload(&prompt)?;
    let mut tokens = Vec::new();

    while !machine.is_complete() {
        if tokens.len() >= token_limit {
            let done = machine.emit_select("done")?;
            send_and_record(transport, &done, &mut trace)?;
            continue;
        }

        let selection = machine.emit_select("token")?;
        send_and_record(transport, &selection, &mut trace)?;
        let token_id = tokens.len() as u64 + 1;
        let token = machine.emit_data(json!({ "id": token_id }))?;
        send_and_record(transport, &token, &mut trace)?;
        tokens.push(token_id);

        match receive_and_record(&mut machine, transport, &mut trace)? {
            Frame::Select { label, .. } if label == "continue" => {}
            Frame::Select { label, .. } if label == "cancel" => {
                match receive_and_record(&mut machine, transport, &mut trace)? {
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

    transport.finish(machine.role())?;
    Ok(report(&machine, plan, tokens, trace))
}

pub fn run_memory_demo(
    conversation: &Conversation,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
) -> Result<DemoReport, RuntimeError> {
    let (mut client_transport, mut server_transport) = memory_pair();
    let client_conversation = conversation.clone();
    let server_conversation = conversation.clone();
    let prompt = prompt.to_string();
    let (client, server) = std::thread::scope(|scope| {
        let server_worker = scope.spawn(move || {
            run_generate_server(&server_conversation, &mut server_transport, token_limit)
        });
        let client_worker = scope.spawn(move || {
            run_generate_client(
                &client_conversation,
                &mut client_transport,
                &prompt,
                cancel_after,
            )
        });
        let client = client_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        let server = server_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        Ok::<_, RuntimeError>((client, server))
    })?;
    Ok(demo_report("memory", client, server))
}

pub fn run_tcp_demo(
    conversation: &Conversation,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
) -> Result<DemoReport, RuntimeError> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let client_conversation = conversation.clone();
    let server_conversation = conversation.clone();
    let prompt = prompt.to_string();
    let (client, server) = std::thread::scope(|scope| {
        let server_worker = scope.spawn(move || {
            let (stream, _) = listener.accept()?;
            let mut transport = TcpTransport::from_stream(stream)?;
            run_generate_server(&server_conversation, &mut transport, token_limit)
        });
        let client_worker = scope.spawn(move || {
            let mut transport = TcpTransport::connect(address)?;
            run_generate_client(&client_conversation, &mut transport, &prompt, cancel_after)
        });
        let client = client_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        let server = server_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        Ok::<_, RuntimeError>((client, server))
    })?;
    Ok(demo_report("tcp", client, server))
}

pub fn run_quic_demo(
    conversation: &Conversation,
    prompt: &str,
    token_limit: usize,
    cancel_after: Option<usize>,
) -> Result<DemoReport, RuntimeError> {
    let listener = QuicListener::bind("127.0.0.1:0".parse().expect("valid address"))?;
    let address = listener.local_addr()?;
    let trusted_certificate = listener.certificate_der().to_vec();
    let client_conversation = conversation.clone();
    let server_conversation = conversation.clone();
    let prompt = prompt.to_string();
    let (client, server) = std::thread::scope(|scope| {
        let server_worker = scope.spawn(move || {
            let mut transport = listener.accept()?;
            run_generate_server(&server_conversation, &mut transport, token_limit)
        });
        let client_worker = scope.spawn(move || {
            let mut transport = QuicTransport::connect(address, &trusted_certificate)?;
            run_generate_client(&client_conversation, &mut transport, &prompt, cancel_after)
        });
        let client = client_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        let server = server_worker
            .join()
            .map_err(|_| RuntimeError::WorkerPanicked)??;
        Ok::<_, RuntimeError>((client, server))
    })?;
    Ok(demo_report("quic", client, server))
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
    transport: &mut T,
    envelope: &WireEnvelope,
    trace: &mut Vec<Frame>,
) -> Result<(), RuntimeError> {
    transport.send(envelope)?;
    trace.push(envelope.frame.clone());
    Ok(())
}

fn receive_and_record<T: Transport>(
    machine: &mut EndpointMachine,
    transport: &mut T,
    trace: &mut Vec<Frame>,
) -> Result<Frame, RuntimeError> {
    let frame = machine.accept(transport.receive()?)?;
    trace.push(frame.clone());
    Ok(frame)
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
        semantic_trace_identity: trace_identity(&semantic_trace),
        frames: machine.sequence(),
        tokens,
        completed: machine.is_complete(),
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
    DemoReport {
        transport_plan: transport_plan.to_string(),
        conversation_identity: client.conversation_identity.clone(),
        semantic_trace_equivalent,
        client,
        server,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn all_transport_plans_preserve_the_same_semantic_trace() {
        let memory = run_memory_demo(&conversation(), "same input", 3, None).unwrap();
        let tcp = run_tcp_demo(&conversation(), "same input", 3, None).unwrap();
        let quic = run_quic_demo(&conversation(), "same input", 3, None).unwrap();
        assert_eq!(memory.conversation_identity, tcp.conversation_identity);
        assert_eq!(memory.conversation_identity, quic.conversation_identity);
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
