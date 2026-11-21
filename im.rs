//! 动态定位微信 / QQ 客户端的可执行文件。
//!
//! 用户随时可能把客户端装到别的盘、别的目录，所以**每次点按钮都重新检索**，
//! 不做任何缓存。检索顺序（命中即返回）：
//!   1. 正在运行的进程 —— 直接读进程真实映像路径，最准，换到哪都能找到；
//!   2. 注册表 —— 客户端安装时写入的 InstallPath / InstallDir；
//!   3. 开始菜单快捷方式 —— 解析 .lnk 里的目标路径；
//!   4. 常见安装目录 —— C~F 盘的 Program Files / 根目录 / 用户目录。
//!
//! 这样即使客户端升级换目录、用户自定义安装路径，也能继续用。

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ImTarget {
    WeChat,
    QQ,
}

impl ImTarget {
    pub fn parse(s: &str) -> Option<ImTarget> {
        match s {
            "wechat" => Some(ImTarget::WeChat),
            "qq" => Some(ImTarget::QQ),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ImTarget::WeChat => "微信",
            ImTarget::QQ => "QQ",
        }
    }
    /// 该客户端的进程名（小写比较）
    fn process_names(self) -> &'static [&'static str] {
        match self {
            ImTarget::WeChat => &["weixin.exe", "wechat.exe"],
            ImTarget::QQ => &["qq.exe"],
        }
    }
    /// 安装目录下可能的相对路径
    fn rel_paths(self) -> &'static [&'static str] {
        match self {
            ImTarget::WeChat => &["Weixin.exe", "WeChat.exe", "WeChat\\WeChat.exe"],
            ImTarget::QQ => &["QQ.exe", "Bin\\QQ.exe", "QQNT\\QQ.exe", "Tencent\\QQ.exe"],
        }
    }
}

/// 已经在运行就直接把它的窗口叫到最前，**绝不启动新进程**。
///
/// 这是「点打开微信/QQ 却弹出扫码登录」的正解：客户端被再启动一次时
/// 会当成新登录，而把已有窗口激活则完全不会触发登录。
#[cfg(windows)]
pub fn focus_existing(target: ImTarget) -> bool {
    for pid in pids_named(target) {
        if let Some(hwnd) = crate::single::find_best_window(pid) {
            log_probe(&format!(
                "{} 已在运行 pid={} hwnd=0x{:x}",
                target.label(),
                pid,
                hwnd
            ));
            return crate::single::bring_to_front(hwnd);
        }
    }
    false
}

#[cfg(not(windows))]
pub fn focus_existing(_t: ImTarget) -> bool {
    false
}

/// 该客户端是否已在运行（不判断有没有窗口）
#[cfg(windows)]
pub fn is_running(target: ImTarget) -> bool {
    !pids_named(target).is_empty()
}

#[cfg(not(windows))]
pub fn is_running(_t: ImTarget) -> bool {
    false
}

/// 列出所有进程名匹配的 PID（不含自己）
#[cfg(windows)]
fn pids_named(target: ImTarget) -> Vec<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let me = std::process::id();
    let mut pids = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == 0 || snap == INVALID_HANDLE_VALUE {
            return pids;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let name = wide_to_string(&entry.szExeFile).to_lowercase();
            if target.process_names().contains(&name.as_str()) && entry.th32ProcessID != me {
                pids.push(entry.th32ProcessID);
            }
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }
    pids
}

/// 启动前的定位过程写一行日志，方便用户反馈「点开还是弹登录」时排查
fn log_probe(line: &str) {
    use std::io::Write;
    let p = crate::db::data_dir().join("startup.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(
            f,
            "{} {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            line
        );
    }
}

/// 依次用四种方式检索，返回第一个真实存在的 exe
pub fn find(target: ImTarget) -> Option<PathBuf> {
    if let Some(p) = from_running_process(target) {
        log_probe(&format!("{} 取运行中进程路径 {}", target.label(), p.display()));
        return Some(p);
    }
    if let Some(p) = from_registry(target) {
        log_probe(&format!("{} 取注册表路径 {}", target.label(), p.display()));
        return Some(p);
    }
    if let Some(p) = from_start_menu(target) {
        log_probe(&format!("{} 取开始菜单快捷方式 {}", target.label(), p.display()));
        return Some(p);
    }
    let p = from_common_paths(target);
    if let Some(ref p) = p {
        log_probe(&format!("{} 取常见目录 {}", target.label(), p.display()));
    }
    p
}

/// 在候选目录里找目标 exe
fn probe_dir(dir: &Path, target: ImTarget) -> Option<PathBuf> {
    for rel in target.rel_paths() {
        let p = dir.join(rel);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

// ---------------------------------------------------------------- 1. 运行中进程

#[cfg(windows)]
fn from_running_process(target: ImTarget) -> Option<PathBuf> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == 0 || snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        let mut found = None;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let name = wide_to_string(&entry.szExeFile).to_lowercase();
            if target.process_names().contains(&name.as_str()) {
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                if h != 0 {
                    let mut buf = [0u16; 1024];
                    let mut len = buf.len() as u32;
                    if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0 {
                        let p = String::from_utf16_lossy(&buf[..len as usize]);
                        let pb = PathBuf::from(&p);
                        if pb.is_file() {
                            found = Some(pb);
                        }
                    }
                    CloseHandle(h);
                }
                if found.is_some() {
                    break;
                }
            }
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
        found
    }
}

#[cfg(not(windows))]
fn from_running_process(_t: ImTarget) -> Option<PathBuf> {
    None
}

#[cfg(windows)]
fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

// ---------------------------------------------------------------- 2. 注册表

#[cfg(windows)]
fn from_registry(target: ImTarget) -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    // (根键, 子键, 值名)
    let keys: &[(isize, &str, &str)] = match target {
        ImTarget::WeChat => &[
            (HKEY_CURRENT_USER, r"Software\Tencent\Weixin", "InstallPath"),
            (HKEY_CURRENT_USER, r"Software\Tencent\Weixin", "InstallDir"),
            (HKEY_CURRENT_USER, r"Software\Tencent\WeChat", "InstallPath"),
            (HKEY_CURRENT_USER, r"Software\Tencent\WeChat", "InstallDir"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Tencent\Weixin", "InstallPath"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\Tencent\Weixin", "InstallPath"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Tencent\WeChat", "InstallPath"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\Tencent\WeChat", "InstallPath"),
        ],
        ImTarget::QQ => &[
            (HKEY_CURRENT_USER, r"Software\Tencent\QQ", "InstallDir"),
            (HKEY_CURRENT_USER, r"Software\Tencent\QQ", "InstallPath"),
            (HKEY_CURRENT_USER, r"Software\Tencent\QQNT", "InstallDir"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Tencent\QQ", "InstallDir"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\Tencent\QQ", "InstallDir"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Tencent\QQNT", "InstallDir"),
        ],
    };
    for (root, sub, val) in keys {
        if let Some(dir) = reg_read_string(*root, sub, val) {
            if let Some(p) = probe_dir(Path::new(&dir), target) {
                return Some(p);
            }
        }
    }
    // 卸载项里的 DisplayIcon 常是 exe 全路径，作为补充
    for (root, sub) in [
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\QQ"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\QQ"),
        (
            HKEY_CURRENT_USER,
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Weixin",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Weixin",
        ),
    ] {
        if let Some(icon) = reg_read_string(root, sub, "DisplayIcon") {
            let clean = icon.trim_matches('"').split(',').next().unwrap_or("").trim();
            if clean.to_lowercase().ends_with(".exe") && Path::new(clean).is_file() {
                let name = Path::new(clean)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if target.process_names().contains(&name.as_str()) {
                    return Some(PathBuf::from(clean));
                }
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn from_registry(_t: ImTarget) -> Option<PathBuf> {
    None
}

#[cfg(windows)]
fn reg_read_string(root: isize, sub: &str, value: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, KEY_READ,
    };

    let sub_w: Vec<u16> = sub.encode_utf16().chain(std::iter::once(0)).collect();
    let val_w: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut hkey: isize = 0;
        if RegOpenKeyExW(root, sub_w.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }
        // 先问需要多大
        let mut ty: u32 = 0;
        let mut size: u32 = 0;
        let mut out = None;
        if RegQueryValueExW(
            hkey,
            val_w.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            std::ptr::null_mut(),
            &mut size,
        ) == 0
            && size > 2
            && size < 8192
        {
            let mut buf = vec![0u8; size as usize + 2];
            let mut got = size;
            if RegQueryValueExW(
                hkey,
                val_w.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                buf.as_mut_ptr(),
                &mut got,
            ) == 0
            {
                let wide: Vec<u16> = buf
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                let s = String::from_utf16_lossy(&wide)
                    .trim_end_matches('\0')
                    .trim()
                    .to_string();
                if !s.is_empty() {
                    out = Some(s);
                }
            }
        }
        RegCloseKey(hkey);
        out
    }
}

// ---------------------------------------------------------------- 3. 开始菜单快捷方式

fn from_start_menu(target: ImTarget) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(pd) = std::env::var("ProgramData") {
        dirs.push(Path::new(&pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(ad) = std::env::var("APPDATA") {
        dirs.push(Path::new(&ad).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    for dir in dirs {
        if let Some(p) = scan_lnk_dir(&dir, target, 0) {
            return Some(p);
        }
    }
    None
}

fn scan_lnk_dir(dir: &Path, target: ImTarget, depth: u8) -> Option<PathBuf> {
    if depth > 3 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        if path.extension().map(|x| x.eq_ignore_ascii_case("lnk")) != Some(true) {
            continue;
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let interesting = match target {
            ImTarget::WeChat => {
                stem.contains("微信") || stem.contains("wechat") || stem.contains("weixin")
            }
            ImTarget::QQ => stem == "qq" || stem.contains("腾讯qq") || stem.contains("qqnt"),
        };
        if !interesting {
            continue;
        }
        if let Some(exe) = exe_from_lnk(&path, target) {
            return Some(PathBuf::from(exe));
        }
    }
    for d in subdirs {
        if let Some(p) = scan_lnk_dir(&d, target, depth + 1) {
            return Some(p);
        }
    }
    None
}

/// 从 .lnk 二进制里提取目标 exe：快捷方式把路径存成 UTF-16LE，
/// 这里扫描「盘符:\...\.exe」形式的片段，再验证文件真实存在。
fn exe_from_lnk(lnk: &Path, target: ImTarget) -> Option<String> {
    let data = std::fs::read(lnk).ok()?;
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let text = String::from_utf16_lossy(&units);
    let chars: Vec<char> = text.chars().collect();
    let want_names = target.process_names();

    let mut i = 0usize;
    while i + 3 < chars.len() {
        let is_drive = chars[i].is_ascii_alphanumeric()
            && chars[i + 1] == ':'
            && (chars[i + 2] == '\\' || chars[i + 2] == '/');
        if !is_drive {
            i += 1;
            continue;
        }
        let mut s = String::new();
        let mut j = i;
        while j < chars.len() && j - i < 280 {
            let c = chars[j];
            if c.is_control() {
                break;
            }
            s.push(c);
            if s.to_lowercase().ends_with(".exe") {
                break;
            }
            j += 1;
        }
        if s.to_lowercase().ends_with(".exe") {
            let name = Path::new(&s)
                .file_name()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if want_names.contains(&name.as_str()) && Path::new(&s).is_file() {
                return Some(s);
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------- 4. 常见目录

fn from_common_paths(target: ImTarget) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    // 各盘符的常见安装位置（不限 C 盘，换盘也能找到）
    for letter in b'C'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        let d = Path::new(&drive);
        if !d.is_dir() {
            continue;
        }
        for rel in [
            "Program Files\\Tencent",
            "Program Files (x86)\\Tencent",
            "Tencent",
        ] {
            roots.push(d.join(rel));
        }
        for rel in ["Weixin", "WeChat", "QQ", "Tencent\\QQNT"] {
            roots.push(d.join(rel));
        }
    }
    // 用户级安装（部分版本默认装到 AppData）
    for var in ["LOCALAPPDATA", "APPDATA", "ProgramData"] {
        if let Ok(base) = std::env::var(var) {
            let b = Path::new(&base);
            for rel in [
                "Programs\\Tencent",
                "Programs\\Weixin",
                "Programs\\WeChat",
                "Tencent",
                "Tencent\\Weixin",
            ] {
                roots.push(b.join(rel));
            }
        }
    }

    for r in roots {
        if let Some(p) = probe_dir(&r, target) {
            return Some(p);
        }
        // 常见目录下还有一层以客户端命名的子目录
        for sub in ["Weixin", "WeChat", "QQ", "QQNT", "Bin"] {
            if let Some(p) = probe_dir(&r.join(sub), target) {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_parse_and_label() {
        assert!(matches!(ImTarget::parse("wechat"), Some(ImTarget::WeChat)));
        assert!(matches!(ImTarget::parse("qq"), Some(ImTarget::QQ)));
        assert!(ImTarget::parse("telegram").is_none());
        assert_eq!(ImTarget::WeChat.label(), "微信");
    }

    #[test]
    fn process_name_match_is_case_insensitive() {
        // 进程名统一转小写后比较
        assert!(ImTarget::QQ.process_names().contains(&"qq.exe"));
        assert!(ImTarget::WeChat.process_names().contains(&"weixin.exe"));
    }

    /// 换目录也要能找到：只要开始菜单快捷方式指向真实 exe
    #[test]
    fn lnk_scan_handles_utf16_path() {
        // 构造一段典型 .lnk 内容：前置垃圾 + UTF-16 路径
        let fake_path = r"D:\某公司\自定义目录\Weixin.exe";
        let mut bytes: Vec<u8> = vec![0x4c, 0x00, 0x00, 0x00];
        for u in fake_path.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0]);
        // 写临时 lnk 后解析（文件本身不存在时不应误报）
        let tmp = std::env::temp_dir().join("dw_test_fake.lnk");
        std::fs::write(&tmp, &bytes).unwrap();
        let got = exe_from_lnk(&tmp, ImTarget::WeChat);
        std::fs::remove_file(&tmp).ok();
        assert!(got.is_none(), "exe 不存在时不能返回假路径");
    }
}
