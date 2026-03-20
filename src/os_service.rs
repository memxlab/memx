use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn install(binary_path: &Path, memx_home: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        install_macos(binary_path, memx_home)
    }

    #[cfg(target_os = "linux")]
    {
        install_linux(binary_path, memx_home)
    }

    #[cfg(target_os = "windows")]
    {
        install_windows(binary_path, memx_home)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (binary_path, memx_home);
        bail!("Background service is not supported on this platform")
    }
}

pub fn is_installed() -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        Ok(macos_plist_path()?.exists())
    }

    #[cfg(target_os = "linux")]
    {
        Ok(linux_unit_path()?.exists())
    }

    #[cfg(target_os = "windows")]
    {
        Ok(windows_task_exists())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Ok(false)
    }
}

pub fn start() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        start_macos()
    }

    #[cfg(target_os = "linux")]
    {
        start_linux()
    }

    #[cfg(target_os = "windows")]
    {
        start_windows()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        bail!("Background service is not supported on this platform")
    }
}

pub fn stop() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        stop_macos()
    }

    #[cfg(target_os = "linux")]
    {
        stop_linux()
    }

    #[cfg(target_os = "windows")]
    {
        stop_windows()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        bail!("Background service is not supported on this platform")
    }
}

pub fn remove() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        remove_macos()
    }

    #[cfg(target_os = "linux")]
    {
        remove_linux()
    }

    #[cfg(target_os = "windows")]
    {
        remove_windows()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        bail!("Background service is not supported on this platform")
    }
}

pub fn status() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        status_macos()
    }

    #[cfg(target_os = "linux")]
    {
        status_linux()
    }

    #[cfg(target_os = "windows")]
    {
        status_windows()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        bail!("Background service is not supported on this platform")
    }
}

pub fn service_manager_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "launchd"
    }

    #[cfg(target_os = "linux")]
    {
        "systemd --user"
    }

    #[cfg(target_os = "windows")]
    {
        "Task Scheduler"
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "unsupported"
    }
}

#[cfg(target_os = "linux")]
fn install_linux(binary_path: &Path, memx_home: &Path) -> Result<()> {
    let unit_path = linux_unit_path()?;
    if let Some(parent) = unit_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    fs::write(&unit_path, render_linux_unit(binary_path, memx_home))
        .with_context(|| format!("Failed to write {}", unit_path.display()))?;

    run_command("systemctl", ["--user", "daemon-reload"])?;
    run_command("systemctl", ["--user", "enable", linux_service_name()])?;
    start_linux()
}

#[cfg(target_os = "linux")]
fn start_linux() -> Result<()> {
    run_command("systemctl", ["--user", "restart", linux_service_name()])?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn stop_linux() -> Result<()> {
    run_command("systemctl", ["--user", "stop", linux_service_name()])?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn remove_linux() -> Result<()> {
    let unit_path = linux_unit_path()?;

    let _ = run_command(
        "systemctl",
        ["--user", "disable", "--now", linux_service_name()],
    );

    if unit_path.exists() {
        fs::remove_file(&unit_path)
            .with_context(|| format!("Failed to remove {}", unit_path.display()))?;
    }

    run_command("systemctl", ["--user", "daemon-reload"])?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn status_linux() -> Result<String> {
    run_command(
        "systemctl",
        [
            "--user",
            "--no-pager",
            "--full",
            "status",
            linux_service_name(),
        ],
    )
}

#[cfg(target_os = "linux")]
fn linux_unit_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    Ok(home.join(".config/systemd/user").join(linux_service_name()))
}

#[cfg(target_os = "linux")]
fn linux_service_name() -> &'static str {
    "memx.service"
}

#[cfg(target_os = "linux")]
fn render_linux_unit(binary_path: &Path, memx_home: &Path) -> String {
    format!(
        "[Unit]\nDescription=MemX background service\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nEnvironment=\"MEMX_HOME={}\"\nExecStart=\"{}\" serve\nRestart=always\nRestartSec=3\n\n[Install]\nWantedBy=default.target\n",
        systemd_escape(memx_home),
        systemd_escape(binary_path)
    )
}

#[cfg(target_os = "macos")]
fn install_macos(binary_path: &Path, memx_home: &Path) -> Result<()> {
    let plist_path = macos_plist_path()?;
    if let Some(parent) = plist_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let logs_dir = memx_home.join("logs");
    fs::create_dir_all(&logs_dir)
        .with_context(|| format!("Failed to create {}", logs_dir.display()))?;

    fs::write(&plist_path, render_macos_plist(binary_path, memx_home))
        .with_context(|| format!("Failed to write {}", plist_path.display()))?;

    let bootstrap_target = macos_launchctl_target()?;
    let _ = run_command(
        "launchctl",
        [
            "bootout",
            &bootstrap_target,
            plist_path.to_string_lossy().as_ref(),
        ],
    );
    run_command(
        "launchctl",
        [
            "bootstrap",
            &bootstrap_target,
            plist_path.to_string_lossy().as_ref(),
        ],
    )?;
    run_command(
        "launchctl",
        [
            "kickstart",
            "-k",
            &format!("{bootstrap_target}/{}", macos_label()),
        ],
    )?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn start_macos() -> Result<()> {
    let bootstrap_target = macos_launchctl_target()?;
    run_command(
        "launchctl",
        [
            "kickstart",
            "-k",
            &format!("{bootstrap_target}/{}", macos_label()),
        ],
    )?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn stop_macos() -> Result<()> {
    let bootstrap_target = macos_launchctl_target()?;
    run_command(
        "launchctl",
        [
            "bootout",
            &bootstrap_target,
            &format!("{bootstrap_target}/{}", macos_label()),
        ],
    )?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn remove_macos() -> Result<()> {
    let plist_path = macos_plist_path()?;
    let bootstrap_target = macos_launchctl_target()?;
    let _ = run_command(
        "launchctl",
        [
            "bootout",
            &bootstrap_target,
            &format!("{bootstrap_target}/{}", macos_label()),
        ],
    );

    if plist_path.exists() {
        fs::remove_file(&plist_path)
            .with_context(|| format!("Failed to remove {}", plist_path.display()))?;
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn status_macos() -> Result<String> {
    let bootstrap_target = macos_launchctl_target()?;
    run_command(
        "launchctl",
        ["print", &format!("{bootstrap_target}/{}", macos_label())],
    )
}

#[cfg(target_os = "macos")]
fn macos_plist_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    Ok(home
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", macos_label())))
}

#[cfg(target_os = "macos")]
fn macos_label() -> &'static str {
    "com.memx.memx"
}

#[cfg(target_os = "macos")]
fn macos_launchctl_target() -> Result<String> {
    let uid = run_command("id", ["-u"])?;
    Ok(format!("gui/{}", uid.trim()))
}

#[cfg(target_os = "macos")]
fn render_macos_plist(binary_path: &Path, memx_home: &Path) -> String {
    let stdout_path = memx_home.join("logs/memx-service.out.log");
    let stderr_path = memx_home.join("logs/memx-service.err.log");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>{label}</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>{binary}</string>\n    <string>serve</string>\n  </array>\n  <key>EnvironmentVariables</key>\n  <dict>\n    <key>MEMX_HOME</key>\n    <string>{memx_home}</string>\n  </dict>\n  <key>RunAtLoad</key>\n  <true/>\n  <key>KeepAlive</key>\n  <true/>\n  <key>StandardOutPath</key>\n  <string>{stdout_path}</string>\n  <key>StandardErrorPath</key>\n  <string>{stderr_path}</string>\n</dict>\n</plist>\n",
        label = xml_escape(macos_label()),
        binary = xml_escape(&binary_path.to_string_lossy()),
        memx_home = xml_escape(&memx_home.to_string_lossy()),
        stdout_path = xml_escape(&stdout_path.to_string_lossy()),
        stderr_path = xml_escape(&stderr_path.to_string_lossy()),
    )
}

#[cfg(target_os = "windows")]
fn install_windows(binary_path: &Path, memx_home: &Path) -> Result<()> {
    run_command(
        "schtasks",
        [
            "/Create",
            "/TN",
            windows_task_name(),
            "/SC",
            "ONLOGON",
            "/TR",
            &windows_task_command(binary_path, memx_home),
            "/F",
        ],
    )?;
    start_windows()
}

#[cfg(target_os = "windows")]
fn start_windows() -> Result<()> {
    run_command("schtasks", ["/Run", "/TN", windows_task_name()])?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn stop_windows() -> Result<()> {
    run_command("schtasks", ["/End", "/TN", windows_task_name()])?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn remove_windows() -> Result<()> {
    let _ = run_command("schtasks", ["/End", "/TN", windows_task_name()]);
    run_command("schtasks", ["/Delete", "/TN", windows_task_name(), "/F"])?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn status_windows() -> Result<String> {
    run_command(
        "schtasks",
        ["/Query", "/TN", windows_task_name(), "/FO", "LIST"],
    )
}

#[cfg(target_os = "windows")]
fn windows_task_name() -> &'static str {
    "MemX"
}

#[cfg(target_os = "windows")]
fn windows_task_command(binary_path: &Path, memx_home: &Path) -> String {
    format!(
        "cmd.exe /C \"set \\\"MEMX_HOME={}\\\" && \\\"{}\\\" serve\"",
        memx_home.display(),
        binary_path.display()
    )
}

#[cfg(target_os = "windows")]
fn windows_task_exists() -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", windows_task_name()])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn run_command<const N: usize>(program: &str, args: [&str; N]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run {program}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        if !stdout.is_empty() {
            return Ok(stdout);
        }
        return Ok(stderr);
    }

    let details = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "no output".to_string()
    };

    bail!("{program} failed: {details}")
}

#[cfg(target_os = "linux")]
fn systemd_escape(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[cfg(target_os = "linux")]
    use super::render_linux_unit;
    #[cfg(target_os = "macos")]
    use super::render_macos_plist;
    #[cfg(target_os = "windows")]
    use super::windows_task_command;

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_unit_contains_binary_and_memx_home() {
        let unit = render_linux_unit(
            Path::new("/Users/demo/.local/bin/memx"),
            Path::new("/Users/demo/.memx"),
        );

        assert!(unit.contains("ExecStart=\"/Users/demo/.local/bin/memx\" serve"));
        assert!(unit.contains("Environment=\"MEMX_HOME=/Users/demo/.memx\""));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_plist_contains_binary_and_memx_home() {
        let plist = render_macos_plist(
            Path::new("/Users/demo/.local/bin/memx"),
            Path::new("/Users/demo/.memx"),
        );

        assert!(plist.contains("<string>/Users/demo/.local/bin/memx</string>"));
        assert!(plist.contains("<string>/Users/demo/.memx</string>"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_task_command_contains_binary_and_memx_home() {
        let command = windows_task_command(
            Path::new(r"C:\Users\demo\AppData\Local\MemX\bin\memx.exe"),
            Path::new(r"C:\Users\demo\.memx"),
        );

        assert!(command.contains(r#"MEMX_HOME=C:\Users\demo\.memx"#));
        assert!(command.contains(r#"memx.exe" serve"#));
    }
}
