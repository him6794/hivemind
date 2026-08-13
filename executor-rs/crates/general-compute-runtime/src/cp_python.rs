use crate::differential::ReferenceObservation;
use crate::supervisor::{Cancellation, CommandSpec, RunStatus, Supervisor, SupervisorError};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonBackendRegistration {
    pub backend_id: String,
    pub executable: String,
    pub runtime_version: String,
    pub guest_image_digest: String,
    pub protocol_version: String,
    pub max_output_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PythonRegistryError {
    EmptyBackendId,
    EmptyExecutable,
    InvalidImageDigest,
    EmptyProtocolVersion,
    ZeroOutputLimit,
    UnsafeExecutable,
    DuplicateBackend(String),
}

impl fmt::Display for PythonRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBackendId => formatter.write_str("python backend id must not be empty"),
            Self::EmptyExecutable => formatter.write_str("python executable must not be empty"),
            Self::InvalidImageDigest => formatter.write_str("python image digest must be sha256 pinned"),
            Self::EmptyProtocolVersion => formatter.write_str("python protocol version must not be empty"),
            Self::ZeroOutputLimit => formatter.write_str("python output limit must be positive"),
            Self::UnsafeExecutable => formatter.write_str("python backend executable is not a registry-safe binary"),
            Self::DuplicateBackend(id) => write!(formatter, "duplicate python backend {id}"),
        }
    }
}

impl std::error::Error for PythonRegistryError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonBackendRegistry {
    backends: BTreeMap<String, PythonBackendRegistration>,
}

impl PythonBackendRegistry {
    pub fn new(registrations: Vec<PythonBackendRegistration>) -> Result<Self, PythonRegistryError> {
        let mut backends = BTreeMap::new();
        for registration in registrations {
            validate_registration(&registration)?;
            if backends.insert(registration.backend_id.clone(), registration).is_some() {
                let id = backends.keys().next_back().cloned().unwrap_or_default();
                return Err(PythonRegistryError::DuplicateBackend(id));
            }
        }
        Ok(Self { backends })
    }

    fn get(&self, backend_id: &str) -> Option<&PythonBackendRegistration> {
        self.backends.get(backend_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PythonAdapterError {
    BackendUnavailable { backend_id: String },
    MalformedObservation(String),
    Supervisor(String),
    Protocol(String),
}

impl fmt::Display for PythonAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BackendUnavailable { backend_id } => write!(formatter, "python backend {backend_id} is unavailable"),
            Self::MalformedObservation(message) => write!(formatter, "malformed python observation: {message}"),
            Self::Supervisor(message) => write!(formatter, "python supervisor failed: {message}"),
            Self::Protocol(message) => write!(formatter, "python protocol failed: {message}"),
        }
    }
}

impl std::error::Error for PythonAdapterError {}

#[derive(Debug, Clone)]
pub struct PinnedPythonAdapter {
    registration: PythonBackendRegistration,
}

impl PinnedPythonAdapter {
    pub fn from_registry(registry: &PythonBackendRegistry, backend_id: &str) -> Result<Self, PythonAdapterError> {
        let Some(registration) = registry.get(backend_id) else {
            return Err(PythonAdapterError::BackendUnavailable {
                backend_id: backend_id.into(),
            });
        };
        Ok(Self {
            registration: registration.clone(),
        })
    }

    pub fn registration(&self) -> &PythonBackendRegistration {
        &self.registration
    }

    pub fn parse_observation(&self, bytes: &[u8]) -> Result<ReferenceObservation, PythonAdapterError> {
        let observation: StrictObservation = serde_json::from_slice(bytes)
            .map_err(|error| PythonAdapterError::MalformedObservation(error.to_string()))?;
        if !matches!(
            observation.status.as_str(),
            "halted" | "exception" | "exited" | "resource_exhausted" | "cancelled"
        ) {
            return Err(PythonAdapterError::MalformedObservation("unknown status".into()));
        }
        if observation.output.len() > self.registration.max_output_bytes {
            return Err(PythonAdapterError::MalformedObservation(
                "output exceeds registered cap".into(),
            ));
        }
        Ok(ReferenceObservation {
            status: observation.status,
            steps: observation.steps,
            output: observation.output,
        })
    }

    pub fn parse_framed_observation(&self, bytes: &[u8]) -> Result<ReferenceObservation, PythonAdapterError> {
        let (observation, consumed) =
            crate::decode_frame::<serde_json::Value>(bytes, self.registration.max_output_bytes)
                .map_err(|error| PythonAdapterError::Protocol(format!("response frame: {error:?}")))?;
        if consumed != bytes.len() {
            return Err(PythonAdapterError::Protocol("response contains trailing bytes".into()));
        }
        let observation =
            serde_json::to_vec(&observation).map_err(|error| PythonAdapterError::Protocol(error.to_string()))?;
        self.parse_observation(&observation)
    }

    pub fn execute(
        &self,
        source: &str,
        input_json: &str,
        seed: u64,
        cancellation: &Cancellation,
    ) -> Result<ReferenceObservation, PythonAdapterError> {
        self.execute_with_timeout(source, input_json, seed, Duration::from_secs(30), cancellation)
    }

    pub fn execute_with_timeout(
        &self,
        source: &str,
        input_json: &str,
        seed: u64,
        timeout: Duration,
        cancellation: &Cancellation,
    ) -> Result<ReferenceObservation, PythonAdapterError> {
        let request = serde_json::json!({
            "source": source,
            "input_json": input_json,
            "seed": seed,
        });
        let input = crate::encode_frame(&request, self.registration.max_output_bytes)
            .map_err(|error| PythonAdapterError::Protocol(format!("request frame: {error:?}")))?;
        let command = CommandSpec::new(&self.registration.executable, ["-c", PYTHON_RUNNER])
            .with_input_limit(input.len())
            .with_timeout(timeout)
            .with_output_limit(self.registration.max_output_bytes);
        let result = Supervisor::new()
            .run_with_stdin(command, &input, cancellation)
            .map_err(|error| PythonAdapterError::Supervisor(supervisor_error_message(error)))?;
        if result.status != RunStatus::Completed {
            let status = match result.status {
                RunStatus::TimedOut => "timed out",
                RunStatus::Cancelled => "cancelled",
                other => return Err(PythonAdapterError::Supervisor(format!("child ended as {other:?}"))),
            };
            return Err(PythonAdapterError::Supervisor(status.into()));
        }
        if result.stdout_truncated {
            return Err(PythonAdapterError::MalformedObservation(
                "stdout frame was truncated".into(),
            ));
        }
        self.parse_framed_observation(&result.stdout)
    }
}

const PYTHON_RUNNER: &str = r#"
import json, struct, sys
frame = sys.stdin.buffer.read()
if len(frame) < 4:
    raise SystemExit(2)
size = struct.unpack('>I', frame[:4])[0]
payload = frame[4:4 + size]
if len(payload) != size:
    raise SystemExit(3)
request = json.loads(payload)
scope = {'input': json.loads(request['input_json']), 'seed': request['seed']}
try:
    safe_globals = {'__builtins__': {}, 'ValueError': ValueError, 'Exception': Exception}
    exec(request['source'], safe_globals, scope)
    status = 'halted'
    output = str(scope.get('result', ''))
except Exception as error:
    status = 'exception'
    output = f'{type(error).__name__}: {error}'
response = json.dumps({'status': status, 'steps': 1, 'output': output}, separators=(',', ':')).encode()
sys.stdout.buffer.write(struct.pack('>I', len(response)) + response)
sys.stdout.buffer.flush()
"#;

fn supervisor_error_message(error: SupervisorError) -> String {
    error.to_string()
}

fn validate_registration(registration: &PythonBackendRegistration) -> Result<(), PythonRegistryError> {
    if registration.backend_id.trim().is_empty() {
        return Err(PythonRegistryError::EmptyBackendId);
    }
    if registration.executable.trim().is_empty() {
        return Err(PythonRegistryError::EmptyExecutable);
    }
    let executable = std::path::Path::new(&registration.executable);
    let basename = executable
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let has_shell_metacharacter = registration
        .executable
        .chars()
        .any(|character| matches!(character, ' ' | '\t' | '\r' | '\n' | ';' | '|' | '&' | '$' | '`' | '<' | '>'));
    if has_shell_metacharacter
        || basename.is_empty()
        || matches!(basename.as_str(), "sh" | "bash" | "dash" | "zsh" | "cmd.exe" | "powershell.exe" | "pwsh.exe" | "pwsh")
        || registration.executable.contains("..")
    {
        return Err(PythonRegistryError::UnsafeExecutable);
    }
    if !registration.guest_image_digest.starts_with("sha256:")
        || registration.guest_image_digest.len() != "sha256:".len() + 64
        || !registration.guest_image_digest["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(PythonRegistryError::InvalidImageDigest);
    }
    if registration.protocol_version.trim().is_empty() {
        return Err(PythonRegistryError::EmptyProtocolVersion);
    }
    if registration.max_output_bytes == 0 {
        return Err(PythonRegistryError::ZeroOutputLimit);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictObservation {
    status: String,
    steps: u64,
    output: String,
}
