//! Central redaction and private-file helpers.

use std::{fs, io, path::Path};

use serde_json::Value;

const SECRET_KEYS: &[&str] = &[
    "authorization",
    "api_key",
    "api-key",
    "access_token",
    "refresh_token",
    "client_secret",
    "password",
];

pub fn redact_text(input: &str, known_secrets: &[&str]) -> String {
    let mut output = input.to_owned();
    for secret in known_secrets.iter().filter(|secret| secret.len() >= 4) {
        output = output.replace(secret, "<redacted>");
    }
    output = redact_bearer_tokens(&output);
    output = redact_url_credentials(&output);
    output
}

pub fn redact_json(value: &Value, known_secrets: &[&str]) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    if is_secret_key(key) {
                        (key.clone(), Value::String("<redacted>".into()))
                    } else {
                        (key.clone(), redact_json(value, known_secrets))
                    }
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| redact_json(value, known_secrets))
                .collect(),
        ),
        Value::String(value) => Value::String(redact_text(value, known_secrets)),
        other => other.clone(),
    }
}

pub fn is_secret_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    SECRET_KEYS
        .iter()
        .any(|candidate| normalized == *candidate || normalized.ends_with(candidate))
}

pub fn child_environment() -> Vec<(String, String)> {
    [
        ("PATH", "/usr/bin:/bin"),
        ("LANG", "C.UTF-8"),
        ("LC_ALL", "C.UTF-8"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}

pub fn restrict_private_file(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if path.exists() {
            let mut permissions = fs::metadata(path)?.permissions();
            if permissions.mode() & 0o777 != 0o600 {
                permissions.set_mode(0o600);
                fs::set_permissions(path, permissions)?;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn redact_bearer_tokens(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    loop {
        let lower = remaining.to_ascii_lowercase();
        let Some(index) = lower.find("bearer ") else {
            output.push_str(remaining);
            break;
        };
        output.push_str(&remaining[..index]);
        output.push_str(&remaining[index..index + "bearer ".len()]);
        output.push_str("<redacted>");
        let token = &remaining[index + "bearer ".len()..];
        let end = token
            .find(|character: char| {
                character.is_whitespace() || matches!(character, ',' | '}' | ']')
            })
            .unwrap_or(token.len());
        remaining = &token[end..];
    }
    output
}

fn redact_url_credentials(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    loop {
        let Some(scheme) = remaining.find("://") else {
            output.push_str(remaining);
            break;
        };
        let authority_start = scheme + 3;
        let authority_end = remaining[authority_start..]
            .find(['/', '?', '#', ' '])
            .map_or(remaining.len(), |index| authority_start + index);
        let authority = &remaining[authority_start..authority_end];
        let Some(at) = authority.rfind('@') else {
            output.push_str(&remaining[..authority_end]);
            remaining = &remaining[authority_end..];
            continue;
        };
        output.push_str(&remaining[..authority_start]);
        output.push_str("<redacted>@");
        output.push_str(&authority[at + 1..]);
        remaining = &remaining[authority_end..];
    }
    output
}
