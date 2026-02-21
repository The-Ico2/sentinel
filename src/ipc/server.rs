use std::{thread, time::Duration};
use serde_json::{from_slice, to_vec};
use windows::core::PCWSTR;
use windows::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE, CloseHandle, GetLastError, ERROR_PIPE_CONNECTED},
    System::Pipes::*,
    Storage::FileSystem::{ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES},
};

use crate::{
    ipc::{
        request::IpcRequest,
        response::IpcResponse,
        dispatch::dispatch,
    },
    custom::windows::WindowsCManager,
};
use crate::{info, warn, error};

const PIPE_NAME: &str = r"\\.\pipe\sentinel";
const PIPE_ACCESS_DUPLEX: u32 = 0x00000003;

const BUFFER_SIZE: u32 = 16 * 1024;

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn start_ipc_server() {
    info!("Starting IPC server on pipe '{}'", PIPE_NAME);
    let windows = WindowsCManager::new();

    unsafe {
        loop {
            let pipe = CreateNamedPipeW(
                PCWSTR(to_wide(PIPE_NAME).as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(PIPE_ACCESS_DUPLEX),
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER_SIZE,
                BUFFER_SIZE,
                0,
                None,
            );

            if pipe == INVALID_HANDLE_VALUE {
                error!("Failed to create named pipe; retrying in 100ms");
                thread::sleep(Duration::from_millis(100));
                continue;
            }

            let connected = match ConnectNamedPipe(pipe, None) {
                Ok(_) => true,
                Err(_) => GetLastError() == ERROR_PIPE_CONNECTED,
            };

            if connected {
                handle_client(pipe, &windows);
                let _ = DisconnectNamedPipe(pipe);
                let _ = CloseHandle(pipe);
            } else {
                warn!("Failed to connect named pipe; closing and retrying in 100ms");
                let _ = CloseHandle(pipe);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

unsafe fn handle_client(pipe: HANDLE, windows: &WindowsCManager) {
    let mut buffer_vec = vec![0u8; BUFFER_SIZE as usize];
    let mut read = 0u32;

    if ReadFile(pipe, Some(&mut buffer_vec), Some(&mut read), None).is_err() {
        warn!("Failed to read from IPC pipe");
        return;
    }

    let req: IpcRequest = match from_slice(&buffer_vec[..read as usize]) {
        Ok(r) => r,
        Err(e) => {
            error!("Invalid IPC request: {e}");
            send(pipe, IpcResponse::err(format!("invalid request: {e}")));
            return;
        }
    };

    let response = match dispatch(&windows, &req.ns, &req.cmd, req.args) {
        Ok(value) => IpcResponse::ok(value),
        Err(err) => {
            warn!("IPC dispatch error: {}", err);
            IpcResponse::err(err)
        }
    };

    send(pipe, response);
}

unsafe fn send(pipe: HANDLE, resp: IpcResponse) {
    let bytes = to_vec(&resp).unwrap_or_else(|e| {
        error!("Failed to serialize IPC response: {e}");
        Vec::new()
    });
    let mut written = 0u32;
    if WriteFile(pipe, Some(&bytes), Some(&mut written), None).is_err() {
        warn!("Failed to write IPC response");
    }
}