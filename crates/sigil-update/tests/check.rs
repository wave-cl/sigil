//! The whole check and the whole update, against a release served from
//! 127.0.0.1.
//!
//! The listener is a few lines of HTTP/1.1; everything on the other side of
//! it -- the request, the parsing, the signature, the digest, the unpack,
//! the swap -- is the code the app runs, with only the base URL and the
//! key handed in.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::Digest;
use sigil_update::checker::{UpdateState, Updater, check};
use sigil_update::install::Install;
use sigil_update::manifest::{self, Asset, Manifest};
use sigil_update::release::Client;
use sigil_update::version::Version;

const SEED: [u8; 32] = [42u8; 32];

type Routes = HashMap<String, (u16, Vec<u8>)>;

/// A canned server: path -> (status, body). Anything else is 404.
struct Server {
    base: String,
}

impl Server {
    fn serve(routes: Routes) -> Server {
        Server::serve_on(TcpListener::bind("127.0.0.1:0").unwrap(), routes)
    }

    fn serve_on(listener: TcpListener, routes: Routes) -> Server {
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes = Arc::new(routes);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let routes = routes.clone();
                std::thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]);
                    let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
                    let (status, body) =
                        routes.get(&path).cloned().unwrap_or((404, b"no".to_vec()));
                    let reason = match status {
                        200 => "OK",
                        403 => "Forbidden",
                        _ => "Not Found",
                    };
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(&body);
                    let _ = stream.flush();
                });
            }
        });
        Server { base }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

/// A release of `tag` with the asset this machine would take, served
/// whole: the API answer, the manifest and its signature, the file.
struct Release {
    routes: Routes,
    asset: String,
    body: Vec<u8>,
}

fn install() -> Install {
    Install::LinuxBinary {
        exe: "/home/c/bin/sigil".into(),
    }
}

fn sha(body: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(body))
}

fn a_release(tag: &str, base: &str, body: &[u8], signed: bool) -> Release {
    let asset = sigil_update::release::asset_name(tag, &install(), std::env::consts::ARCH).unwrap();
    let (json_name, sig_name) = manifest::file_names(tag);
    let mut m = Manifest {
        tag: tag.to_owned(),
        version: Version::parse(tag).unwrap().to_string(),
        assets: Default::default(),
    };
    m.assets.insert(
        asset.clone(),
        Asset {
            bytes: body.len() as u64,
            sha256: sha(body),
        },
    );
    let json = m.to_canonical_bytes();
    let sig = manifest::signature_file(&manifest::sign(&SEED, &json));

    let entry = |name: &str, len: usize| {
        format!(
            r#"{{"name":"{name}","browser_download_url":"{base}/download/{name}","size":{len}}}"#
        )
    };
    let mut assets = vec![entry(&asset, body.len())];
    if signed {
        assets.push(entry(&json_name, json.len()));
        assets.push(entry(&sig_name, sig.len()));
    }
    let latest = format!(
        r#"{{"tag_name":"{tag}","html_url":"https://example.invalid/{tag}","assets":[{}]}}"#,
        assets.join(",")
    );
    let mut routes = HashMap::new();
    routes.insert("/releases/latest".to_owned(), (200, latest.into_bytes()));
    routes.insert(format!("/download/{asset}"), (200, body.to_vec()));
    routes.insert(format!("/download/{json_name}"), (200, json));
    routes.insert(format!("/download/{sig_name}"), (200, sig.into_bytes()));
    Release {
        routes,
        asset,
        body: body.to_vec(),
    }
}

/// The API answer has to carry the download URLs, and the port is not
/// known until the socket is bound: so a port is taken and released, the
/// release is written against it, and the real listener binds it.
fn served(tag: &str, body: &[u8], signed: bool) -> (Server, Release) {
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let release = a_release(tag, &format!("http://127.0.0.1:{port}"), body, signed);
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let server = Server::serve_on(listener, release.routes.clone());
    (server, release)
}

fn key() -> ed25519_dalek::VerifyingKey {
    manifest::verifying_key(&manifest::public_of(&SEED))
}

fn current() -> Version {
    Version::parse("0.1.5").unwrap()
}

#[test]
fn a_newer_signed_release_with_a_build_for_this_install_is_available() {
    let (server, release) = served("v0.1.6", b"the new sigil", true);
    let client = Client::new(server.base.clone());
    let (state, found) = check(&client, &key(), &install(), current());
    assert_eq!(
        state,
        UpdateState::Available {
            version: Version::parse("0.1.6").unwrap(),
            notes_url: "https://example.invalid/v0.1.6".into(),
            asset: release.asset.clone(),
        }
    );
    let found = found.unwrap();
    assert_eq!(found.manifest.assets[&release.asset].bytes, 13);
}

#[test]
fn the_same_or_an_older_release_is_up_to_date() {
    for tag in ["v0.1.5", "v0.1.4"] {
        let (server, _) = served(tag, b"x", true);
        let client = Client::new(server.base.clone());
        let (state, found) = check(&client, &key(), &install(), current());
        assert!(
            matches!(state, UpdateState::UpToDate { .. }),
            "{tag}: {state:?}"
        );
        assert!(found.is_none());
    }
}

#[test]
fn a_newer_release_without_a_manifest_is_unsigned_not_a_fault() {
    let (server, _) = served("v0.1.6", b"x", false);
    let client = Client::new(server.base.clone());
    let (state, found) = check(&client, &key(), &install(), current());
    assert_eq!(
        state,
        UpdateState::Unsigned {
            version: Version::parse("0.1.6").unwrap(),
            notes_url: "https://example.invalid/v0.1.6".into(),
        }
    );
    assert!(found.is_none());
}

#[test]
fn a_manifest_signed_by_somebody_else_fails_and_says_so() {
    let (server, _) = served("v0.1.6", b"x", true);
    let client = Client::new(server.base.clone());
    let other = manifest::verifying_key(&manifest::public_of(&[1u8; 32]));
    let (state, found) = check(&client, &other, &install(), current());
    match state {
        UpdateState::Failed { why } => assert!(why.contains("signature"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(found.is_none());
}

#[test]
fn a_release_with_no_build_for_this_install_fails_and_names_it() {
    let (server, _) = served("v0.1.6", b"x", true);
    let client = Client::new(server.base.clone());
    let elsewhere = Install::MacBundle {
        app: "/Applications/Sigil.app".into(),
    };
    let (state, _) = check(&client, &key(), &elsewhere, current());
    match state {
        UpdateState::Failed { why } => assert!(why.contains("no build for"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn nobody_listening_is_unreachable_and_so_is_a_refusal() {
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", probe.local_addr().unwrap());
    drop(probe);
    let started = Instant::now();
    let (state, _) = check(&Client::new(base), &key(), &install(), current());
    assert!(
        matches!(state, UpdateState::Unreachable { .. }),
        "{state:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(15));

    let mut routes = HashMap::new();
    routes.insert(
        "/releases/latest".to_owned(),
        (403, b"rate limited".to_vec()),
    );
    let server = Server::serve(routes);
    let (state, _) = check(
        &Client::new(server.base.clone()),
        &key(),
        &install(),
        current(),
    );
    match state {
        UpdateState::Unreachable { why } => assert!(why.contains("403"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_download_is_kept_only_when_it_is_the_file_the_manifest_names() {
    let (server, release) = served("v0.1.6", b"the new sigil", true);
    let client = Client::new(server.base.clone());
    let dir = tempfile::tempdir().unwrap();
    let url = server.url(&format!("/download/{}", release.asset));
    let part = dir.path().join(format!("{}.part", release.asset));
    let fetch = |expect: &Asset, seen: &dyn Fn(u64, u64)| {
        sigil_update::download::download(&client, &url, &release.asset, expect, dir.path(), seen)
    };

    let expect = Asset {
        bytes: 13,
        sha256: sha(&release.body),
    };
    let seen = Mutex::new(Vec::new());
    let file = fetch(&expect, &|done, total| {
        seen.lock().unwrap().push((done, total))
    })
    .unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"the new sigil");
    assert!(!part.exists());
    assert_eq!(seen.lock().unwrap().last(), Some(&(13, 13)));

    // The digest of something else: the digest says no, nothing stays.
    let wrong = Asset {
        bytes: 13,
        sha256: sha(b"the old sigil"),
    };
    let err = fetch(&wrong, &|_, _| {}).unwrap_err();
    assert!(matches!(err, sigil_update::Error::ShaMismatch(_)), "{err}");
    assert!(!file.exists() && !part.exists());

    // Longer than the manifest says: stopped before the end.
    let short = Asset {
        bytes: 5,
        sha256: sha(&release.body),
    };
    let err = fetch(&short, &|_, _| {}).unwrap_err();
    assert!(matches!(err, sigil_update::Error::TooLarge(_)), "{err}");
    assert!(!file.exists() && !part.exists());
}

/// The thread, end to end: it checks on its own, wakes the window, and on
/// Update fetches the tarball and puts the binary in place.
#[cfg(unix)]
#[test]
fn the_updater_checks_by_itself_and_installs_on_request() {
    // A tarball with `sigil` at its root, as the release makes it.
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("bin").join("sigil");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, "old").unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("sigil"), "new").unwrap();
    let tarball = dir.path().join("sigil.tar.gz");
    assert!(
        std::process::Command::new("tar")
            .arg("-czf")
            .arg(&tarball)
            .arg("-C")
            .arg(&src)
            .arg("sigil")
            .status()
            .unwrap()
            .success()
    );
    let body = std::fs::read(&tarball).unwrap();
    let (server, release) = served("v9.9.9", &body, true);
    let install = Install::LinuxBinary { exe: exe.clone() };

    let wakes = Arc::new(Mutex::new(0u32));
    let counted = wakes.clone();
    let updater = Updater::start(
        Client::new(server.base.clone()),
        key(),
        install,
        Duration::from_millis(0),
        Duration::from_secs(3600),
        move || *counted.lock().unwrap() += 1,
    );
    let until = |want: &dyn Fn(&UpdateState) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let state = updater.state();
            if want(&state) {
                return state;
            }
            assert!(Instant::now() < deadline, "still {state:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let state = until(&|s| matches!(s, UpdateState::Available { .. }));
    assert!(
        *wakes.lock().unwrap() >= 2,
        "Checking, then Available, each woke the window"
    );
    assert!(matches!(&state, UpdateState::Available { asset, .. } if *asset == release.asset));
    assert_eq!(
        std::fs::read_to_string(&exe).unwrap(),
        "old",
        "nothing installed unasked"
    );

    updater.update();
    let state = until(&|s| matches!(s, UpdateState::Ready { .. } | UpdateState::Failed { .. }));
    assert_eq!(
        state,
        UpdateState::Ready {
            version: Version::parse("9.9.9").unwrap()
        }
    );
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
    // Ready is final until a restart: a check now would only lie.
    updater.check_now();
    std::thread::sleep(Duration::from_millis(300));
    assert!(matches!(updater.state(), UpdateState::Ready { .. }));
}
