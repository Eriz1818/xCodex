mod generated;

pub use generated::*;

use std::io;
use std::io::Read;
use std::path::Path;

use serde_json::Value;

#[derive(Debug)]
pub enum HookReadError {
    Io(io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for HookReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HookReadError::Io(err) => write!(f, "I/O error reading hook payload: {err}"),
            HookReadError::Json(err) => write!(f, "invalid JSON hook payload: {err}"),
        }
    }
}

impl std::error::Error for HookReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            HookReadError::Io(err) => Some(err),
            HookReadError::Json(err) => Some(err),
        }
    }
}

impl From<io::Error> for HookReadError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for HookReadError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HookEvent {
    AgentTurnComplete(AgentTurnCompletePayload),
    ApprovalRequested(ApprovalRequestedPayload),
    SessionStart(SessionStartPayload),
    SessionEnd(SessionEndPayload),
    ModelRequestStarted(ModelRequestStartedPayload),
    ModelResponseCompleted(ModelResponseCompletedPayload),
    ToolCallStarted(ToolCallStartedPayload),
    ToolCallFinished(ToolCallFinishedPayload),
    Unknown(UnknownHookEvent),
}

impl HookEvent {
    pub fn event_type(&self) -> &str {
        match self {
            HookEvent::AgentTurnComplete(payload) => &payload.event_type,
            HookEvent::ApprovalRequested(payload) => &payload.event_type,
            HookEvent::SessionStart(payload) => &payload.event_type,
            HookEvent::SessionEnd(payload) => &payload.event_type,
            HookEvent::ModelRequestStarted(payload) => &payload.event_type,
            HookEvent::ModelResponseCompleted(payload) => &payload.event_type,
            HookEvent::ToolCallStarted(payload) => &payload.event_type,
            HookEvent::ToolCallFinished(payload) => &payload.event_type,
            HookEvent::Unknown(payload) => payload.event_type.as_deref().unwrap_or(""),
        }
    }

    pub fn to_json_value(&self) -> Value {
        match self {
            HookEvent::AgentTurnComplete(payload) => {
                serde_json::to_value(payload).unwrap_or(Value::Null)
            }
            HookEvent::ApprovalRequested(payload) => {
                serde_json::to_value(payload).unwrap_or(Value::Null)
            }
            HookEvent::SessionStart(payload) => {
                serde_json::to_value(payload).unwrap_or(Value::Null)
            }
            HookEvent::SessionEnd(payload) => serde_json::to_value(payload).unwrap_or(Value::Null),
            HookEvent::ModelRequestStarted(payload) => {
                serde_json::to_value(payload).unwrap_or(Value::Null)
            }
            HookEvent::ModelResponseCompleted(payload) => {
                serde_json::to_value(payload).unwrap_or(Value::Null)
            }
            HookEvent::ToolCallStarted(payload) => {
                serde_json::to_value(payload).unwrap_or(Value::Null)
            }
            HookEvent::ToolCallFinished(payload) => {
                serde_json::to_value(payload).unwrap_or(Value::Null)
            }
            HookEvent::Unknown(payload) => payload.raw.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnknownHookEvent {
    pub event_type: Option<String>,
    pub raw: Value,
    pub extra: ExtraFields,
    pub parse_error: Option<String>,
}

pub fn read_event_from_stdin() -> Result<HookEvent, HookReadError> {
    read_event_from_reader(io::stdin())
}

pub fn read_event_from_reader<R: Read>(reader: R) -> Result<HookEvent, HookReadError> {
    let payload = read_payload_json_from_reader(reader)?;
    Ok(parse_event(payload))
}

pub fn read_payload_json_from_stdin() -> Result<Value, HookReadError> {
    read_payload_json_from_reader(io::stdin())
}

pub fn read_payload_json_from_reader<R: Read>(mut reader: R) -> Result<Value, HookReadError> {
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;

    let payload = if buf.is_empty() || buf.iter().all(u8::is_ascii_whitespace) {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_slice(&buf)?
    };

    resolve_payload_path_envelope(payload)
}

fn resolve_payload_path_envelope(payload: Value) -> Result<Value, HookReadError> {
    let payload_path = payload
        .as_object()
        .and_then(|obj| obj.get("payload-path"))
        .and_then(Value::as_str);

    if let Some(payload_path) = payload_path {
        let contents = std::fs::read_to_string(Path::new(payload_path))?;
        Ok(serde_json::from_str(&contents)?)
    } else {
        Ok(payload)
    }
}

pub fn parse_event(payload: Value) -> HookEvent {
    let event_type = payload
        .as_object()
        .and_then(|obj| obj.get("type"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let Some(event_type) = event_type else {
        return HookEvent::Unknown(unknown_from_value(None, payload, None));
    };

    match event_type.as_str() {
        "agent-turn-complete" => match parse_known::<AgentTurnCompletePayload>(&payload) {
            Ok(ev) => ev,
            Err(err) => HookEvent::Unknown(unknown_from_value(
                Some(event_type.clone()),
                payload,
                Some(err),
            )),
        },
        "approval-requested" => match parse_known::<ApprovalRequestedPayload>(&payload) {
            Ok(ev) => ev,
            Err(err) => HookEvent::Unknown(unknown_from_value(
                Some(event_type.clone()),
                payload,
                Some(err),
            )),
        },
        "session-start" => match parse_known::<SessionStartPayload>(&payload) {
            Ok(ev) => ev,
            Err(err) => HookEvent::Unknown(unknown_from_value(
                Some(event_type.clone()),
                payload,
                Some(err),
            )),
        },
        "session-end" => match parse_known::<SessionEndPayload>(&payload) {
            Ok(ev) => ev,
            Err(err) => HookEvent::Unknown(unknown_from_value(
                Some(event_type.clone()),
                payload,
                Some(err),
            )),
        },
        "model-request-started" => match parse_known::<ModelRequestStartedPayload>(&payload) {
            Ok(ev) => ev,
            Err(err) => HookEvent::Unknown(unknown_from_value(
                Some(event_type.clone()),
                payload,
                Some(err),
            )),
        },
        "model-response-completed" => {
            match parse_known::<ModelResponseCompletedPayload>(&payload) {
                Ok(ev) => ev,
                Err(err) => HookEvent::Unknown(unknown_from_value(
                    Some(event_type.clone()),
                    payload,
                    Some(err),
                )),
            }
        }
        "tool-call-started" => match parse_known::<ToolCallStartedPayload>(&payload) {
            Ok(ev) => ev,
            Err(err) => HookEvent::Unknown(unknown_from_value(
                Some(event_type.clone()),
                payload,
                Some(err),
            )),
        },
        "tool-call-finished" => match parse_known::<ToolCallFinishedPayload>(&payload) {
            Ok(ev) => ev,
            Err(err) => HookEvent::Unknown(unknown_from_value(
                Some(event_type.clone()),
                payload,
                Some(err),
            )),
        },
        _ => HookEvent::Unknown(unknown_from_value(Some(event_type), payload, None)),
    }
}

fn parse_known<T>(payload: &Value) -> Result<HookEvent, String>
where
    T: serde::de::DeserializeOwned + Into<HookEvent>,
{
    serde_json::from_value::<T>(payload.clone())
        .map(Into::into)
        .map_err(|err| err.to_string())
}

fn unknown_from_value(
    event_type: Option<String>,
    payload: Value,
    parse_error: Option<String>,
) -> UnknownHookEvent {
    let mut extra = ExtraFields::new();
    if let Some(obj) = payload.as_object() {
        for (k, v) in obj {
            if k == "event-id" || k == "schema-version" || k == "timestamp" || k == "type" {
                continue;
            }
            extra.insert(k.clone(), v.clone());
        }
    }

    UnknownHookEvent {
        event_type,
        raw: payload,
        extra,
        parse_error,
    }
}

impl From<AgentTurnCompletePayload> for HookEvent {
    fn from(value: AgentTurnCompletePayload) -> Self {
        Self::AgentTurnComplete(value)
    }
}

impl From<ApprovalRequestedPayload> for HookEvent {
    fn from(value: ApprovalRequestedPayload) -> Self {
        Self::ApprovalRequested(value)
    }
}

impl From<SessionStartPayload> for HookEvent {
    fn from(value: SessionStartPayload) -> Self {
        Self::SessionStart(value)
    }
}

impl From<SessionEndPayload> for HookEvent {
    fn from(value: SessionEndPayload) -> Self {
        Self::SessionEnd(value)
    }
}

impl From<ModelRequestStartedPayload> for HookEvent {
    fn from(value: ModelRequestStartedPayload) -> Self {
        Self::ModelRequestStarted(value)
    }
}

impl From<ModelResponseCompletedPayload> for HookEvent {
    fn from(value: ModelResponseCompletedPayload) -> Self {
        Self::ModelResponseCompleted(value)
    }
}

impl From<ToolCallStartedPayload> for HookEvent {
    fn from(value: ToolCallStartedPayload) -> Self {
        Self::ToolCallStarted(value)
    }
}

impl From<ToolCallFinishedPayload> for HookEvent {
    fn from(value: ToolCallFinishedPayload) -> Self {
        Self::ToolCallFinished(value)
    }
}
