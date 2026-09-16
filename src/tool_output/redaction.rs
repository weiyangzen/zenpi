//! Bounded, byte-position-preserving credential masking before tail/range cuts.
//! A prefix that may become a credential is withheld until resolved. Raw output
//! is stored separately. Masked bytes are ASCII '*' so offsets remain raw offsets.
use std::{collections::VecDeque, fmt};
const LOOKAHEAD: usize = 16 * 1024;
const CHUNK: usize = 4096;
const ASSIGNMENTS: &[&[u8]] = &[
    b"proxy-authorization=",
    b"cookie=",
    b"set-cookie=",
    b"api_key=",
    b"api-key=",
    b"x-api-key=",
    b"access_token=",
    b"refresh_token=",
    b"password=",
    b"client_secret=",
    b"private_key=",
    b"secret_key=",
    b"secret=",
];
const HEADERS: &[&[u8]] = &[
    b"authorization:",
    b"proxy-authorization:",
    b"cookie:",
    b"set-cookie:",
    b"api-key:",
    b"x-api-key:",
    b"access-token:",
    b"refresh-token:",
    b"client-secret:",
];
#[derive(Debug, Default)]
enum Mode {
    #[default]
    Normal,
    ValueStart,
    Token,
    Quoted {
        quote: u8,
        escaped: bool,
    },
    Header,
    Url {
        authority: Vec<u8>,
        overflow: bool,
    },
}
pub(super) struct OutputRedactor {
    secrets: Vec<Vec<u8>>,
    pending: Vec<u8>,
    known_pending: Vec<u8>,
    known_mask: VecDeque<bool>,
    syntax_output: VecDeque<u8>,
    mask_remaining: usize,
    secret_prefixes: [Vec<usize>; 256],
    mode: Mode,
    emitted: u64,
}
impl fmt::Debug for OutputRedactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never expose held prefixes, URL userinfo or registered values in Debug.
        f.debug_struct("OutputRedactor")
            .field("held_bytes", &self.pending.len())
            .field("emitted", &self.emitted)
            .finish_non_exhaustive()
    }
}
impl Default for OutputRedactor {
    fn default() -> Self {
        let secrets: Vec<Vec<u8>> = crate::security::registered_secret_values()
            .into_iter()
            .filter(|s| s.len() >= 4)
            .map(String::into_bytes)
            .collect();
        let mut secret_prefixes: [Vec<usize>; 256] = std::array::from_fn(|_| vec![]);
        for (index, secret) in secrets.iter().enumerate() {
            secret_prefixes[secret[0] as usize].push(index);
        }
        Self {
            secrets,
            secret_prefixes,
            pending: vec![],
            known_pending: vec![],
            known_mask: VecDeque::new(),
            syntax_output: VecDeque::new(),
            mask_remaining: 0,
            mode: Mode::Normal,
            emitted: 0,
        }
    }
}
fn prefix(bytes: &[u8], pattern: &[u8]) -> bool {
    bytes.len() < pattern.len() && bytes.eq_ignore_ascii_case(&pattern[..bytes.len()])
}
fn starts(bytes: &[u8], pattern: &[u8]) -> bool {
    bytes.len() >= pattern.len() && bytes[..pattern.len()].eq_ignore_ascii_case(pattern)
}
fn token_end(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b',' | b'}' | b']')
}
fn authority_end(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'/' | b'?' | b'#')
}
impl OutputRedactor {
    pub(super) fn emitted(&self) -> u64 {
        self.emitted
    }
    pub(super) fn feed(&mut self, input: &[u8]) -> Vec<u8> {
        let mut output = Vec::new();
        for chunk in input.chunks(CHUNK) {
            self.known_pending.extend_from_slice(chunk);
            self.process_known(false, false);
            self.pending.extend_from_slice(chunk);
            let mut syntax = Vec::new();
            self.process(false, &mut syntax);
            self.syntax_output.extend(syntax);
            self.drain_ready(&mut output);
        }
        self.emitted += output.len() as u64;
        output
    }
    pub(super) fn finish(&mut self, complete: bool) -> Vec<u8> {
        self.process_known(true, !complete);
        let mut syntax = Vec::new();
        self.process(true, &mut syntax);
        if let Mode::Url {
            authority,
            overflow,
        } = &mut self.mode
        {
            Self::mask_authority(authority, *overflow || !complete);
            syntax.append(authority);
        }
        self.mode = Mode::Normal;
        self.syntax_output.extend(syntax);
        let mut output = Vec::new();
        self.drain_ready(&mut output);
        debug_assert!(self.known_mask.is_empty() && self.syntax_output.is_empty());
        self.emitted += output.len() as u64;
        output
    }
    fn drain_ready(&mut self, output: &mut Vec<u8>) {
        while !self.known_mask.is_empty() && !self.syntax_output.is_empty() {
            let byte = self.syntax_output.pop_front().unwrap();
            output.push(if self.known_mask.pop_front().unwrap() {
                b'*'
            } else {
                byte
            });
        }
    }
    fn process_known(&mut self, eof: bool, interrupted: bool) {
        let mut cursor = 0;
        while cursor < self.known_pending.len() {
            let bytes = &self.known_pending[cursor..];
            let mut matched = 0;
            let mut partial = false;
            for index in &self.secret_prefixes[bytes[0] as usize] {
                let secret = &self.secrets[*index];
                if bytes.starts_with(secret) {
                    matched = matched.max(secret.len());
                }
                if bytes.len() < secret.len() && secret.starts_with(bytes) {
                    partial = true;
                }
            }
            if partial && !eof {
                break;
            }
            if partial && interrupted {
                matched = bytes.len();
            }
            self.mask_remaining = self.mask_remaining.max(matched);
            self.known_mask.push_back(self.mask_remaining > 0);
            self.mask_remaining = self.mask_remaining.saturating_sub(1);
            cursor += 1;
        }
        self.known_pending.drain(..cursor);
        debug_assert!(self.known_pending.len() <= LOOKAHEAD);
    }
    fn mask_authority(bytes: &mut [u8], force: bool) {
        if force {
            bytes.fill(b'*');
        } else if let Some(at) = bytes.iter().rposition(|b| *b == b'@') {
            bytes[..at].fill(b'*');
        }
    }
    fn process(&mut self, eof: bool, output: &mut Vec<u8>) {
        let mut cursor = 0;
        while cursor < self.pending.len() {
            let bytes = &self.pending[cursor..];
            match &mut self.mode {
                Mode::Normal => {
                    if !eof
                        && (ASSIGNMENTS.iter().chain(HEADERS).any(|p| prefix(bytes, p))
                            || prefix(bytes, b"bearer ")
                            || prefix(bytes, b"://"))
                    {
                        break;
                    }
                    let matched = ASSIGNMENTS
                        .iter()
                        .find(|p| starts(bytes, p))
                        .map(|p| (p.len(), Mode::ValueStart))
                        .or_else(|| {
                            HEADERS
                                .iter()
                                .find(|p| starts(bytes, p))
                                .map(|p| (p.len(), Mode::Header))
                        })
                        .or_else(|| starts(bytes, b"bearer ").then_some((7, Mode::ValueStart)))
                        .or_else(|| {
                            starts(bytes, b"://").then_some((
                                3,
                                Mode::Url {
                                    authority: vec![],
                                    overflow: false,
                                },
                            ))
                        });
                    if let Some((count, mode)) = matched {
                        output.extend_from_slice(&bytes[..count]);
                        cursor += count;
                        self.mode = mode;
                    } else {
                        output.push(bytes[0]);
                        cursor += 1;
                    }
                }
                Mode::ValueStart => {
                    let byte = bytes[0];
                    if byte.is_ascii_whitespace() {
                        output.push(b'*');
                        cursor += 1;
                    } else if matches!(byte, b'\'' | b'"') {
                        output.push(b'*');
                        cursor += 1;
                        self.mode = Mode::Quoted {
                            quote: byte,
                            escaped: false,
                        };
                    } else {
                        self.mode = Mode::Token;
                    }
                }
                Mode::Token => {
                    if token_end(bytes[0]) {
                        self.mode = Mode::Normal;
                    } else {
                        output.push(b'*');
                        cursor += 1;
                    }
                }
                Mode::Quoted { quote, escaped } => {
                    let byte = bytes[0];
                    output.push(b'*');
                    cursor += 1;
                    if *escaped {
                        *escaped = false;
                    } else if byte == b'\\' {
                        *escaped = true;
                    } else if byte == *quote {
                        self.mode = Mode::Normal;
                    }
                }
                Mode::Header => {
                    if matches!(bytes[0], b'\r' | b'\n') {
                        self.mode = Mode::Normal;
                    } else {
                        output.push(b'*');
                        cursor += 1;
                    }
                }
                Mode::Url {
                    authority,
                    overflow,
                } => {
                    if authority_end(bytes[0]) {
                        Self::mask_authority(authority, *overflow);
                        output.append(authority);
                        self.mode = Mode::Normal;
                    } else {
                        authority.push(bytes[0]);
                        cursor += 1;
                        if authority.len() >= LOOKAHEAD {
                            // An oversized authority cannot be retained safely;
                            // mask all its bytes until its delimiter, including
                            // any later userinfo separator or hostname.
                            *overflow = true;
                            authority.fill(b'*');
                            output.append(authority);
                        }
                    }
                }
            }
        }
        self.pending.drain(..cursor);
        debug_assert!(self.pending.len() <= LOOKAHEAD);
    }
}
