//! 门窗管理软件 · 单文件安装器
//!
//! 设计目标（老板自己换电脑用）：
//! 1. 双击就能装，不依赖任何外部工具/联网；
//! 2. **安装目录可以自己选**（默认 D 盘，避免 Program Files 写不进数据）；
//! 3. **自带数据**：把当前 data/app.db 原样带过去，装完打开就是原来的订单和客户；
//! 4. 已存在的数据**绝不覆盖**；
//! 5. 自动建桌面 + 开始菜单快捷方式，并附带「卸载.bat」。
//!
//! 两个版本由 Cargo feature 控制：
//! - 默认（`bundle-data`）= 自用版，内嵌 app.db
//! - `--no-default-features` = 开源版，不内嵌数据，首次运行自动生成基础数据

#![windows_subsystem = "windows"]

use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::BOOL;
use windows_sys::Win32::UI::Shell::{
    SHBrowseForFolderW, SHGetPathFromIDListW, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS,
    BROWSEINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO,
};

const APP_EXE_NAME: &str = "门窗管理软件.exe";

/// 主程序本体（编译时嵌入）
const APP_EXE: &[u8] = include_bytes!("../../src-tauri/target/release/door-window-manager.exe");

// 默认：只带基础数据（型号公式 / 玻璃公式 / 螺丝规则 / 开孔 / 默认配置）
#[cfg(not(feature = "with-orders"))]
const SEED_DB: &[u8] = include_bytes!("../../src-tauri/app-seed-open.db");
// 可选：连当前订单和客户一起打包（整机迁移用）
#[cfg(feature = "with-orders")]
const SEED_DB: &[u8] = include_bytes!("../../src-tauri/app-seed.db");

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn msg(title: &str, text: &str, flags: u32) -> i32 {
    let t = wide(text);
    let h = wide(title);
    unsafe { MessageBoxW(0, t.as_ptr(), h.as_ptr(), flags) }
}

fn ask(title: &str, text: &str) -> bool {
    msg(title, text, MB_YESNO | MB_ICONQUESTION) == 6 // IDYES
}

/// 弹系统「选择文件夹」对话框，返回用户选的路径
fn pick_folder(title: &str, start: &str) -> Option<PathBuf> {
    // 用 BROWSEINFO 让对话框从指定目录开始
    let start_w = wide(start);
    let mut display = [0u16; 260];
    let title_w = wide(title);
    let bi = BROWSEINFOW {
        hwndOwner: 0,
        pidlRoot: std::ptr::null_mut(),
        pszDisplayName: display.as_mut_ptr(),
        lpszTitle: title_w.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        lpfn: None,
        lParam: 0,
        iImage: 0,
    };
    let _ = start_w; // 系统对话框不保证记住初始路径，选择后由用户确认
    unsafe {
        let pidl = SHBrowseForFolderW(&bi);
        if pidl.is_null() {
            return None;
        }
        let mut buf = [0u16; 260];
        let ok = SHGetPathFromIDListW(pidl, buf.as_mut_ptr());
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..end]);
        if s.trim().is_empty() {
            None
        } else {
            Some(PathBuf::from(s))
        }
    }
}

fn default_dir() -> PathBuf {
    // 优先 D 盘（门窗厂机器上 D 盘是数据盘），没有 D 盘就用用户目录
    if Path::new(r"D:\").is_dir() {
        PathBuf::from(r"D:\门窗管理软件")
    } else {
        match std::env::var("LOCALAPPDATA") {
            Ok(p) => PathBuf::from(p).join("门窗管理软件"),
            Err(_) => PathBuf::from(r"C:\门窗管理软件"),
        }
    }
}

/// PowerShell -EncodedCommand：避免中文路径在命令行上被转码搞坏
fn run_powershell(script: &str) -> bool {
    let utf16: Vec<u16> = script.encode_utf16().collect();
    let mut bytes = Vec::with_capacity(utf16.len() * 2);
    for u in utf16 {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    let b64 = b64_encode(&bytes);
    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
            &b64,
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn b64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        out.push(T[(b[0] >> 2) as usize] as char);
        out.push(T[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(b[2] & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// 建桌面 + 开始菜单快捷方式
fn make_shortcuts(dir: &Path) {
    let exe = dir.join(APP_EXE_NAME);
    let exe_s = exe.to_string_lossy().replace('\'', "''");
    let dir_s = dir.to_string_lossy().replace('\'', "''");
    let script = format!(
        r#"
$ErrorActionPreference = 'SilentlyContinue'
$exe = '{exe}'
$work = '{dir}'
$ws = New-Object -ComObject WScript.Shell
$targets = @(
  [Environment]::GetFolderPath('Desktop'),
  (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs')
)
foreach ($t in $targets) {{
  if (-not (Test-Path $t)) {{ continue }}
  $lnk = Join-Path $t '门窗管理软件.lnk'
  $s = $ws.CreateShortcut($lnk)
  $s.TargetPath = $exe
  $s.WorkingDirectory = $work
  $s.IconLocation = "$exe,0"
  $s.Description = '门窗订单与下料管理'
  $s.Save()
}}
"#,
        exe = exe_s,
        dir = dir_s
    );
    let _ = run_powershell(&script);
}

/// 写一个卸载脚本（删快捷方式 + 删自己所在目录）
fn write_uninstaller(dir: &Path) {
    let bat = r#"@echo off
chcp 936 >nul
echo.
echo  正在卸载 门窗管理软件 ...
echo  注意：软件目录里的 data 文件夹（订单/客户数据）也会被删除。
echo  如需保留数据，请先把 data 文件夹复制到别处。
echo.
pause
del /f /q "%USERPROFILE%\Desktop\门窗管理软件.lnk" 2>nul
del /f /q "%APPDATA%\Microsoft\Windows\Start Menu\Programs\门窗管理软件.lnk" 2>nul
start "" cmd /c "timeout /t 2 >nul & rmdir /s /q ""%~dp0"""
"#;
    // 批处理按 GBK 存，中文才不会变乱码
    let gbk = to_gbk(bat);
    let _ = std::fs::write(dir.join("卸载.bat"), gbk);
}

/// 极简 GBK 编码：只处理中文与 ASCII，够写这个提示脚本
fn to_gbk(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in s.chars() {
        if (ch as u32) < 0x80 {
            out.push(ch as u8);
        } else {
            // 用系统 API 转码
            out.extend_from_slice(&gbk_bytes(ch));
        }
    }
    out
}

fn gbk_bytes(ch: char) -> Vec<u8> {
    use windows_sys::Win32::Globalization::{WideCharToMultiByte, CP_ACP};
    let mut w = [ch as u16, 0];
    let mut buf = [0u8; 8];
    let n = unsafe {
        WideCharToMultiByte(
            CP_ACP,
            0,
            w.as_mut_ptr(),
            1,
            buf.as_mut_ptr(),
            buf.len() as i32,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if n > 0 {
        buf[..n as usize].to_vec()
    } else {
        vec![b'?']
    }
}

fn install(dir: &Path, silent: bool) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败：{}", e))?;

    // 1) 主程序
    let exe_dst = dir.join(APP_EXE_NAME);
    std::fs::write(&exe_dst, APP_EXE).map_err(|e| format!("写主程序失败：{}", e))?;

    // 2) 数据：只在不存在时写入，绝不覆盖已有数据
    let data_dir = dir.join("data");
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("创建 data 目录失败：{}", e))?;
    let db_dst = data_dir.join("app.db");
    let mut seeded = false;
    if !db_dst.exists() && !SEED_DB.is_empty() {
        std::fs::write(&db_dst, SEED_DB).map_err(|e| format!("写入初始数据失败：{}", e))?;
        seeded = true;
    }

    // 3) 快捷方式 + 卸载脚本
    make_shortcuts(dir);
    write_uninstaller(dir);

    if silent {
        println!(
            "installed to {} | seeded={} | keepsExisting={}",
            dir.display(),
            seeded,
            !seeded && db_dst.exists()
        );
        return Ok(());
    }

    // 4) 把「已有数据未被覆盖」这件事如实告诉用户
    if !seeded && db_dst.exists() {
        msg(
            "安装完成",
            &format!(
                "程序已安装到：\n{}\n\n检测到该目录已有数据（data\\app.db），已保留原数据，没有覆盖。\n\n桌面和开始菜单已创建快捷方式。",
                dir.display()
            ),
            MB_OK | MB_ICONINFORMATION,
        );
    } else if cfg!(feature = "with-orders") {
        msg(
            "安装完成",
            &format!(
                "程序已安装到：\n{}\n\n已一并导入原有订单 / 客户 / 公式数据。\n桌面和开始菜单已创建快捷方式。",
                dir.display()
            ),
            MB_OK | MB_ICONINFORMATION,
        );
    } else {
        msg(
            "安装完成",
            &format!(
                "程序已安装到：\n{}\n\n基础数据已就绪（69 个型号、1415 条型材公式、242 条玻璃公式），\
                 不含订单和客户，可以直接开始用。\n桌面和开始菜单已创建快捷方式。",
                dir.display()
            ),
            MB_OK | MB_ICONINFORMATION,
        );
    }
    Ok(())
}

fn main() {
    // 静默安装（供自动化测试 / 批量部署）：installer.exe --install-to "D://某目录"
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--install-to") {
        if let Some(dir) = args.get(i + 1) {
            match install(Path::new(dir), true) {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    let with_orders = cfg!(feature = "with-orders");
    let intro = if with_orders {
        "这个安装包会装上：程序 + 全部基础数据 + 当前订单和客户。\n\n\
         安装时可以选择目录；目标目录里如果已有 data\\app.db，会保留原数据不覆盖。\n\n\
         继续安装吗？"
    } else {
        "安装内容：程序 + 全部基础数据（69 个型号 / 1415 条型材公式 / 242 条玻璃公式、\
         螺丝规则、开孔参数、玻璃类型），**不含任何订单和客户**。\n\n\
         安装时可以选择目录；装完就是一个干净可用的新软件。\n\n继续安装吗？"
    };
    if !ask("门窗管理软件 · 安装", intro) {
        return;
    }

    let suggested = default_dir();
    let picked = pick_folder(
        "选择安装目录（建议装在 D 盘，不要装进 Program Files，否则可能写不进数据）",
        &suggested.to_string_lossy(),
    );
    let dir = match picked {
        Some(p) => p,
        None => {
            if ask(
                "没有选择目录",
                &format!(
                    "你没有选择安装目录。\n\n要使用默认目录吗？\n{}",
                    suggested.display()
                ),
            ) {
                suggested
            } else {
                return;
            }
        }
    };

    // 如果选的目录里已经有程序，确认是否覆盖
    if dir.join(APP_EXE_NAME).exists() {
        if !ask(
            "目录里已有程序",
            &format!(
                "{} 里已经有「{}」。\n\n继续会覆盖程序文件，但【不会】覆盖 data\\app.db 里的数据。\n\n继续吗？",
                dir.display(),
                APP_EXE_NAME
            ),
        ) {
            return;
        }
    }

    if let Err(e) = install(&dir, false) {
        msg("安装失败", &e, MB_OK | MB_ICONERROR);
        return;
    }

    if ask(
        "安装完成",
        "要现在打开软件吗？\n（首次启动会自动初始化数据，大约 1 秒）",
    ) {
        let exe = dir.join(APP_EXE_NAME);
        let _ = std::process::Command::new(&exe)
            .current_dir(&dir)
            .spawn();
    }
}

// 让 BOOL 的导入不被优化掉（BROWSEINFO 回调类型需要）
#[allow(dead_code)]
fn _keep_bool(_: BOOL) {}
