//! 开机自启：写入 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`。

use anyhow::Result;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "porttool";
/// 自启动时带上这个参数：只显示托盘，不弹主窗口。
pub const AUTOSTART_ARG: &str = "--autostart";

pub fn is_enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_READ)
        .and_then(|key| key.get_value::<String, _>(VALUE_NAME))
        .is_ok()
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE)?;
    if enabled {
        key.set_value(VALUE_NAME, &current_command())?;
    } else if key.get_value::<String, _>(VALUE_NAME).is_ok() {
        key.delete_value(VALUE_NAME)?;
    }
    Ok(())
}

/// 当前程序的启动命令；路径已经过 [`scoop_current_path`] 归一。
fn current_command() -> String {
    let exe = scoop_current_path(&std::env::current_exe().unwrap_or_default());
    format!("\"{}\" {AUTOSTART_ARG}", exe.display())
}

/// 升级后自启动指向的最新安装位置。
///
/// Scoop 的应用目录带版本号（`apps\porttool\0.2.0\`），升级后旧目录会被清理；
/// 从资源管理器直接启动时 `current_exe()` 是带版本号的真实路径。这里把
/// `...\scoop\apps\<应用>\<版本>\` 归一成 `...\scoop\apps\<应用>\current\`，
/// 使注册表里的路径跨升级有效。路径里没有这个模式时原样返回。
pub fn scoop_current_path(exe: &std::path::Path) -> std::path::PathBuf {
    let parts: Vec<std::ffi::OsString> = exe
        .components()
        .map(|c| c.as_os_str().to_os_string())
        .collect();
    // `apps` 之后至少还有 <应用>\<版本>\ 和文件名三段
    let Some(i) = parts.iter().position(|c| c.eq_ignore_ascii_case("apps")) else {
        return exe.to_path_buf();
    };
    if i == 0
        || i + 3 >= parts.len()
        || parts[i + 2].eq_ignore_ascii_case("current")
        || !parts[i - 1].to_string_lossy().to_ascii_lowercase().ends_with("scoop")
    {
        return exe.to_path_buf();
    }
    let mut path = std::path::PathBuf::new();
    for (j, part) in parts.iter().enumerate() {
        path.push(if j == i + 2 { std::ffi::OsStr::new("current") } else { part });
    }
    path
}

/// 让注册表里的自启动路径跟上当前安装位置。
///
/// 在启动时调用：已经在用自启、但程序换了安装位置（例如 Scoop 升级到新版本目
/// 录、或改用 `--config` 之外的新路径运行）时，把 Run 键改成现在这个 exe 的归一路径。
pub fn refresh() {
    if !is_enabled() {
        return;
    }
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE)
    else {
        return;
    };
    match key.get_value::<String, _>(VALUE_NAME) {
        Ok(existing) if existing == current_command() => {}
        Ok(_) => {
            let _ = key.set_value(VALUE_NAME, &current_command());
        }
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn scoop_installed_exe_resolves_to_current() {
        let exe = Path::new(r"C:\Users\me\scoop\apps\porttool\0.2.0\porttoolw.exe");
        assert_eq!(
            scoop_current_path(exe),
            Path::new(r"C:\Users\me\scoop\apps\porttool\current\porttoolw.exe")
        );
        // 已经是 current 联接路径时保持不变
        let current = Path::new(r"C:\Users\me\scoop\apps\porttool\current\porttoolw.exe");
        assert_eq!(scoop_current_path(current), current);
    }

    #[test]
    fn non_scoop_paths_unchanged() {
        for path in [
            r"C:\Program Files\porttool\porttoolw.exe",
            r"D:\Develop\workspace\rust_dev\comhub\target\release\porttoolw.exe",
            // \apps\ 不在 scoop 目录下，不动它
            r"C:\tools\apps\porttool\0.2.0\porttoolw.exe",
        ] {
            let path = Path::new(path);
            assert_eq!(scoop_current_path(path), path, "{}", path.display());
        }
    }
}
