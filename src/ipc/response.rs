use serde::{Serialize, Deserialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    pub ok: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
}

impl IpcResponse {
    pub fn ok(data: Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        let msg_str = msg.into();
        Self {
            ok: false,
            data: None,
            error: Some(msg_str),
        }
    }
}