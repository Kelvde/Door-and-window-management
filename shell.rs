//! 调用系统默认程序打开文件 / 文件夹。
//!
//! 这里刻意不使用 `cmd /C start "" "path"`：
//! 手动加一层引号后，`std::process::Command` 在 Windows 上会按 MSVCRT 规则再转义一次，
//! cmd 解析出来的路径就变成 `\D:\...\htm\` 这种形式，系统报「找不到文件」。
//! 直接调 ShellExecuteW 由系统做文件关联，Unicode 安全，也不需要拼命令行。

/// 用系统默认程序打开任意路径（文件 / 文件夹）
#[cfg(windows)]
pub fn shell_open(target: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // 统一成 UTF-16 + NUL 结尾，避免中文路径被按 ANSI 截断
    let file: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let op: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();

    // 返回值 > 32 才算成功；<= 32 是 ShellExecute 的错误码
    let ret = unsafe {
        ShellExecuteW(
            0, // 无父窗口
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(), // 不带参数
            std::ptr::null(), // 工作目录用默认
            SW_SHOWNORMAL,
        )
    };

    if ret <= 32 {
        return Err(format!("{}（错误码 {}）", err_hint(ret), ret));
    }
    Ok(())
}

/// ShellExecute 的返回值 <= 32 都是错误码，这里翻成人话，方便用户判断是哪一环出问题
#[cfg(windows)]
fn err_hint(code: isize) -> &'static str {
    match code {
        0 => "系统内存或资源不足",
        2 => "文件不存在",
        3 => "找不到打开该文件的程序所在路径",
        5 => "系统拒绝访问该文件",
        26 => "文件被占用或无法写入",
        27 => "文件关联不完整",
        29 => "文件被拒绝访问",
        30 => "文件正被其他程序占用",
        31 => "系统没有关联可打开该文件的程序",
        32 => "没有可用的资源管理器",
        _ => "系统调用失败",
    }
}

#[cfg(not(windows))]
pub fn shell_open(_target: &str) -> Result<(), String> {
    Err("当前系统不支持直接打开文件".to_string())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// 错误码必须翻成可读提示，不能只丢一个数字
    #[test]
    fn error_codes_are_translated() {
        assert_eq!(err_hint(2), "文件不存在");
        assert_eq!(err_hint(31), "系统没有关联可打开该文件的程序");
        assert!(!err_hint(12345).is_empty(), "未知错误码也要有兜底文案");
    }

    /// 中文/空格路径必须能原样按 UTF-16 传给系统，不能被 ANSI 截断
    #[test]
    fn chinese_path_survives_utf16() {
        let s = r"D:\门窗管理软件\data\export\客户单据_20261110_测试 空格.html";
        let mut wide: Vec<u16> = s.encode_utf16().collect();
        wide.push(0); // 模拟 C 字符串结尾
        assert_eq!(String::from_utf16(&wide[..wide.len() - 1]).unwrap(), s);
        assert_eq!(*wide.last().unwrap(), 0);
    }
}
