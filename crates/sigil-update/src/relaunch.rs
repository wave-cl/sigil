//! Starting the new sigil after this one has gone.
//!
//! The instance lock (`sigil_platform::instance`) is held until this
//! process exits, so the new one cannot simply be started now: it would
//! see the lock, say "already running", and quit. Instead a small shell
//! waits for this pid to disappear and then starts the new copy. It gives
//! up after a minute, so a process that will not die leaves nothing
//! waiting for ever -- the user sees the old sigil still there, which is
//! the truth.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::Install;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// `open -n <app>`: through Launch Services, so the new bundle is
    /// registered and gets the same treatment as a click in Finder.
    MacApp(PathBuf),
    /// The executable itself.
    Exe(PathBuf),
}

/// What to start afterwards, for this kind of install.
pub fn target(install: &Install) -> Option<Target> {
    match install {
        Install::MacBundle { app } => Some(Target::MacApp(app.clone())),
        Install::LinuxBinary { exe } => Some(Target::Exe(exe.clone())),
        // The path from startup, never `current_exe()` now: on Linux that
        // reads `/proc/self/exe`, which says `/usr/bin/sigil (deleted)` once
        // the package manager has replaced the file -- and the waiter, told
        // to exec that, exited, so Restart quit and started nothing.
        Install::LinuxPackage { exe, .. } => Some(Target::Exe(exe.clone())),
        Install::Unsupported { .. } => None,
    }
}

const WAIT_THEN: &str =
    "n=0; while kill -0 \"$1\" 2>/dev/null && [ $n -lt 300 ]; do sleep 0.2; n=$((n+1)); done; ";

/// The command, as argv. The pid and the path are positional parameters,
/// never spliced into the script: a path with a space or a quote in it is
/// still one word.
pub fn relaunch_command(pid: u32, target: &Target) -> Vec<String> {
    let (tail, path) = match target {
        Target::MacApp(app) => ("exec /usr/bin/open -n \"$2\"", app),
        Target::Exe(exe) => ("exec \"$2\"", exe),
    };
    vec![
        "/bin/sh".into(),
        "-c".into(),
        format!("{WAIT_THEN}{tail}"),
        "sigil-relaunch".into(),
        pid.to_string(),
        path.to_string_lossy().into_owned(),
    ]
}

/// Start the waiter, detached: its own process group and no inherited
/// pipes, so it outlives this process and nothing waits on it.
pub fn spawn_relaunch(pid: u32, target: &Target) -> std::io::Result<()> {
    let words = relaunch_command(pid, target);
    let mut command = Command::new(&words[0]);
    command
        .args(&words[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pid_and_the_path_are_words_of_their_own() {
        let app = PathBuf::from("/Applications/Some Body's Apps/Sigil.app");
        let words = relaunch_command(4242, &Target::MacApp(app.clone()));
        assert_eq!(words[0], "/bin/sh");
        assert_eq!(words[1], "-c");
        assert!(words[2].contains("kill -0 \"$1\""), "{}", words[2]);
        assert!(
            words[2].ends_with("exec /usr/bin/open -n \"$2\""),
            "{}",
            words[2]
        );
        assert!(!words[2].contains("4242") && !words[2].contains("Sigil.app"));
        assert_eq!(words[3], "sigil-relaunch");
        assert_eq!(words[4], "4242");
        assert_eq!(words[5], app.to_str().unwrap());

        let exe = PathBuf::from("/home/c/bin/sigil");
        let words = relaunch_command(7, &Target::Exe(exe));
        assert!(words[2].ends_with("exec \"$2\""), "{}", words[2]);
        assert_eq!(words[5], "/home/c/bin/sigil");
    }

    #[test]
    fn the_waiter_gives_up() {
        let words = relaunch_command(1, &Target::Exe("/x".into()));
        assert!(words[2].contains("[ $n -lt 300 ]"), "{}", words[2]);
    }

    #[test]
    fn each_install_knows_what_to_start() {
        assert_eq!(
            target(&Install::MacBundle {
                app: "/Applications/Sigil.app".into()
            }),
            Some(Target::MacApp("/Applications/Sigil.app".into()))
        );
        assert_eq!(
            target(&Install::LinuxBinary {
                exe: "/home/c/bin/sigil".into()
            }),
            Some(Target::Exe("/home/c/bin/sigil".into()))
        );
        assert_eq!(
            target(&Install::LinuxPackage {
                kind: crate::install::PackageKind::Deb,
                exe: "/usr/bin/sigil".into()
            }),
            Some(Target::Exe("/usr/bin/sigil".into())),
            "the path recorded at startup, not one read now"
        );
        assert_eq!(target(&Install::Unsupported { why: "no".into() }), None);
    }

    /// The script really waits for the pid and really runs the target:
    /// a `sleep` stands in for sigil, and the target writes a file.
    #[cfg(unix)]
    #[test]
    fn the_waiter_starts_the_target_after_the_pid_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let mark = dir.path().join("started");
        let script = dir.path().join("target.sh");
        std::fs::write(&script, format!("#!/bin/sh\ntouch '{}'\n", mark.display())).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut stand_in = Command::new("sleep").arg("0.5").spawn().unwrap();
        spawn_relaunch(stand_in.id(), &Target::Exe(script)).unwrap();
        assert!(!mark.exists(), "not before the pid is gone");
        stand_in.wait().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !mark.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(mark.exists(), "the target ran once the pid was gone");
    }
}
