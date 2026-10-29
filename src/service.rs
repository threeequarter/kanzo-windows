//! `kanzo service`: run the board in the background, started at login.
//! A LaunchAgent on macOS, a systemd user unit on Linux (a Raspberry Pi, an old laptop),
//! a Task Scheduler task on Windows.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::home::Home;

const LABEL: &str = "com.github.emanuele-em.kanzo";
const UNIT: &str = "kanzo.service";
const TASK: &str = "Kanzo";

pub fn install(home: &Home) -> Result<()> {
    let exe = std::env::current_exe().context("cannot find the kanzo program")?;
    if cfg!(target_os = "macos") {
        install_launchd(home, &exe)
    } else if cfg!(target_os = "linux") {
        install_systemd(home, &exe)
    } else if cfg!(windows) {
        install_task(home, &exe)
    } else {
        bail!("kanzo service supports macOS, Linux and Windows; start `kanzo --no-open` with your own service manager")
    }
}

pub fn uninstall() -> Result<()> {
    if cfg!(target_os = "macos") {
        uninstall_launchd()
    } else if cfg!(target_os = "linux") {
        uninstall_systemd()
    } else if cfg!(windows) {
        uninstall_task()
    } else {
        bail!("kanzo service supports macOS, Linux and Windows")
    }
}

/// Restarts the service, if it's installed, so it runs a newly installed Kanzo; false
/// when there's none.
pub fn restart() -> Result<bool> {
    if cfg!(target_os = "macos") && plist_path()?.exists() {
        run("launchctl", &["kickstart", "-k", &format!("{}/{LABEL}", launchd_domain()?)])?;
    } else if cfg!(target_os = "linux") && unit_path()?.exists() {
        run("systemctl", &["--user", "restart", UNIT])?;
    } else {
        return Ok(false);
    }
    Ok(true)
}

fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().context("cannot find your home folder")
}

/// Runs a command and fails with its own error output.
fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let out = Command::new(cmd).args(args).output().with_context(|| format!("cannot run {cmd}"))?;
    if !out.status.success() {
        bail!("{cmd} {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

// macOS

fn plist_path() -> Result<PathBuf> {
    Ok(home_dir()?.join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

fn launchd_domain() -> Result<String> {
    let uid = Command::new("id").arg("-u").output().context("cannot run id -u")?;
    Ok(format!("gui/{}", String::from_utf8_lossy(&uid.stdout).trim()))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn plist(exe: &Path, dir: &Path, log: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>--no-open</string>
        <string>--home</string>
        <string>{dir}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
</dict>
</plist>
"#,
        exe = xml_escape(&exe.to_string_lossy()),
        dir = xml_escape(&dir.to_string_lossy()),
        log = xml_escape(&log.to_string_lossy()),
    )
}

fn install_launchd(home: &Home, exe: &Path) -> Result<()> {
    let log = home_dir()?.join("Library/Logs/kanzo.log");
    let path = plist_path()?;
    std::fs::create_dir_all(path.parent().expect("plist path has a parent"))?;
    let domain = launchd_domain()?;
    // Replace an older install so a changed path or data folder takes effect.
    let _ = Command::new("launchctl").args(["bootout", &format!("{domain}/{LABEL}")]).output();
    std::fs::write(&path, plist(exe, &home.dir, &log)).with_context(|| format!("cannot write {}", path.display()))?;
    run("launchctl", &["bootstrap", &domain, &path.to_string_lossy()])?;
    println!(
        "Kanzo now starts at login and is running at http://localhost:{}\nLogs: {}",
        home.config.port,
        log.display()
    );
    Ok(())
}

fn uninstall_launchd() -> Result<()> {
    let path = plist_path()?;
    let _ = Command::new("launchctl").args(["bootout", &format!("{}/{LABEL}", launchd_domain()?)]).output();
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;
        println!("Removed the Kanzo LaunchAgent; the board no longer starts at login.");
    } else {
        println!("The Kanzo LaunchAgent was not installed.");
    }
    Ok(())
}

// Linux

fn unit_path() -> Result<PathBuf> {
    let config = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home_dir()?.join(".config"),
    };
    Ok(config.join("systemd/user").join(UNIT))
}

/// Quotes one argument for a systemd `ExecStart=` line.
fn systemd_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%"))
}

fn unit(exe: &Path, dir: &Path) -> String {
    format!(
        "[Unit]\n\
         Description=Kanzo board\n\
         After=network.target\n\
         \n\
         [Service]\n\
         ExecStart={} --no-open --home {}\n\
         Restart=on-failure\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        systemd_quote(&exe.to_string_lossy()),
        systemd_quote(&dir.to_string_lossy()),
    )
}

fn install_systemd(home: &Home, exe: &Path) -> Result<()> {
    let path = unit_path()?;
    std::fs::create_dir_all(path.parent().expect("unit path has a parent"))?;
    std::fs::write(&path, unit(exe, &home.dir)).with_context(|| format!("cannot write {}", path.display()))?;
    run("systemctl", &["--user", "daemon-reload"])?;
    run("systemctl", &["--user", "enable", UNIT])?;
    // Restart rather than start, so a changed path or data folder takes effect.
    run("systemctl", &["--user", "restart", UNIT])?;
    println!(
        "Kanzo now starts at login and is running at http://localhost:{}\n\
         Logs: journalctl --user -u kanzo\n\
         To start it at boot without logging in (a board with no screen): sudo loginctl enable-linger $USER",
        home.config.port
    );
    Ok(())
}

fn uninstall_systemd() -> Result<()> {
    let path = unit_path()?;
    let _ = Command::new("systemctl").args(["--user", "disable", "--now", UNIT]).output();
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;
        let _ = Command::new("systemctl").args(["--user", "daemon-reload"]).output();
        println!("Removed the Kanzo service; the board no longer starts at login.");
    } else {
        println!("The Kanzo service was not installed.");
    }
    Ok(())
}

// Windows

/// `%LOCALAPPDATA%\kanzo\kanzo.log`: the task has no console to write to.
fn log_path() -> Result<PathBuf> {
    Ok(dirs::data_local_dir().context("cannot find your local app data folder")?.join("kanzo").join("kanzo.log"))
}

/// Quotes one argument for a Windows command line.
fn windows_quote(s: &str) -> String {
    // A backslash before the closing quote would escape it, as in "C:\".
    let trailing = s.len() - s.trim_end_matches('\\').len();
    format!("\"{s}{}\"", "\\".repeat(trailing))
}

/// The task: started at this user's logon, as this user, without admin rights, so it
/// reaches the data folder and git's credentials. One copy at a time, no time limit,
/// on battery too.
fn task_xml(exe: &Path, dir: &Path, user: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Kanzo board</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>--background --no-open --home {dir}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        user = xml_escape(user),
        exe = xml_escape(&exe.to_string_lossy()),
        dir = xml_escape(&windows_quote(&dir.to_string_lossy())),
    )
}

fn install_task(home: &Home, exe: &Path) -> Result<()> {
    let var = |name: &str| std::env::var(name).with_context(|| format!("cannot find {name}"));
    let user = format!("{}\\{}", var("USERDOMAIN")?, var("USERNAME")?);
    // schtasks reads the file as UTF-16, the way Task Scheduler exports tasks.
    let mut xml = vec![0xFF, 0xFE];
    xml.extend(task_xml(exe, &home.dir, &user).encode_utf16().flat_map(u16::to_le_bytes));
    let file = std::env::temp_dir().join("kanzo-task.xml");
    std::fs::write(&file, xml).with_context(|| format!("cannot write {}", file.display()))?;
    // Replace an older install so a changed path or data folder takes effect.
    let _ = Command::new("schtasks").args(["/End", "/TN", TASK]).output();
    let created = run("schtasks", &["/Create", "/TN", TASK, "/XML", &file.to_string_lossy(), "/F"]);
    let _ = std::fs::remove_file(&file);
    created?;
    run("schtasks", &["/Run", "/TN", TASK])?;
    println!(
        "Kanzo now starts at login and is running at http://localhost:{}\nLogs: {}",
        home.config.port,
        log_path()?.display()
    );
    Ok(())
}

fn uninstall_task() -> Result<()> {
    let installed = Command::new("schtasks").args(["/Query", "/TN", TASK]).output().is_ok_and(|o| o.status.success());
    if !installed {
        println!("The Kanzo task was not installed.");
        return Ok(());
    }
    let _ = Command::new("schtasks").args(["/End", "/TN", TASK]).output();
    run("schtasks", &["/Delete", "/TN", TASK, "/F"])?;
    println!("Removed the Kanzo task; the board no longer starts at login.");
    Ok(())
}

/// For the task's `--background`: closes the console window Windows opens for it, and
/// sends what Kanzo prints to the log instead. Kanzo runs on without a log if it
/// can't open one.
#[cfg(windows)]
pub fn detach_console() {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{FreeConsole, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
    // SAFETY: no arguments; it only detaches this process from its console.
    unsafe { FreeConsole() };
    let Ok(path) = log_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(log) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else { return };
    // The handle stays open for the life of the process: Rust's stdout and stderr
    // look up the standard handles on every write.
    let handle = log.into_raw_handle();
    // SAFETY: `handle` is a valid file handle that is never closed.
    unsafe {
        SetStdHandle(STD_OUTPUT_HANDLE, handle);
        SetStdHandle(STD_ERROR_HANDLE, handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn plist_is_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("agent.plist");
        let text = plist(Path::new("/usr/local/bin/kanzo"), Path::new("/Users/a & b/kanzo"), Path::new("/tmp/k.log"));
        std::fs::write(&file, &text).unwrap();
        let lint = Command::new("plutil").arg("-lint").arg(&file).output().unwrap();
        assert!(lint.status.success(), "{}", String::from_utf8_lossy(&lint.stdout));
        assert!(text.contains("/Users/a &amp; b/kanzo"));
    }

    #[test]
    fn unit_quotes_paths() {
        let text = unit(Path::new("/home/pi/.cargo/bin/kanzo"), Path::new("/home/pi/my \"kanzo\" 100%"));
        assert!(
            text.contains(r#"ExecStart="/home/pi/.cargo/bin/kanzo" --no-open --home "/home/pi/my \"kanzo\" 100%%""#),
            "{text}"
        );
        assert!(text.contains("WantedBy=default.target"));
    }

    #[test]
    fn task_quotes_paths() {
        let text = task_xml(Path::new(r"C:\Users\a & b\.cargo\bin\kanzo.exe"), Path::new(r"D:\"), r"PC\a<b>");
        assert!(text.contains(r"<Command>C:\Users\a &amp; b\.cargo\bin\kanzo.exe</Command>"), "{text}");
        assert!(text.contains(r#"<Arguments>--background --no-open --home "D:\\"</Arguments>"#), "{text}");
        assert!(text.contains(r"<UserId>PC\a&lt;b&gt;</UserId>"), "{text}");
        assert_eq!(windows_quote(r"C:\Users\my kanzo"), r#""C:\Users\my kanzo""#);
    }
}
