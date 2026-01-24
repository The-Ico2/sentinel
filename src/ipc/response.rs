use serde::{Serialize, Deserialize};
use serde_json::Value;
use crate::{info, warn};

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    pub ok: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
}

impl IpcResponse {
    pub fn ok(data: Value) -> Self {
        info!("IPC Response created: ok=true, data present");
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        let msg_str = msg.into();
        warn!("IPC Response created: ok=false, error='{}'", msg_str);
        Self {
            ok: false,
            data: None,
            error: Some(msg_str),
        }
    }
}