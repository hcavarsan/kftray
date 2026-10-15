use std::fmt::{
    self,
    Display,
};

use kftray_telemetry::{
    Operation,
    measure_with_event,
};
use serde::ser::{
    Serialize,
    SerializeStruct,
    Serializer,
};

/// The rejection of a command that ran under `measure`. It reaches the
/// webview as `{ message, eventId }`: the id of the error event the failure
/// produced, so the webview links the problem report to it instead of
/// reporting the same failure again, or `null` when nothing was sent (an
/// expected outcome, a repeat, reporting off). The object shape alone tells
/// the webview the backend owns the reporting for this command.
#[derive(Debug, PartialEq, Eq)]
pub struct MeasuredError {
    pub message: String,
    pub event_id: Option<String>,
}

impl From<String> for MeasuredError {
    fn from(message: String) -> Self {
        Self {
            message,
            event_id: None,
        }
    }
}

impl Display for MeasuredError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Serialize for MeasuredError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("MeasuredError", 2)?;
        state.serialize_field("message", &self.message)?;
        state.serialize_field("eventId", &self.event_id)?;
        state.end()
    }
}

/// `measure` for a command: the error carries the event id it captured.
pub async fn measured<T, E: Display>(
    operation: Operation, future: impl Future<Output = Result<T, E>>,
) -> Result<T, MeasuredError> {
    let (result, event_id) = measure_with_event(operation, future).await;
    result.map_err(|error| MeasuredError {
        message: error.to_string(),
        event_id: event_id.map(|id| id.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn serializes_a_null_event_id_when_nothing_was_reported() {
        let error = MeasuredError::from("port in use".to_string());

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({ "message": "port in use", "eventId": null })
        );
    }

    #[test]
    fn serializes_camel_case_keys_with_an_event_id() {
        let error = MeasuredError {
            message: "port in use".to_string(),
            event_id: Some("0a1b2c3d-0000-4000-8000-000000000000".to_string()),
        };

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "message": "port in use",
                "eventId": "0a1b2c3d-0000-4000-8000-000000000000",
            })
        );
    }
}
