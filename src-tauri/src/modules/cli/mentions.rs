//! `@path` file mentions in agent prompts.
//!
//! When the user message contains `@<path>` tokens, replace them with an
//! inlined block of the file's contents (capped at 64 KB per file). The
//! original `@path` token stays in the message for traceability; the inlined
//! content is appended below.

use std::path::{Path, PathBuf};

const MAX_INLINE_BYTES: usize = 64 * 1024;
const MAX_FILES_PER_MESSAGE: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineFile {
    pub mention: String,
    pub resolved_path: PathBuf,
    pub content: String,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub struct MentionExpansion {
    pub message: String,
    pub inlined: Vec<InlineFile>,
    pub errors: Vec<String>,
}

/// Detect `@<path>` tokens in `message`. A mention starts at `@` preceded by
/// start-of-string or whitespace, and runs until the next whitespace.
/// Returns the original mentions in order of appearance (deduped by path).
pub fn extract_mentions(message: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = message.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        let at_word_start = i == 0 || bytes[i - 1].is_ascii_whitespace();
        if c == '@' && at_word_start {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && !(bytes[end] as char).is_whitespace() {
                end += 1;
            }
            if end > start {
                let path = &message[start..end];
                let trimmed = path.trim_end_matches(|c: char| {
                    matches!(c, '.' | ',' | ':' | ';' | '!' | '?' | ')' | ']' | '}')
                });
                if !trimmed.is_empty() && !out.iter().any(|m| m == trimmed) {
                    out.push(trimmed.to_string());
                }
            }
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// Resolve and inline `@path` mentions. Paths are resolved against `cwd` if
/// relative. Files outside `allowed_roots` (when non-empty) are rejected.
pub fn expand_mentions(message: &str, cwd: &Path, allowed_roots: &[PathBuf]) -> MentionExpansion {
    let mentions = extract_mentions(message);
    let mut inlined: Vec<InlineFile> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (idx, m) in mentions.iter().enumerate() {
        if idx >= MAX_FILES_PER_MESSAGE {
            errors.push(format!(
                "@-mention cap reached ({MAX_FILES_PER_MESSAGE}); skipped: @{m}"
            ));
            break;
        }
        let raw = Path::new(m);
        let resolved = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            cwd.join(raw)
        };
        let canonical = match std::fs::canonicalize(&resolved) {
            Ok(p) => p,
            Err(e) => {
                errors.push(format!("@{m}: {e}"));
                continue;
            }
        };
        if !allowed_roots.is_empty() && !is_under_any_root(&canonical, allowed_roots) {
            errors.push(format!(
                "@{m}: outside allowed roots (use `pengine fs add <root>` first)"
            ));
            continue;
        }
        match read_capped(&canonical, MAX_INLINE_BYTES) {
            Ok((content, truncated)) => inlined.push(InlineFile {
                mention: m.clone(),
                resolved_path: canonical,
                content,
                truncated,
            }),
            Err(e) => errors.push(format!("@{m}: {e}")),
        }
    }

    let mut message = message.to_string();
    if !inlined.is_empty() {
        message.push_str("\n\n## Mentioned files\n");
        for f in &inlined {
            let path = f.resolved_path.display();
            let trunc_note = if f.truncated { " (truncated)" } else { "" };
            message.push_str(&format!("\n--- @{} → {path}{trunc_note} ---\n", f.mention));
            message.push_str(&f.content);
            if !f.content.ends_with('\n') {
                message.push('\n');
            }
        }
    }
    MentionExpansion {
        message,
        inlined,
        errors,
    }
}

fn is_under_any_root(p: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| {
        std::fs::canonicalize(r)
            .map(|c| p.starts_with(c))
            .unwrap_or(false)
    })
}

fn read_capped(path: &Path, cap: usize) -> Result<(String, bool), String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| format!("open: {e}"))?;
    let mut buf = Vec::with_capacity(cap.min(8192));
    let mut chunk = [0u8; 8192];
    let mut total = 0usize;
    let mut truncated = false;
    loop {
        let n = f.read(&mut chunk).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        let take = n.min(cap.saturating_sub(total));
        buf.extend_from_slice(&chunk[..take]);
        total = total.saturating_add(take);
        if total >= cap {
            truncated = true;
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    Ok((text, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn extract_basic_mentions() {
        let m = extract_mentions("look at @foo.rs and @bar/baz.txt please");
        assert_eq!(m, vec!["foo.rs".to_string(), "bar/baz.txt".to_string()]);
    }

    #[test]
    fn extract_strips_trailing_punctuation() {
        let m = extract_mentions("see @foo.rs, @bar.rs.");
        assert_eq!(m, vec!["foo.rs".to_string(), "bar.rs".to_string()]);
    }

    #[test]
    fn extract_skips_email_like_at() {
        // @ not at word boundary should be skipped (email-ish)
        let m = extract_mentions("ping me at user@example.com");
        assert!(m.is_empty(), "got: {m:?}");
    }

    #[test]
    fn expand_inlines_file_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("hi.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello world").unwrap();
        let exp = expand_mentions(&format!("show @{}", path.display()), dir.path(), &[]);
        assert!(exp.message.contains("hello world"));
        assert_eq!(exp.inlined.len(), 1);
        assert!(exp.errors.is_empty());
    }

    #[test]
    fn expand_rejects_outside_root() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let path = outside.path().join("o.txt");
        std::fs::write(&path, "x").unwrap();
        let exp = expand_mentions(
            &format!("@{}", path.display()),
            dir.path(),
            &[dir.path().to_path_buf()],
        );
        assert!(exp.inlined.is_empty());
        assert_eq!(exp.errors.len(), 1);
        assert!(exp.errors[0].contains("outside allowed roots"));
    }
}
