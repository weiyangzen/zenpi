use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;
use zenpi::folder_source::{FolderSource, RemoteSpec, list_local};

#[test]
fn parses_ssh_config_alias_and_explicit_forms() {
    // ssh:// with explicit user and port.
    let spec = RemoteSpec::parse("ssh://alice@example.com:2222/srv/work").unwrap();
    assert_eq!(spec.user.as_deref(), Some("alice"));
    assert_eq!(spec.host, "example.com");
    assert_eq!(spec.port, Some(2222));
    assert_eq!(spec.path, "/srv/work");

    // scp-like explicit host:path.
    let spec = RemoteSpec::parse("bob@10.0.0.5:/home/bob/proj").unwrap();
    assert_eq!(spec.user.as_deref(), Some("bob"));
    assert_eq!(spec.host, "10.0.0.5");
    assert_eq!(spec.port, None);
    assert_eq!(spec.path, "/home/bob/proj");

    // `~/.ssh/config` alias with no user/port.
    let spec = RemoteSpec::parse("gpu-box:/data/proj").unwrap();
    assert_eq!(spec.host, "gpu-box");
    assert_eq!(spec.user, None);
    assert_eq!(spec.path, "/data/proj");

    // Optional explicit identity key.
    let spec = RemoteSpec::parse("ssh://h/p?identity=/keys/id_ed25519").unwrap();
    assert_eq!(spec.identity.unwrap().display().to_string(), "/keys/id_ed25519");

    assert!(RemoteSpec::parse("").is_err());
    assert!(RemoteSpec::parse("host-only").is_err());
}

#[test]
fn probe_argv_is_read_only_and_bounded() {
    let spec = RemoteSpec::parse("ssh://alice@example.com:2222/srv?identity=/k/id").unwrap();
    let argv = spec.probe_argv("/srv/work");
    assert_eq!(argv[0], "ssh");
    assert!(argv.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
    assert!(argv.windows(2).any(|w| w == ["-p", "2222"]));
    assert!(argv.windows(2).any(|w| w == ["-i", "/k/id"]));
    assert!(argv.contains(&"alice@example.com".to_owned()));
    let command = argv.last().unwrap();
    assert!(command.starts_with("ls -1Ap -- "), "{command}");
    assert!(!command.contains("rm ") && !command.contains(">"), "{command}");
}

#[test]
fn resolve_distinguishes_local_and_remote() {
    let dir = tempdir().unwrap();
    let local = FolderSource::resolve(dir.path().to_str().unwrap()).unwrap();
    assert!(!local.is_remote());
    assert!(matches!(local, FolderSource::Local { .. }));

    let remote = FolderSource::resolve("gpu-box:/data/proj").unwrap();
    assert!(remote.is_remote());
    match remote {
        FolderSource::Remote { spec } => assert_eq!(spec.host, "gpu-box"),
        _ => unreachable!(),
    }
    assert!(FolderSource::resolve("/definitely/not/here").is_err());
}

#[test]
fn remote_probe_uses_stub_and_bounds_output() {
    let dir = tempdir().unwrap();
    let stub = dir.path().join("ssh-stub.sh");
    fs::write(&stub, "#!/bin/sh\nprintf 'alpha/\\nbeta.txt\\n./\\n../\\ngamma/\\n'\n").unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o700)).unwrap();
    // SAFETY: single-threaded test process during setup.
    unsafe { std::env::set_var("ZENPI_SSH_BIN", &stub) };
    let spec = RemoteSpec::parse("gpu-box:/data/proj").unwrap();
    let entries = spec.probe("/data/proj").unwrap();
    assert_eq!(entries, vec!["alpha/", "beta.txt", "gamma/"]);
    unsafe { std::env::remove_var("ZENPI_SSH_BIN") };
}

#[test]
fn local_listing_is_sorted_and_bounded() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("b")).unwrap();
    fs::write(dir.path().join("a"), "x").unwrap();
    let entries = list_local(dir.path()).unwrap();
    assert_eq!(entries, vec!["a", "b/"]);
}
