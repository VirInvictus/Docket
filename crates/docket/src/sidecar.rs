//! The view-state sidecar: fold state persisted under
//! ~/.local/state/docket/, keyed by document content hash, so folds
//! survive restarts and content-preserving renames and never touch
//! the document itself.

use std::path::PathBuf;

fn state_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("docket"))
}

fn sidecar_path(doc_hash: &str) -> Option<PathBuf> {
    Some(state_dir()?.join(format!("{doc_hash}.json")))
}

/// Load the folded-header hashes for a document hash. Missing or
/// unreadable sidecars mean no folds.
pub fn load(doc_hash: &str) -> Vec<u64> {
    let Some(path) = sidecar_path(doc_hash) else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    // Minimal shape: {"folded": ["<16 hex>", ...]} parsed without a
    // JSON dependency: take hex runs inside quotes.
    text.split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| s.len() == 16 && s.chars().all(|c| c.is_ascii_hexdigit()))
        .filter_map(|s| u64::from_str_radix(s, 16).ok())
        .collect()
}

/// Persist the folded-header hashes for a document hash. An empty
/// set still writes (clearing stale folds).
pub fn store(doc_hash: &str, folded: &[u64]) {
    let Some(path) = sidecar_path(doc_hash) else {
        return;
    };
    let mut body = String::from("{\"folded\":[");
    for (i, h) in folded.iter().enumerate() {
        if i > 0 {
            body.push(',');
        }
        body.push('"');
        body.push_str(&format!("{h:016x}"));
        body.push('"');
    }
    body.push_str("]}");
    if std::fs::create_dir_all(state_dir().unwrap_or_default()).is_ok() {
        let _ = std::fs::write(path, body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_then_load_round_trips() {
        let key = "roundtrip-test-only";
        store(key, &[0x0123_4567_89ab_cdef, 42]);
        let loaded = load(key);
        assert_eq!(loaded, vec![0x0123_4567_89ab_cdef, 42]);
        // Clean the scratch key.
        std::fs::remove_file(sidecar_path(key).unwrap()).ok();
    }

    #[test]
    fn missing_sidecar_is_empty() {
        assert!(load("no-such-sidecar-key").is_empty());
    }
}
