#![allow(dead_code)]
//! Setup bawaan: salin RustMusic.exe ke folder program, buat shortcut, daftar uninstall.

use std::path::{Path, PathBuf};
use std::process::Command;

const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\RustMusic";

const MK_LINKS: &str = "$w=New-Object -ComObject WScript.Shell; foreach($t in @((Join-Path ([Environment]::GetFolderPath('Desktop')) 'RustMusic.lnk'),(Join-Path ([Environment]::GetFolderPath('Programs')) 'RustMusic.lnk'))){ $s=$w.CreateShortcut($t); $s.TargetPath=$env:RM_EXE; $s.WorkingDirectory=$env:RM_DIR; $s.IconLocation=$env:RM_EXE; $s.Description='RustMusic'; $s.Save() }";

const RM_LINKS: &str = "foreach($t in @((Join-Path ([Environment]::GetFolderPath('Desktop')) 'RustMusic.lnk'),(Join-Path ([Environment]::GetFolderPath('Programs')) 'RustMusic.lnk'))){ Remove-Item -LiteralPath $t -Force -ErrorAction SilentlyContinue }";

/// Folder pasang: %LOCALAPPDATA%\Programs\RustMusic (tanpa hak admin).
fn install_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("Programs").join("RustMusic"))
        .unwrap_or_else(|| PathBuf::from("RustMusic"))
}

fn installed_exe() -> PathBuf {
    install_dir().join("RustMusic.exe")
}

#[cfg(windows)]
fn hide(c: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    c.creation_flags(0x0800_0000)
}

#[cfg(windows)]
fn run_ps(script: &str, exe: &Path, dir: &Path) -> Result<(), String> {
    let mut c = Command::new("powershell");
    c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("RM_EXE", exe)
        .env("RM_DIR", dir);
    let out = hide(&mut c)
        .output()
        .map_err(|e| format!("PowerShell tidak jalan: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(windows)]
fn do_install() -> Result<PathBuf, String> {
    let src = std::env::current_exe().map_err(|e| format!("{e}"))?;
    let dir = install_dir();
    let dst = installed_exe();
    std::fs::create_dir_all(&dir).map_err(|e| format!("buat folder gagal: {e}"))?;
    std::fs::copy(&src, &dst)
        .map_err(|e| format!("salin file gagal (tutup RustMusic yang sedang jalan dulu): {e}"))?;
    run_ps(MK_LINKS, &dst, &dir).map_err(|e| format!("buat shortcut gagal: {e}"))?;

    let dst_s = dst.to_string_lossy().to_string();
    let dir_s = dir.to_string_lossy().to_string();
    let un = format!("\"{}\" --uninstall", dst_s);
    let vals: [(&str, &str); 5] = [
        ("DisplayName", "RustMusic"),
        ("Publisher", "RustMusic"),
        ("DisplayIcon", dst_s.as_str()),
        ("InstallLocation", dir_s.as_str()),
        ("UninstallString", un.as_str()),
    ];
    for (name, val) in vals {
        let mut c = Command::new("reg");
        c.args(["add", UNINSTALL_KEY, "/v", name, "/t", "REG_SZ", "/d", val, "/f"]);
        let _ = hide(&mut c).status();
    }
    Ok(dst)
}

/// Ya kalau exe ini belum ada di folder pasang (jadi perlu tampil layar Setup).
#[tauri::command]
pub fn install_state() -> bool {
    #[cfg(not(windows))]
    {
        return false;
    }
    #[cfg(windows)]
    {
        let Ok(exe) = std::env::current_exe() else { return false };
        let s = exe.to_string_lossy().to_lowercase().replace('/', "\\");
        // Build dev (cargo run) tidak perlu setup.
        if s.contains("\\target\\") {
            return false;
        }
        s != installed_exe().to_string_lossy().to_lowercase()
    }
}

/// Tombol Setup: salin exe, buat shortcut, buka hasil pasang, tutup setup.
#[tauri::command]
pub async fn install_app(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = app;
        return Err("Setup hanya untuk Windows".into());
    }
    #[cfg(windows)]
    {
        let exe = tokio::task::spawn_blocking(do_install)
            .await
            .map_err(|e| e.to_string())??;
        Command::new(&exe)
            .spawn()
            .map_err(|e| format!("gagal membuka RustMusic: {e}"))?;
        app.exit(0);
        Ok(())
    }
}

/// `RustMusic.exe --uninstall`: hapus shortcut, entri uninstall, dan folder program.
pub fn uninstall() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let dir = install_dir();
        let exe = installed_exe();
        let _ = run_ps(RM_LINKS, &exe, &dir);
        let mut r = Command::new("reg");
        r.args(["delete", UNINSTALL_KEY, "/f"]);
        let _ = hide(&mut r).status();
        // Exe ini masih jalan, jadi hapus folder sesudah proses keluar.
        let mut c = Command::new("cmd");
        c.raw_arg(format!(
            "/C ping -n 3 127.0.0.1 >nul & rmdir /S /Q \"{}\"",
            dir.display()
        ));
        let _ = hide(&mut c).spawn();
    }
}
