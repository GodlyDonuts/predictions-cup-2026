// Hand-rolled equivalents of the exact secret-scan regexes used by the
// prepare-*-publication-*.mjs scripts. Each function returns true when the
// content WOULD match the JS regex (i.e. should be rejected).
fn run_of(content: &str, start: usize, pred: impl Fn(u8) -> bool) -> usize {
    let b = content.as_bytes();
    let mut n = 0;
    while start + n < b.len() && pred(b[start + n]) { n += 1; }
    n
}
fn find_at(content: &str, needle: &str, min_tail: usize, tail: impl Fn(u8) -> bool) -> bool {
    let mut from = 0;
    while let Some(i) = content[from..].find(needle) {
        let at = from + i;
        if run_of(content, at + needle.len(), &tail) >= min_tail { return true; }
        from = at + 1;
    }
    false
}
fn contains_ignore_case(content: &str, needle: &str) -> bool {
    content.to_lowercase().contains(&needle.to_lowercase())
}
fn begins_private_key(content: &str) -> bool {
    // /-----BEGIN [A-Z ]*PRIVATE KEY-----/
    let needle = "-----BEGIN ";
    let mut from = 0;
    while let Some(i) = content[from..].find(needle) {
        let at = from + i + needle.len();
        let n = run_of(content, at, |b| b.is_ascii_uppercase() || b == b' ');
        if content[at + n..].starts_with("PRIVATE KEY-----") { return true; }
        from = at;
    }
    false
}
fn begins_typed_private_key(content: &str) -> bool {
    // /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY/
    for prefix in ["-----BEGIN PRIVATE KEY", "-----BEGIN RSA PRIVATE KEY", "-----BEGIN EC PRIVATE KEY", "-----BEGIN OPENSSH PRIVATE KEY"] {
        if content.contains(prefix) { return true; }
    }
    false
}
fn auth_bearer(content: &str) -> bool {
    // /Authorization:\s*Bearer [a-zA-Z0-9]/
    let needle = "Authorization:";
    let mut from = 0;
    while let Some(i) = content[from..].find(needle) {
        let mut at = from + i + needle.len();
        while at < content.len() && content.as_bytes()[at].is_ascii_whitespace() { at += 1; }
        if content[at..].starts_with("Bearer ")
            && content.as_bytes().get(at + 7).is_some_and(|b| b.is_ascii_alphanumeric())
        {
            return true;
        }
        from = at;
    }
    false
}
fn token_url_param(content: &str) -> bool {
    // /[?&](?:token|access_token)=/i
    let lower = content.to_lowercase();
    for needle in ["?token=", "&token=", "?access_token=", "&access_token="] {
        if lower.contains(needle) { return true; }
    }
    false
}
fn https_token(content: &str) -> bool {
    // /https:\/\/[^\s"']+token=[A-Za-z0-9%]+/i
    let lower = content.to_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("https://") {
        let mut at = from + i + 8;
        while at < lower.len() && ![b' ', b'\t', b'\n', b'\r', b'"', b'\''].contains(&lower.as_bytes()[at]) {
            if lower[at..].starts_with("token=") {
                let n = run_of(&lower, at + 6, |b| b.is_ascii_alphanumeric() || b == b'%');
                if n >= 1 { return true; }
            }
            at += 1;
        }
        from = at;
    }
    false
}
fn ace_tail(content: &str, min: usize, hexdigit_lower_only: bool) -> bool {
    find_at(content, "ace_", min, |b| {
        if hexdigit_lower_only { b.is_ascii_digit() || (b'a'..=b'f').contains(&b) } else { b.is_ascii_hexdigit() }
    })
}
/// /ace_[a-f0-9]{30,}|sk-[a-zA-Z0-9_-]{20,}|-----BEGIN [A-Z ]*PRIVATE KEY-----|https:\/\/[^\s"']+token=[A-Za-z0-9%]+/i
pub fn pack_check(content: &str) -> bool {
    ace_tail(content, 30, true)
        || find_at(content, "sk-", 20, |b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || begins_private_key(content)
        || https_token(content)
}
/// /ace_[a-f0-9]{20,}|eyJ[A-Za-z0-9_-]{50,}\./
pub fn ace_jwt_check(content: &str) -> bool {
    ace_tail(content, 20, true)
        || {
            let needle = "eyJ";
            let mut from = 0;
            let mut found = false;
            while let Some(i) = content[from..].find(needle) {
                let at = from + i;
                let n = run_of(content, at + needle.len(), |b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
                if n >= 47 && content.as_bytes().get(at + needle.len() + n) == Some(&b'.') { found = true; break; }
                // regex is eyJ[chars]{50,}\. meaning eyJ + 50+ chars + '.'
                if at + 3 + 50 < content.len()
                    && run_of(content, at + 3, |b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') >= 50
                {
                    let end = at + 3 + run_of(content, at + 3, |b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
                    if content.as_bytes().get(end) == Some(&b'.') { found = true; break; }
                }
                from = at + 1;
            }
            found
        }
}
/// /(?:ace_|sk-)[a-zA-Z0-9]{15,}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY|Authorization:\s*Bearer [a-zA-Z0-9]/
pub fn scan_a(content: &str) -> bool {
    find_at(content, "ace_", 15, |b| b.is_ascii_alphanumeric())
        || find_at(content, "sk-", 15, |b| b.is_ascii_alphanumeric())
        || begins_typed_private_key(content)
        || auth_bearer(content)
}
/// /ace_[a-f\d]{20,}|sk-[a-z\d]{20,}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|[?&](?:token|access_token)=/i
pub fn scan_b(content: &str) -> bool {
    find_at(content, "ace_", 20, |b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || find_at(content, "sk-", 20, |b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || begins_typed_private_key(content)
        || token_url_param(content)
}
/// /ace_[a-f0-9]{20,}|github_pat_[a-z0-9_]+|ghp_[a-z0-9]{20,}|Bearer\s+[a-z0-9._-]{20,}/i
pub fn scan_c(content: &str) -> bool {
    let lower = content.to_lowercase();
    ace_tail(&lower, 20, true)
        || find_at(&lower, "github_pat_", 1, |b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        || find_at(&lower, "ghp_", 20, |b| b.is_ascii_lowercase() || b.is_ascii_digit())
        || {
            let needle = "bearer";
            let mut from = 0;
            let mut found = false;
            while let Some(i) = lower[from..].find(needle) {
                let mut at = from + i + needle.len();
                let ws = run_of(&lower, at, |b| b.is_ascii_whitespace());
                if ws >= 1 {
                    at += ws;
                    if run_of(&lower, at, |b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_' || b == b'-') >= 20 {
                        found = true; break;
                    }
                }
                from = at;
            }
            found
        }
}
