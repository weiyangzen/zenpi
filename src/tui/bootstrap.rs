//! First-run authentication bootstrap for the interactive host.
//!
//! When a user has no usable connection, starting the agent first and failing
//! afterwards tells them nothing actionable.  The bootstrap offers login,
//! connection selection, and exit — and nothing else: no prompt, no shell, no
//! project restore, and no resource or worker runner.
//!
//! A configuration that is *broken* (bad TOML, an unsafe state directory, an
//! unreadable session) is never reported as "not logged in".  Only a credential
//! that is explicitly missing is a reason to show this, so `auth_gate` lets
//! every other error propagate to the ordinary path.

use std::io::{BufRead, Write};

use crate::config::{ConfigPaths, ConfigSummary};
use crate::error::ZenpiError;

/// Whether the interactive host can start, and if not, why.
pub(crate) enum AuthGate {
    Ready,
    /// No usable credential; the reason is safe to show the user.
    NeedsAuth(String),
}

/// Classify the current configuration for the interactive host.
///
/// The distinction that matters: a credential that is *missing* is a first-run
/// state this module handles, while a configuration that is *unreadable* is a
/// failure the user has to see as itself.
pub(crate) fn auth_gate(
    paths: &ConfigPaths,
    profile: Option<&str>,
) -> Result<AuthGate, ZenpiError> {
    // A malformed config, an unsafe directory, or an unreadable credential
    // store returns Err here, and that is deliberate: those must not be
    // presented as "you are not logged in".
    let status = crate::config::status_for_profile(paths, profile)?;
    if status.is_ready() {
        return Ok(AuthGate::Ready);
    }
    let profiles = crate::config::list_profiles(paths)?;
    let reason = match status.auth_binding_state.as_deref() {
        // These three have exactly one remedy a first-run flow can offer.
        Some("unconfigured") => "the selected profile names a credential that is not stored",
        Some("login_required") => "the stored credential needs a new login",
        Some("revoked") => "the stored credential was revoked",
        Some(_) => {
            // expired / refreshing / uncertain / anonymous_pending_route all
            // have a working path of their own (refresh at request time, or no
            // credential at all), so they are not first-run states.
            return Ok(AuthGate::Ready);
        }
        None if profiles.is_empty() && !status.api_key_present => {
            "no provider connection is configured"
        }
        None => return Ok(AuthGate::Ready),
    };
    Ok(AuthGate::NeedsAuth(reason.to_owned()))
}

pub(crate) enum BootstrapOutcome {
    /// A usable connection now exists; the caller may start the host.
    Ready,
    /// The user chose to exit without connecting.
    Exited,
}

/// Offer login, connection selection, or exit until one of them happens.
///
/// Everything here is a trusted local interaction: it runs before any agent,
/// tool, or resource task exists, so nothing it does can be reached from a
/// prompt.  The authorization URL goes to stderr, as it does for the CLI.
pub(crate) fn run_auth_bootstrap(
    paths: &ConfigPaths,
    profile: Option<&str>,
    reason: &str,
) -> Result<BootstrapOutcome, ZenpiError> {
    eprintln!("zenpi: {reason}");
    loop {
        let profiles = crate::config::list_profiles(paths)?;
        let credentials = crate::config::auth_list(paths).unwrap_or_default();
        eprintln!();
        if profiles.is_empty() {
            eprintln!("no connections are configured yet");
        } else {
            eprintln!("connections:");
            for (index, entry) in profiles.iter().enumerate() {
                let state = crate::config::profile_credential(paths, &entry.name)
                    .ok()
                    .flatten()
                    .and_then(|id| {
                        credentials
                            .iter()
                            .find(|credential| credential.credential_id == id)
                            .map(|credential| credential.state)
                    })
                    .unwrap_or("no credential");
                eprintln!(
                    "  {}) {}{} provider={} model={} [{}]",
                    index + 1,
                    if entry.active { "* " } else { "  " },
                    entry.name,
                    entry.provider.as_deref().unwrap_or("-"),
                    entry.model.as_deref().unwrap_or("-"),
                    state,
                );
            }
        }
        eprintln!();
        eprintln!("  l) log in with a browser");
        eprintln!("  d) log in with a device code");
        eprintln!("  q) exit");
        eprint!("choice: ");
        let _ = std::io::stderr().flush();

        let Some(choice) = read_line()? else {
            return Ok(BootstrapOutcome::Exited);
        };
        match choice.trim() {
            "" => continue,
            "q" | "quit" | "exit" => return Ok(BootstrapOutcome::Exited),
            "l" | "d" => {
                let device = choice.trim() == "d";
                attempt_login(device)?;
                return Ok(BootstrapOutcome::Ready);
            }
            other => {
                let Some(index) = other.parse::<usize>().ok().filter(|index| *index > 0) else {
                    eprintln!("zenpi: enter a number from the list, l, d, or q");
                    continue;
                };
                let Some(entry) = profiles.get(index - 1) else {
                    eprintln!("zenpi: no connection numbered {index}");
                    continue;
                };
                // Selecting a connection is only useful if it is usable; the
                // loop re-reads state rather than assuming the choice worked.
                crate::config::use_profile(paths, &entry.name)?;
                match auth_gate(paths, profile)? {
                    AuthGate::Ready => return Ok(BootstrapOutcome::Ready),
                    AuthGate::NeedsAuth(reason) => {
                        eprintln!("zenpi: {}", reason);
                    }
                }
            }
        }
    }
}

/// Run one interactive login and bind the resulting credential.
///
/// This reuses the same login library the CLI drives; the bootstrap owns no
/// second authentication state machine.
fn attempt_login(device: bool) -> Result<(), ZenpiError> {
    let paths = ConfigPaths::discover()?;
    crate::core::run_cli_codex_login(&paths, device, false, Some("codex"), None)?;
    let summary: ConfigSummary = crate::config::doctor_for_profile(&paths, Some("codex"))?;
    if summary.status.is_ready() {
        Ok(())
    } else {
        Err(ZenpiError::Message(
            "the login finished but the connection is still not usable; run `zenpi config doctor --profile codex`"
                .into(),
        ))
    }
}

fn read_line() -> Result<Option<String>, ZenpiError> {
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(line.trim_end_matches(['\n', '\r']).to_owned())),
        Err(_) => Err(ZenpiError::arguments(
            "could not read the bootstrap choice from standard input",
        )),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    /// A private state root, the way zenpi creates it.
    fn paths() -> (tempfile::TempDir, ConfigPaths) {
        let directory = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::for_home(directory.path());
        fs::create_dir_all(&paths.root).unwrap();
        fs::set_permissions(&paths.root, fs::Permissions::from_mode(0o700)).unwrap();
        (directory, paths)
    }

    fn write_config(paths: &ConfigPaths, body: &str) {
        fs::write(&paths.config, body).unwrap();
    }

    #[test]
    fn an_empty_installation_is_a_first_run_not_a_failure() {
        let (_dir, paths) = paths();
        match auth_gate(&paths, None).unwrap() {
            AuthGate::NeedsAuth(reason) => assert!(reason.contains("no provider connection")),
            AuthGate::Ready => panic!("an empty installation needs the bootstrap"),
        }
    }

    #[test]
    fn a_broken_config_is_reported_as_itself_not_as_not_logged_in() {
        let (_dir, paths) = paths();
        write_config(&paths, "this is not toml [[[\n");
        // The gate must not swallow this into "you are not logged in": the
        // caller has to see a parse failure as a parse failure.
        assert!(auth_gate(&paths, None).is_err());
    }

    #[test]
    fn a_profile_naming_a_missing_credential_asks_for_a_login() {
        let (_dir, paths) = paths();
        write_config(
            &paths,
            "default_profile = 'codex'\n[profiles.codex]\nprovider = 'openai-codex'\nauth_method = 'oauth'\nauth_ref = 'cred_absent'\n",
        );
        match auth_gate(&paths, None).unwrap() {
            AuthGate::NeedsAuth(reason) => assert!(reason.contains("not stored"), "{reason}"),
            AuthGate::Ready => panic!("a missing credential is a first-run state"),
        }
    }

    #[test]
    fn a_usable_connection_never_shows_the_bootstrap() {
        let (_dir, paths) = paths();
        // A legacy flat configuration plus its key is a usable connection too,
        // and must not be turned into a login prompt.
        write_config(
            &paths,
            "provider='deepseek'\nmodel='deepseek-flash'\nbase_url='https://api.deepseek.com'\nrequires_openai_auth=true\n",
        );
        fs::write(
            &paths.auth,
            "{\"OPENAI_API_KEY\":\"synthetic-legacy-key\"}\n",
        )
        .unwrap();
        fs::set_permissions(&paths.auth, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(auth_gate(&paths, None).unwrap(), AuthGate::Ready));
    }
}
