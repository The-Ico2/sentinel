use serde::{Deserialize, Serialize};
use serde_json::{Value, to_vec, from_slice};
use windows::core::PCWSTR;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, ERROR_PIPE_BUSY},
    Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES,
    },
    System::Pipes::WaitNamedPipeW,
};
use crate::ipc::response::IpcResponse;
use crate::{info, warn, error};

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcRequest {
    pub ns: String,
    pub cmd: String,
    pub args: Option<Value>,
}

const PIPE_NAME: &str = r"\\.\pipe\sentinel";
const BUFFER_SIZE: usize = 16 * 1024;

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn send_ipc_request(request: IpcRequest) -> Result<IpcResponse, String> {
    info!("IPC request: ns='{}', cmd='{}', args={:?}", request.ns, request.cmd, request.args);

    unsafe {
        // --- Connect to pipe ---
        let handle: HANDLE = loop {
            let result = CreateFileW(
                PCWSTR(to_wide(PIPE_NAME).as_ptr()),
                FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            );

            match result {
                Ok(h) => {
                    info!("Connected to IPC pipe '{}'", PIPE_NAME);
                    break h;
                },
                Err(err) => {
                    let code = err.code().0 as u32;
                    if code == ERROR_PIPE_BUSY.0 {
                        warn!("IPC pipe busy, waiting...");
                        let _ = WaitNamedPipeW(PCWSTR(to_wide(PIPE_NAME).as_ptr()), 2000);
                        continue;
                    }
                    error!("Failed to connect to IPC pipe: {:?}", err);
                    return Err(format!("IPC connect failed: {:?}", err));
                }
            }
        };

        // --- Send request ---
        let payload = match to_vec(&request) {
            Ok(p) => p,
            Err(e) => {
                let _ = CloseHandle(handle);
                error!("Failed to serialize IPC request: {e}");
                return Err(format!("IPC serialize failed: {e}"));
            }
        };

        let mut written = 0u32;
        if WriteFile(handle, Some(&payload), Some(&mut written), None).is_err() {
            let _ = CloseHandle(handle);
            error!("IPC write failed");
            return Err("IPC write failed".into());
        }
        info!("Sent {} bytes to IPC server", written);

        // --- Read response ---
        let mut buffer = vec![0u8; BUFFER_SIZE];
        let mut read = 0u32;
        if ReadFile(handle, Some(&mut buffer), Some(&mut read), None).is_err() {
            let _ = CloseHandle(handle);
            error!("IPC read failed");
            return Err("IPC read failed".into());
        }
        info!("Received {} bytes from IPC server", read);

        let _ = CloseHandle(handle);

        match from_slice::<IpcResponse>(&buffer[..read as usize]) {
            Ok(resp) => {
                info!("IPC response: ok={}, error={:?}", resp.ok, resp.error);
                Ok(resp)
            },
            Err(e) => {
                error!("Failed to decode IPC response: {e}");
                Err(format!("IPC decode failed: {e}"))
            }
        }
    }
}