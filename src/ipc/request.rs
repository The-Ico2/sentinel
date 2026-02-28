use serde::{Deserialize, Serialize};
use serde_json::{Value, to_vec, from_slice};
use windows::core::PCWSTR;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, ERROR_PIPE_BUSY, ERROR_MORE_DATA, ERROR_BROKEN_PIPE, ERROR_NO_DATA},
    Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES,
    },
    System::Pipes::{WaitNamedPipeW, SetNamedPipeHandleState, PIPE_READMODE_MESSAGE},
};
use crate::ipc::response::IpcResponse;
use crate::error;

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcRequest {
    pub ns: String,
    pub cmd: String,
    pub args: Option<Value>,
}

const PIPE_NAME: &str = r"\\.\pipe\sentinel";
const READ_CHUNK: usize = 64 * 1024;

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn is_win32_error(err: &windows::core::Error, win32_code: u32) -> bool {
    err.code() == windows::core::HRESULT::from_win32(win32_code)
}

pub fn send_ipc_request(request: IpcRequest) -> Result<IpcResponse, String> {
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
                Ok(h) => break h,
                Err(err) => {
                    let code = err.code().0 as u32;
                    if code == ERROR_PIPE_BUSY.0 {
                        let _ = WaitNamedPipeW(PCWSTR(to_wide(PIPE_NAME).as_ptr()), 2000);
                        continue;
                    }
                    return Err(format!("IPC connect failed: {:?}", err));
                }
            }
        };

        // --- Send request ---

        // Switch the client handle to message-read mode so ReadFile
        // returns ERROR_MORE_DATA when a message exceeds the read
        // buffer, instead of silently truncating.
        {
            let mut mode = PIPE_READMODE_MESSAGE;
            let _ = SetNamedPipeHandleState(handle, Some(&mut mode), None, None);
        }

        let payload = match to_vec(&request) {
            Ok(p) => p,
            Err(e) => {
                let _ = CloseHandle(handle);
                return Err(format!("IPC serialize failed: {e}"));
            }
        };

        let mut written = 0u32;
        if WriteFile(handle, Some(&payload), Some(&mut written), None).is_err() {
            let _ = CloseHandle(handle);
            return Err("IPC write failed".into());
        }

        // --- Read response (multi-chunk loop for messages > READ_CHUNK) ---
        let mut response = Vec::<u8>::new();
        loop {
            let mut chunk = vec![0u8; READ_CHUNK];
            let mut read = 0u32;

            match ReadFile(handle, Some(&mut chunk), Some(&mut read), None) {
                Ok(_) => {
                    if read == 0 {
                        break;
                    }
                    response.extend_from_slice(&chunk[..read as usize]);
                    // In byte-mode reads a successful ReadFile means we got
                    // all available data for now.  For message-mode pipes the
                    // OS would signal ERROR_MORE_DATA if more is pending.
                    break;
                }
                Err(e) => {
                    if read > 0 {
                        response.extend_from_slice(&chunk[..read as usize]);
                    }

                    if is_win32_error(&e, ERROR_MORE_DATA.0) {
                        // More data available — keep reading
                        continue;
                    }

                    // Broken pipe / no data after accumulating bytes means
                    // the server closed its end — treat what we have as complete.
                    if is_win32_error(&e, ERROR_BROKEN_PIPE.0)
                        || is_win32_error(&e, ERROR_NO_DATA.0)
                    {
                        break;
                    }

                    let _ = CloseHandle(handle);
                    error!("[IPC] [Response] read failed: {:?}", e);
                    return Err("[IPC] [Response] read failed".into());
                }
            }
        }

        let _ = CloseHandle(handle);

        match from_slice::<IpcResponse>(&response) {
            Ok(resp) => Ok(resp),
            Err(e) => {
                error!("[IPC] decode failed ({} bytes): {e}", response.len());
                Err(format!("[IPC] decode failed: {e}"))
            }
        }
    }
}