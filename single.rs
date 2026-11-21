//! 单实例辅助：找到已经在运行的同一个程序，把它的主窗口恢复到前台。
//!
//! 场景：用户重复双击桌面图标。以前只是弹一个「已经在运行」的提示框，
//! 现在直接把已有窗口拉到最前（含最小化还原），体验上等同于「切回去」。

/// 找到另一个已运行实例的主窗口并激活（排除当前进程自己）
#[cfg(windows)]
pub fn activate_existing() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let me = std::process::id();
    let exe_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_lowercase()))
        .unwrap_or_else(|| "door-window-manager.exe".to_string());

    let mut pids: Vec<u32> = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == 0 || snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let name = wide_to_string(&entry.szExeFile).to_lowercase();
            if name == exe_name && entry.th32ProcessID != me {
                pids.push(entry.th32ProcessID);
            }
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }

    for pid in pids {
        if let Some(hwnd) = find_main_window(pid) {
            if bring_to_front(hwnd) {
                return true;
            }
        }
    }
    false
}

#[cfg(not(windows))]
pub fn activate_existing() -> bool {
    false
}

#[cfg(windows)]
fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(windows)]
struct WindowSearch {
    pid: u32,
    /// (hwnd, 面积, 是否可见)
    best: Option<(isize, i64, bool)>,
}

/// 找该进程最像「主窗口」的顶层窗口：
/// 标题非空、尺寸大于 150×100；**可见的优先**，同类里取面积最大的。
/// 不做「必须可见」的硬过滤——微信/QQ 缩到托盘时窗口是隐藏的，
/// 但依然要能把它叫回来。
#[cfg(windows)]
pub fn find_best_window(pid: u32) -> Option<isize> {
    use windows_sys::Win32::Foundation::{BOOL, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };

    unsafe extern "system" fn callback(hwnd: isize, lparam: isize) -> BOOL {
        let ctx = &mut *(lparam as *mut WindowSearch);
        let mut wpid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut wpid);
        if wpid != ctx.pid || GetWindowTextLengthW(hwnd) == 0 {
            return 1;
        }
        // 排除「消息循环窗口 / 输入法窗口」。
        // 微信会建一个 Qt...MessageWindow 类、标题 WxTrayIconMessageWindow 的窗口，
        // 尺寸等于整个屏幕、还标成可见，但它不是能聊天的主窗口——激活它等于没打开。
        let mut cls = [0u16; 256];
        let n = GetClassNameW(hwnd, cls.as_mut_ptr(), cls.len() as i32);
        if n > 0 {
            let cname = String::from_utf16_lossy(&cls[..n as usize]).to_lowercase();
            if cname.contains("messagewindow") || cname.contains("ime") || cname.contains("powermessage")
            {
                return 1;
            }
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW != 0 {
            return 1;
        }
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut r) == 0 {
            return 1;
        }
        let w = (r.right - r.left) as i64;
        let h = (r.bottom - r.top) as i64;
        if w < 150 || h < 100 {
            return 1;
        }
        let visible = IsWindowVisible(hwnd) != 0;
        let area = w * h;
        let better = match ctx.best {
            None => true,
            Some((_, a, v)) => (visible && !v) || (visible == v && area > a),
        };
        if better {
            ctx.best = Some((hwnd, area, visible));
        }
        1 // 继续枚举
    }

    let mut ctx = WindowSearch {
        pid,
        best: None,
    };
    unsafe {
        EnumWindows(Some(callback), &mut ctx as *mut WindowSearch as isize);
    }
    ctx.best.map(|(h, _, _)| h)
}

/// 枚举该进程的可见顶层窗口（有标题、尺寸正常），取第一个
#[cfg(windows)]
fn find_main_window(pid: u32) -> Option<isize> {
    find_best_window(pid)
}

/// 还原（若最小化/隐藏）+ 提到最前。
/// Windows 有前台锁定：直接 SetForegroundWindow 常被拒绝，
/// 这里用「置顶再取消置顶」+ AttachThreadInput 两种方式兜底。
#[cfg(windows)]
pub fn bring_to_front(hwnd: isize) -> bool {
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, IsIconic, SetForegroundWindow, SetWindowPos,
        ShowWindow, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        SW_RESTORE, SW_SHOW,
    };

    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
        // 置顶/取消置顶：绕开「后台进程不能抢前台」的限制
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        BringWindowToTop(hwnd);
        if SetForegroundWindow(hwnd) != 0 {
            return true;
        }
        // 兜底：临时挂到当前前台线程的输入队列上
        let fg = GetForegroundWindow();
        if fg != 0 {
            let fg_tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
            let my_tid = GetCurrentThreadId();
            if fg_tid != 0 && fg_tid != my_tid {
                AttachThreadInput(my_tid, fg_tid, 1);
                SetForegroundWindow(hwnd);
                AttachThreadInput(my_tid, fg_tid, 0);
            }
        }
    }
    true
}

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
