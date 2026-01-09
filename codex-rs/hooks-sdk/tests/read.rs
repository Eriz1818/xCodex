use codex_hooks_sdk::HookEvent;
use codex_hooks_sdk::read_event_from_reader;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn tool_call_finished_payload(marker_key: &str, marker_value: &str) -> String {
    format!(
        "{{\"schema-version\":1,\"event-id\":\"e\",\"timestamp\":\"t\",\"type\":\"tool-call-finished\",\"thread-id\":\"th\",\"turn-id\":\"tu\",\"cwd\":\"/tmp\",\"model-request-id\":\"m\",\"attempt\":1,\"tool-name\":\"exec\",\"call-id\":\"c\",\"status\":\"completed\",\"duration-ms\":1,\"success\":true,\"output-bytes\":0,\"{marker_key}\":\"{marker_value}\"}}"
    )
}

#[test]
fn reads_inline_payload_and_preserves_unknown_fields() {
    let marker_key = "__extra__";
    let marker_value = "hello";

    let payload = tool_call_finished_payload(marker_key, marker_value);
    let event = read_event_from_reader(payload.as_bytes()).expect("read");

    let HookEvent::ToolCallFinished(payload) = event else {
        panic!("expected tool-call-finished");
    };

    assert_eq!(payload.tool_name, "exec");
    assert_eq!(
        payload.extra.get(marker_key).and_then(|v| v.as_str()),
        Some(marker_value)
    );
}

#[test]
fn resolves_payload_path_envelope() {
    let dir = TempDir::new().expect("tmp");

    let marker_key = "__marker__";
    let marker_value = "ok";

    let payload_path = dir.path().join("payload.json");
    std::fs::write(
        &payload_path,
        tool_call_finished_payload(marker_key, marker_value),
    )
    .expect("write");

    let envelope = format!("{{\"payload-path\":\"{}\"}}", payload_path.display());
    let event = read_event_from_reader(envelope.as_bytes()).expect("read");

    let HookEvent::ToolCallFinished(payload) = event else {
        panic!("expected tool-call-finished");
    };

    assert_eq!(
        payload.extra.get(marker_key).and_then(|v| v.as_str()),
        Some(marker_value)
    );
}

#[test]
fn unknown_event_types_become_unknown() {
    let payload = r#"{
  "schema-version": 1,
  "event-id": "e",
  "timestamp": "t",
  "type": "new-event-type",
  "answer": 42
}"#;

    let event = read_event_from_reader(payload.as_bytes()).expect("read");
    let HookEvent::Unknown(payload) = event else {
        panic!("expected unknown");
    };

    assert_eq!(payload.event_type.as_deref(), Some("new-event-type"));
    assert_eq!(
        payload
            .extra
            .get("answer")
            .and_then(serde_json::Value::as_i64),
        Some(42)
    );
}
