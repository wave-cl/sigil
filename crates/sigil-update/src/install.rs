//! How this copy of sigil was installed, and therefore how a newer one
//! takes its place.
//!
//! Three shapes: a macOS bundle, which is swapped whole; a bare Linux
//! binary somewhere the user can write, which is replaced beside itself;
//! and a Linux package, which the package manager replaces under `pkexec`
//! because `/usr/bin` is root's. Anything else -- `target/release`, a
//! platform with no updater -- is `Unsupported`, and the pane says so in
//! words rather than offering a button that would fail.
//!
//! **Nothing here writes over a running binary.** A new file is put beside
//! the old and renamed into place, so the running process keeps the inode
//! it started with. Overwriting in place corrupts the process on Linux and
//! gets it killed on macOS when the signature no longer matches.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageKind {
    Deb,
    Rpm,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// `/Applications/Sigil.app`, or wherever the bundle is.
    MacBundle { app: PathBuf },
    /// A tarball's binary, somewhere the user can write.
    LinuxBinary { exe: PathBuf },
    /// `/usr/bin/sigil`, from a .deb or .rpm.
    LinuxPackage { kind: PackageKind },
    /// Why sigil cannot update itself here.
    Unsupported { why: String },
}

impl Install {
    /// One sentence for the Desktop pane.
    pub fn describe(&self) -> String {
        match self {
            Install::MacBundle { app } => format!("installed as {}", app.display()),
            Install::LinuxBinary { exe } => format!("installed as {}", exe.display()),
            Install::LinuxPackage {
                kind: PackageKind::Deb,
            } => "installed as a .deb package".into(),
            Install::LinuxPackage {
                kind: PackageKind::Rpm,
            } => "installed as an .rpm package".into(),
            Install::Unsupported { why } => why.clone(),
        }
    }

    pub fn is_supported(&self) -> bool {
        !matches!(self, Install::Unsupported { .. })
    }
}

/// Read the real machine.
pub fn detect() -> Install {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            return Install::Unsupported {
                why: format!("sigil cannot tell where it is running from ({e})"),
            };
        }
    };
    let dir_writable = exe.parent().map(dir_is_writable).unwrap_or(false);
    let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    classify(
        std::env::consts::OS,
        &exe,
        dir_writable,
        &os_release,
        on_path("dpkg"),
        on_path("rpm"),
    )
}

/// The decision, over plain facts, so every branch can be tested on any
/// machine.
pub fn classify(
    os: &str,
    exe: &Path,
    dir_writable: bool,
    os_release: &str,
    has_dpkg: bool,
    has_rpm: bool,
) -> Install {
    match os {
        "macos" => match bundle_of(exe) {
            Some(app) => Install::MacBundle { app },
            None => Install::Unsupported {
                why: format!(
                    "running from {}, not from a .app bundle, so sigil cannot update itself",
                    exe.parent().unwrap_or(exe).display()
                ),
            },
        },
        "linux" => {
            if exe.starts_with("/usr") {
                match package_kind(os_release, has_dpkg, has_rpm) {
                    Some(kind) => Install::LinuxPackage { kind },
                    None => Install::Unsupported {
                        why: format!(
                            "installed at {}, and neither dpkg nor rpm is here to replace it",
                            exe.display()
                        ),
                    },
                }
            } else if dir_writable {
                Install::LinuxBinary {
                    exe: exe.to_path_buf(),
                }
            } else {
                Install::Unsupported {
                    why: format!(
                        "sigil cannot write to {}, so it cannot update itself",
                        exe.parent().unwrap_or(exe).display()
                    ),
                }
            }
        }
        other => Install::Unsupported {
            why: format!("sigil does not update itself on {other}"),
        },
    }
}

/// The `.app` an executable at `<app>/Contents/MacOS/<name>` belongs to.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app")
        .then(|| app.to_path_buf())
}

/// Which package manager owns `/usr/bin/sigil`: what `/etc/os-release`
/// says first, and which tool is present only when it says nothing.
pub fn package_kind(os_release: &str, has_dpkg: bool, has_rpm: bool) -> Option<PackageKind> {
    let ids: Vec<String> = os_release
        .lines()
        .filter_map(|l| l.strip_prefix("ID=").or_else(|| l.strip_prefix("ID_LIKE=")))
        .flat_map(|v| {
            v.trim_matches('"')
                .split_whitespace()
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        })
        .collect();
    for id in &ids {
        match id.as_str() {
            "debian" | "ubuntu" => return Some(PackageKind::Deb),
            "fedora" | "rhel" | "centos" | "suse" | "opensuse" => return Some(PackageKind::Rpm),
            _ => {}
        }
    }
    if has_dpkg {
        Some(PackageKind::Deb)
    } else if has_rpm {
        Some(PackageKind::Rpm)
    } else {
        None
    }
}

/// The command that installs a package, asking for authorisation through
/// polkit. The front end (apt-get, dnf) rather than the back end (dpkg,
/// rpm) because the front end resolves the package's dependencies; the back
/// end only when the front end is missing.
pub fn package_command(
    kind: PackageKind,
    file: &Path,
    has_apt_get: bool,
    has_dnf: bool,
) -> Vec<String> {
    assert!(
        file.is_absolute(),
        "pkexec runs elsewhere: the path must be absolute"
    );
    let file = file.to_string_lossy().into_owned();
    let words: &[&str] = match (kind, has_apt_get, has_dnf) {
        (PackageKind::Deb, true, _) => &["pkexec", "apt-get", "install", "-y"],
        (PackageKind::Deb, false, _) => &["pkexec", "dpkg", "-i"],
        (PackageKind::Rpm, _, true) => &["pkexec", "dnf", "install", "-y"],
        (PackageKind::Rpm, _, false) => &["pkexec", "rpm", "-U"],
    };
    words
        .iter()
        .map(|w| (*w).to_owned())
        .chain(std::iter::once(file))
        .collect()
}

/// Unpack a downloaded archive into `into` and say where the payload is:
/// `into/sigil` for a tarball, `into/Sigil.app` for a zip.
///
/// `tar` and `ditto` rather than a crate: both are on every machine sigil
/// runs on, and `ditto` is the only thing that puts a bundle back together
/// with its symlinks and its signature intact -- it is what made the zip.
pub fn unpack(archive: &Path, into: &Path) -> Result<PathBuf, Error> {
    std::fs::create_dir_all(into)?;
    let name = archive.to_string_lossy();
    let (status, payload) = if name.ends_with(".tar.gz") {
        (
            Command::new("tar")
                .arg("-xzf")
                .arg(archive)
                .arg("-C")
                .arg(into)
                .status(),
            into.join("sigil"),
        )
    } else if name.ends_with(".zip") {
        (
            Command::new("ditto")
                .arg("-x")
                .arg("-k")
                .arg(archive)
                .arg(into)
                .status(),
            into.join("Sigil.app"),
        )
    } else {
        return Err(Error::Install(format!(
            "{name} is not an archive sigil knows how to open"
        )));
    };
    let status = status.map_err(|e| Error::Install(format!("could not unpack {name}: {e}")))?;
    if !status.success() {
        return Err(Error::Install(format!(
            "unpacking {name} failed ({status})"
        )));
    }
    if !payload.exists() {
        return Err(Error::Install(format!(
            "{name} does not contain {}",
            payload.file_name().unwrap_or_default().to_string_lossy()
        )));
    }
    Ok(payload)
}

/// Put `new_binary` where `exe` is: copied to `<exe>.new` (a fresh inode),
/// made executable, renamed over. The running process keeps the old inode.
pub fn install_linux_binary(new_binary: &Path, exe: &Path) -> Result<(), Error> {
    let staged = exe.with_extension("new");
    let _ = std::fs::remove_file(&staged);
    std::fs::copy(new_binary, &staged)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&staged, exe)?;
    Ok(())
}

/// The bundle that the last update moved aside, if any.
pub fn previous_bundle(app: &Path) -> PathBuf {
    let mut name = app.file_name().unwrap_or_default().to_os_string();
    // `Sigil.app.previous`, not `Sigil.previous.app`: Launch Services indexes
    // anything ending in .app, and a second sigil registered for the
    // `sigil://` scheme is a coin toss over which one opens a link.
    name.push(".previous");
    app.with_file_name(name)
}

/// Swap `new_app` in for `app`, keeping the old bundle beside it as
/// `<app>.previous` until the new one has started once.
///
/// Both renames are on one volume when the caller staged on the app's own
/// -- see [`crate::download::staging_dir_beside`] -- so each is atomic and
/// there is never a moment with no bundle at the path. On failure the old
/// bundle is put back.
pub fn install_mac_bundle(new_app: &Path, app: &Path) -> Result<(), Error> {
    let exe = new_app.join("Contents/MacOS/sigil");
    if !exe.is_file() || !new_app.join("Contents/Info.plist").is_file() {
        return Err(Error::Install(format!(
            "{} is not a sigil bundle",
            new_app.display()
        )));
    }
    // A download made by this process carries no quarantine flag; one that
    // came another way might. Harmless when there is nothing to remove.
    let _ = Command::new("xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(new_app)
        .status();
    let previous = previous_bundle(app);
    if previous.exists() {
        std::fs::remove_dir_all(&previous)?;
    }
    std::fs::rename(app, &previous)?;
    if let Err(e) = std::fs::rename(new_app, app) {
        let _ = std::fs::rename(&previous, app);
        return Err(Error::Install(format!(
            "could not put the new bundle at {}: {e}",
            app.display()
        )));
    }
    // The release's spelling of the name, when the installed one differs
    // only in case -- `Sigil.app` from before it was `Sigil.app`. Finder
    // shows the file name, and the volume is case-insensitive, so this is a
    // rename to what is already the same file; if it is not, nothing is
    // lost by leaving it.
    let mut placed = app.to_path_buf();
    if let (Some(theirs), Some(ours)) = (new_app.file_name(), app.file_name())
        && theirs != ours
        && theirs
            .to_string_lossy()
            .eq_ignore_ascii_case(&ours.to_string_lossy())
    {
        let spelt = app.with_file_name(theirs);
        if std::fs::rename(app, &spelt).is_ok() {
            placed = spelt;
        }
    }
    // Tell Launch Services the bundle at this path has changed. It keeps
    // the name, the version and the icon from when it last looked, and the
    // relaunch that follows is `open`, which asks it: without this the new
    // copy came up in the Dock under the old name with a placeholder for an
    // icon. Best effort -- the bundle is in place either way.
    let _ = std::process::Command::new(LSREGISTER)
        .arg("-f")
        .arg(&placed)
        .status();
    Ok(())
}

const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

/// At a successful start of a bundle: the bundle the last update moved
/// aside is no longer a way back, so it goes. Nothing is touched when the
/// executable is not inside a `.app`.
pub fn remove_previous_bundle(exe: &Path) -> Option<PathBuf> {
    let app = bundle_of(exe)?;
    let previous = previous_bundle(&app);
    if !previous.is_dir() {
        return None;
    }
    match std::fs::remove_dir_all(&previous) {
        Ok(()) => Some(previous),
        Err(e) => {
            tracing::warn!("could not remove {}: {e}", previous.display());
            None
        }
    }
}

/// Hand a package to the package manager, through polkit.
pub fn install_package(kind: PackageKind, file: &Path) -> Result<(), Error> {
    let words = package_command(kind, file, on_path("apt-get"), on_path("dnf"));
    let status = Command::new(&words[0])
        .args(&words[1..])
        .status()
        .map_err(|e| Error::Install(format!("could not run pkexec: {e}")))?;
    match status.code() {
        Some(0) => Ok(()),
        // pkexec's own two: authorisation refused or dismissed, and not
        // installed at all.
        Some(126) => Err(Error::Install("authorisation was cancelled".into())),
        Some(127) => Err(Error::Install("pkexec is not installed".into())),
        _ => Err(Error::Install(format!(
            "{} failed ({status})",
            words[1..].join(" ")
        ))),
    }
}

fn dir_is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".sigil-write-probe-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEBIAN: &str = "PRETTY_NAME=\"Ubuntu 24.04\"\nID=ubuntu\nID_LIKE=debian\n";
    const FEDORA: &str = "NAME=\"Fedora Linux\"\nID=fedora\n";

    #[test]
    fn an_executable_inside_a_bundle_is_a_bundle_install() {
        let exe = Path::new("/Applications/Sigil.app/Contents/MacOS/sigil");
        assert_eq!(
            classify("macos", exe, true, "", false, false),
            Install::MacBundle {
                app: PathBuf::from("/Applications/Sigil.app")
            }
        );
        let dev = Path::new("/Users/c/projects/sigil/target/release/sigil");
        match classify("macos", dev, true, "", false, false) {
            Install::Unsupported { why } => assert!(why.contains("not from a .app"), "{why}"),
            other => panic!("{other:?}"),
        }
        // Inside the tree but not at the executable's place.
        let odd = Path::new("/Applications/Sigil.app/Contents/Resources/sigil");
        assert!(!classify("macos", odd, true, "", false, false).is_supported());
    }

    #[test]
    fn a_linux_binary_under_usr_is_a_package_and_elsewhere_is_itself() {
        let usr = Path::new("/usr/bin/sigil");
        assert_eq!(
            classify("linux", usr, false, DEBIAN, true, false),
            Install::LinuxPackage {
                kind: PackageKind::Deb
            }
        );
        assert_eq!(
            classify("linux", usr, false, FEDORA, false, true),
            Install::LinuxPackage {
                kind: PackageKind::Rpm
            }
        );
        // Writable /usr/bin means root; the package is still the truth.
        assert_eq!(
            classify("linux", usr, true, DEBIAN, true, false),
            Install::LinuxPackage {
                kind: PackageKind::Deb
            }
        );
        assert!(!classify("linux", usr, false, "", false, false).is_supported());

        let home = Path::new("/home/c/bin/sigil");
        assert_eq!(
            classify("linux", home, true, DEBIAN, true, false),
            Install::LinuxBinary {
                exe: home.to_path_buf()
            }
        );
        match classify("linux", home, false, DEBIAN, true, false) {
            Install::Unsupported { why } => assert!(why.contains("cannot write"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert!(!classify("windows", home, true, "", false, false).is_supported());
    }

    #[test]
    fn os_release_decides_the_package_kind_before_which_tool_is_present() {
        // A Debian box with rpm installed as a tool is still Debian.
        assert_eq!(package_kind(DEBIAN, false, true), Some(PackageKind::Deb));
        assert_eq!(package_kind(FEDORA, true, false), Some(PackageKind::Rpm));
        assert_eq!(
            package_kind("ID=\"centos\"\n", false, false),
            Some(PackageKind::Rpm)
        );
        assert_eq!(
            package_kind("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n", false, false),
            Some(PackageKind::Deb)
        );
        // Says nothing: whichever tool is here.
        assert_eq!(package_kind("", true, false), Some(PackageKind::Deb));
        assert_eq!(package_kind("", false, true), Some(PackageKind::Rpm));
        assert_eq!(package_kind("ID=arch\n", false, false), None);
    }

    #[test]
    fn the_package_command_prefers_the_front_end() {
        let deb = Path::new("/home/c/.local/share/sigil/updates/sigil-v0.1.6-x86_64-linux-gnu.deb");
        assert_eq!(
            package_command(PackageKind::Deb, deb, true, false),
            ["pkexec", "apt-get", "install", "-y", deb.to_str().unwrap()]
        );
        assert_eq!(
            package_command(PackageKind::Deb, deb, false, false),
            ["pkexec", "dpkg", "-i", deb.to_str().unwrap()]
        );
        let rpm = Path::new("/tmp/sigil.rpm");
        assert_eq!(
            package_command(PackageKind::Rpm, rpm, false, true),
            ["pkexec", "dnf", "install", "-y", "/tmp/sigil.rpm"]
        );
        assert_eq!(
            package_command(PackageKind::Rpm, rpm, true, false),
            ["pkexec", "rpm", "-U", "/tmp/sigil.rpm"]
        );
    }

    #[test]
    #[should_panic(expected = "absolute")]
    fn the_package_command_refuses_a_relative_path() {
        package_command(PackageKind::Deb, Path::new("sigil.deb"), true, false);
    }

    #[test]
    fn the_previous_bundle_does_not_end_in_dot_app() {
        assert_eq!(
            previous_bundle(Path::new("/Applications/Sigil.app")),
            PathBuf::from("/Applications/Sigil.app.previous")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_binary_is_replaced_on_a_fresh_inode_and_the_old_one_still_reads() {
        use std::io::Read;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("sigil");
        std::fs::write(&exe, b"old").unwrap();
        let before = std::fs::metadata(&exe).unwrap().ino();
        // Held open, as a running process holds its own executable.
        let mut running = std::fs::File::open(&exe).unwrap();

        let new = dir.path().join("unpacked").join("sigil");
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::write(&new, b"new bytes").unwrap();
        install_linux_binary(&new, &exe).unwrap();

        let meta = std::fs::metadata(&exe).unwrap();
        assert_ne!(meta.ino(), before, "a fresh inode, never the running one");
        assert_eq!(meta.permissions().mode() & 0o777, 0o755);
        assert_eq!(std::fs::read(&exe).unwrap(), b"new bytes");
        assert!(!exe.with_extension("new").exists());
        let mut still = String::new();
        running.read_to_string(&mut still).unwrap();
        assert_eq!(still, "old", "the running inode is untouched");
    }

    /// The swap itself is renames, and runs anywhere; the zip round trip
    /// through `ditto` is macOS's own and is taken only there.
    #[test]
    fn a_bundle_is_swapped_whole_and_the_old_one_kept_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Sigil.app");
        let make = |at: &Path, body: &str| {
            std::fs::create_dir_all(at.join("Contents/MacOS")).unwrap();
            std::fs::write(at.join("Contents/MacOS/sigil"), body).unwrap();
            std::fs::write(at.join("Contents/Info.plist"), "<plist/>").unwrap();
        };
        make(&app, "old");
        let src = dir.path().join("src");
        make(&src.join("Sigil.app"), "new");
        let staging = dir.path().join("staging");
        let new_app = if cfg!(target_os = "macos") {
            // The new one arrives as a zip, exactly as the release makes it.
            let zip = dir.path().join("sigil-v9.9.9-aarch64-apple-darwin.zip");
            assert!(
                Command::new("ditto")
                    .args(["-c", "-k", "--keepParent"])
                    .arg(src.join("Sigil.app"))
                    .arg(&zip)
                    .status()
                    .unwrap()
                    .success()
            );
            let new_app = unpack(&zip, &staging).unwrap();
            assert_eq!(new_app, staging.join("Sigil.app"));
            new_app
        } else {
            std::fs::create_dir_all(&staging).unwrap();
            std::fs::rename(src.join("Sigil.app"), staging.join("Sigil.app")).unwrap();
            staging.join("Sigil.app")
        };

        install_mac_bundle(&new_app, &app).unwrap();
        assert_eq!(
            std::fs::read_to_string(app.join("Contents/MacOS/sigil")).unwrap(),
            "new"
        );
        let previous = previous_bundle(&app);
        assert_eq!(
            std::fs::read_to_string(previous.join("Contents/MacOS/sigil")).unwrap(),
            "old"
        );
        assert!(!new_app.exists(), "moved, not copied");

        // The next start of the new bundle removes the way back -- but only
        // for an executable that is inside a bundle.
        assert_eq!(remove_previous_bundle(&dir.path().join("sigil")), None);
        assert!(previous.exists());
        assert_eq!(
            remove_previous_bundle(&app.join("Contents/MacOS/sigil")),
            Some(previous.clone())
        );
        assert!(!previous.exists());
        assert_eq!(
            remove_previous_bundle(&app.join("Contents/MacOS/sigil")),
            None
        );
    }

    /// An install spelt `sigil.app` from before the name was `Sigil.app`
    /// takes the release's spelling when the new bundle differs only in
    /// case -- on a volume that treats them as one file.
    #[test]
    fn an_old_spelling_of_the_bundle_takes_the_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let make = |at: &Path, body: &str| {
            std::fs::create_dir_all(at.join("Contents/MacOS")).unwrap();
            std::fs::write(at.join("Contents/MacOS/sigil"), body).unwrap();
            std::fs::write(at.join("Contents/Info.plist"), "<plist/>").unwrap();
        };
        let old = dir.path().join("sigil.app");
        make(&old, "old");
        let new = dir.path().join("staging").join("Sigil.app");
        make(&new, "new");
        install_mac_bundle(&new, &old).unwrap();
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".app"))
            .collect();
        // Only on a case-insensitive volume is the spelling ours to choose;
        // either way the bundle is there once and holds the new binary.
        assert_eq!(names.len(), 1, "{names:?}");
        let insensitive = dir.path().join("SIGIL.APP").exists();
        if insensitive {
            assert_eq!(names[0], "Sigil.app", "{names:?}");
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join(&names[0]).join("Contents/MacOS/sigil"))
                .unwrap(),
            "new"
        );
    }

    #[test]
    fn a_zip_without_the_binary_leaves_the_bundle_alone() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Sigil.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/MacOS/sigil"), "old").unwrap();
        std::fs::write(app.join("Contents/Info.plist"), "<plist/>").unwrap();
        let hollow = dir.path().join("hollow").join("Sigil.app");
        std::fs::create_dir_all(hollow.join("Contents")).unwrap();
        std::fs::write(hollow.join("Contents/Info.plist"), "<plist/>").unwrap();
        assert!(matches!(
            install_mac_bundle(&hollow, &app),
            Err(Error::Install(_))
        ));
        assert_eq!(
            std::fs::read_to_string(app.join("Contents/MacOS/sigil")).unwrap(),
            "old"
        );
        assert!(!previous_bundle(&app).exists());

        // And an archive that is not one at all.
        let not = dir.path().join("sigil-v1.2.3-x86_64-linux-gnu.tar.gz");
        std::fs::write(&not, "nope").unwrap();
        assert!(matches!(
            unpack(&not, &dir.path().join("out")),
            Err(Error::Install(_))
        ));
    }
}
