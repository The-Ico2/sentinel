// ~/sentinel/sentinel-backend/src/ipc/appdata/trayicons.rs
// Enumerates system tray (notification area) icons including hidden overflow area.
// Returns app name, tooltip, process ID, visibility, and exe path for each icon.

use serde_json::{json, Value};
use std::os::windows::process::CommandExt;

/// Retrieves all system tray notification area icons using Shell_NotifyIconGetRect
/// and toolbar enumeration of the notification area.
pub fn get_tray_icons_json() -> Value {
	// Primary strategy: use UI Automation / toolbar message enumeration via PowerShell
	// The notification area is a ToolbarWindow32 inside Shell_TrayWnd
	let script = r#"
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Diagnostics;
using System.Collections.Generic;

public class TrayIconEnumerator {
    [DllImport("user32.dll", SetLastError = true)]
    static extern IntPtr FindWindow(string lpClassName, string lpWindowName);

    [DllImport("user32.dll", SetLastError = true)]
    static extern IntPtr FindWindowEx(IntPtr hwndParent, IntPtr hwndChildAfter, string lpszClass, string lpszWindow);

    [DllImport("user32.dll")]
    static extern int SendMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);

    [DllImport("kernel32.dll")]
    static extern IntPtr OpenProcess(uint dwDesiredAccess, bool bInheritHandle, uint dwProcessId);

    [DllImport("kernel32.dll")]
    static extern IntPtr VirtualAllocEx(IntPtr hProcess, IntPtr lpAddress, UIntPtr dwSize, uint flAllocationType, uint flProtect);

    [DllImport("kernel32.dll")]
    static extern bool VirtualFreeEx(IntPtr hProcess, IntPtr lpAddress, UIntPtr dwSize, uint dwFreeType);

    [DllImport("kernel32.dll")]
    static extern bool ReadProcessMemory(IntPtr hProcess, IntPtr lpBaseAddress, byte[] lpBuffer, UIntPtr nSize, out UIntPtr lpNumberOfBytesRead);

    [DllImport("kernel32.dll")]
    static extern bool WriteProcessMemory(IntPtr hProcess, IntPtr lpBaseAddress, byte[] lpBuffer, UIntPtr nSize, out UIntPtr lpNumberOfBytesWritten);

    [DllImport("kernel32.dll")]
    static extern bool CloseHandle(IntPtr hObject);

    const uint TB_BUTTONCOUNT = 0x0418;
    const uint TB_GETBUTTON = 0x0417;
    const uint TB_GETBUTTONTEXTW = 0x004B;
    const uint PROCESS_VM_OPERATION = 0x0008;
    const uint PROCESS_VM_READ = 0x0010;
    const uint PROCESS_VM_WRITE = 0x0020;
    const uint MEM_COMMIT = 0x1000;
    const uint MEM_RELEASE = 0x8000;
    const uint PAGE_READWRITE = 0x04;

    [StructLayout(LayoutKind.Sequential)]
    struct TBBUTTON64 {
        public int iBitmap;
        public int idCommand;
        public byte fsState;
        public byte fsStyle;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 6)]
        public byte[] bReserved;
        public ulong dwData;
        public long iString;
    }

    public static string Enumerate() {
        var results = new List<string>();
        // Main tray
        EnumerateToolbar("Shell_TrayWnd", null, "TrayNotifyWnd", "SysPager", "ToolbarWindow32", "Visible", results);
        // Overflow area
        EnumerateToolbar("NotifyIconOverflowWindow", null, null, null, "ToolbarWindow32", "Overflow", results);

        return string.Join("\n", results);
    }

    static void EnumerateToolbar(string parentClass, string parentTitle, string child1, string child2, string toolbarClass, string area, List<string> results) {
        IntPtr hwnd = FindWindow(parentClass, parentTitle);
        if (hwnd == IntPtr.Zero) return;

        if (child1 != null) {
            hwnd = FindWindowEx(hwnd, IntPtr.Zero, child1, null);
            if (hwnd == IntPtr.Zero) return;
        }
        if (child2 != null) {
            hwnd = FindWindowEx(hwnd, IntPtr.Zero, child2, null);
            if (hwnd == IntPtr.Zero) return;
        }
        IntPtr toolbar = FindWindowEx(hwnd, IntPtr.Zero, toolbarClass, null);
        if (toolbar == IntPtr.Zero) return;

        uint explorerPid;
        GetWindowThreadProcessId(toolbar, out explorerPid);
        IntPtr hProcess = OpenProcess(PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE, false, explorerPid);
        if (hProcess == IntPtr.Zero) return;

        try {
            int count = SendMessage(toolbar, TB_BUTTONCOUNT, IntPtr.Zero, IntPtr.Zero);
            int btnSize = Marshal.SizeOf(typeof(TBBUTTON64));
            IntPtr remoteBtn = VirtualAllocEx(hProcess, IntPtr.Zero, (UIntPtr)btnSize, MEM_COMMIT, PAGE_READWRITE);
            IntPtr remoteTxt = VirtualAllocEx(hProcess, IntPtr.Zero, (UIntPtr)512, MEM_COMMIT, PAGE_READWRITE);

            if (remoteBtn == IntPtr.Zero || remoteTxt == IntPtr.Zero) return;

            byte[] localBtn = new byte[btnSize];
            for (int i = 0; i < count; i++) {
                SendMessage(toolbar, TB_GETBUTTON, (IntPtr)i, remoteBtn);
                UIntPtr bytesRead;
                ReadProcessMemory(hProcess, remoteBtn, localBtn, (UIntPtr)btnSize, out bytesRead);

                var btn = new TBBUTTON64();
                var handle = GCHandle.Alloc(localBtn, GCHandleType.Pinned);
                try { btn = (TBBUTTON64)Marshal.PtrToStructure(handle.AddrOfPinnedObject(), typeof(TBBUTTON64)); }
                finally { handle.Free(); }

                // Read dwData to get the NOTIFYICONDATA-like struct (contains hWnd and uID at known offsets)
                byte[] dwDataBuf = new byte[32];
                if (btn.dwData != 0) {
                    ReadProcessMemory(hProcess, (IntPtr)(long)btn.dwData, dwDataBuf, (UIntPtr)32, out bytesRead);
                }

                // Extract hWnd (first 8 bytes on 64-bit) and uID (next 4 bytes)
                long iconHwnd = BitConverter.ToInt64(dwDataBuf, 0);
                uint iconUid = BitConverter.ToUInt32(dwDataBuf, 8);

                uint iconPid = 0;
                if (iconHwnd != 0) {
                    GetWindowThreadProcessId((IntPtr)iconHwnd, out iconPid);
                }

                // Get tooltip text
                int txtLen = SendMessage(toolbar, TB_GETBUTTONTEXTW, (IntPtr)btn.idCommand, remoteTxt);
                string tooltip = "";
                if (txtLen > 0) {
                    byte[] txtBuf = new byte[txtLen * 2 + 2];
                    ReadProcessMemory(hProcess, remoteTxt, txtBuf, (UIntPtr)txtBuf.Length, out bytesRead);
                    tooltip = Encoding.Unicode.GetString(txtBuf, 0, txtLen * 2);
                }

                // Get process name and exe path
                string procName = "";
                string exePath = "";
                if (iconPid > 0) {
                    try {
                        var proc = Process.GetProcessById((int)iconPid);
                        procName = proc.ProcessName;
                        try { exePath = proc.MainModule.FileName; } catch {}
                    } catch {}
                }

                bool visible = (btn.fsState & 0x08) == 0; // TBSTATE_HIDDEN = 0x08

                results.Add(string.Format("ICON_START={0}", i));
                results.Add(string.Format("ICON_AREA={0}", area));
                results.Add(string.Format("ICON_PID={0}", iconPid));
                results.Add(string.Format("ICON_PROCNAME={0}", procName));
                results.Add(string.Format("ICON_EXEPATH={0}", exePath));
                results.Add(string.Format("ICON_TOOLTIP={0}", tooltip.Replace("\n", " ").Replace("\r", "")));
                results.Add(string.Format("ICON_VISIBLE={0}", visible));
                results.Add(string.Format("ICON_END={0}", i));
            }

            VirtualFreeEx(hProcess, remoteBtn, UIntPtr.Zero, MEM_RELEASE);
            VirtualFreeEx(hProcess, remoteTxt, UIntPtr.Zero, MEM_RELEASE);
        } finally {
            CloseHandle(hProcess);
        }
    }
}
"@

$output = [TrayIconEnumerator]::Enumerate()
Write-Output $output
"#;

	let output = std::process::Command::new("powershell")
		.args(["-NoProfile", "-Command", script])
		.creation_flags(0x08000000) // CREATE_NO_WINDOW
		.output();

	match output {
		Ok(o) => {
			let stdout = String::from_utf8_lossy(&o.stdout);
			let stderr = String::from_utf8_lossy(&o.stderr);
			parse_tray_output(&stdout, &stderr)
		}
		Err(e) => {
			json!({
				"error": format!("Failed to enumerate tray icons: {}", e),
				"count": 0,
				"icons": [],
			})
		}
	}
}

fn parse_tray_output(stdout: &str, stderr: &str) -> Value {
	let lines: Vec<&str> = stdout.lines().collect();

	let mut icons = Vec::new();
	let mut current: Option<TrayIconBuilder> = None;

	for line in &lines {
		if line.starts_with("ICON_START=") {
			current = Some(TrayIconBuilder::default());
		} else if line.starts_with("ICON_END=") {
			if let Some(n) = current.take() {
				icons.push(json!({
					"area": n.area,
					"pid": n.pid,
					"process_name": n.proc_name,
					"exe_path": n.exe_path,
					"tooltip": n.tooltip,
					"visible": n.visible,
				}));
			}
		} else if let Some(ref mut n) = current {
			if let Some(v) = line.strip_prefix("ICON_AREA=") {
				n.area = v.trim().to_string();
			} else if let Some(v) = line.strip_prefix("ICON_PID=") {
				n.pid = v.trim().parse().unwrap_or(0);
			} else if let Some(v) = line.strip_prefix("ICON_PROCNAME=") {
				n.proc_name = v.trim().to_string();
			} else if let Some(v) = line.strip_prefix("ICON_EXEPATH=") {
				n.exe_path = v.trim().to_string();
			} else if let Some(v) = line.strip_prefix("ICON_TOOLTIP=") {
				n.tooltip = v.trim().to_string();
			} else if let Some(v) = line.strip_prefix("ICON_VISIBLE=") {
				n.visible = v.trim().eq_ignore_ascii_case("true");
			}
		}
	}

	let visible_count = icons.iter().filter(|i| i["visible"].as_bool().unwrap_or(false)).count();
	let overflow_count = icons.iter().filter(|i| i["area"].as_str() == Some("Overflow")).count();

	let mut result = json!({
		"count": icons.len(),
		"visible_count": visible_count,
		"overflow_count": overflow_count,
		"icons": icons,
	});

	if !stderr.trim().is_empty() {
		result["warning"] = json!(stderr.trim());
	}

	result
}

#[derive(Default)]
struct TrayIconBuilder {
	area: String,
	pid: u32,
	proc_name: String,
	exe_path: String,
	tooltip: String,
	visible: bool,
}
