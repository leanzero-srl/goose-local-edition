//! `goose memory scan-secrets` — which memory files still hold a secret value on disk, by file, line
//! and key NAME, never the value. Every read of the store is redacted since Q-505 (2026-09-29), so a
//! value left on disk no longer reaches a prompt; this report is how the owner finds the entries to
//! rewrite. It changes nothing.

use anyhow::Result;
use goose::config::paths::Paths;
use goose_memory_store::MemoryStore;
use std::collections::BTreeMap;

pub fn scan_secrets() -> Result<()> {
    let working_dir = std::env::current_dir()?;
    let store = MemoryStore::new(Paths::config_dir().join("memory"), &working_dir);
    let found = store.secret_scan()?;
    println!(
        "scanned {} and {}",
        store.global_dir.display(),
        store.local_dir.display()
    );
    if found.is_empty() {
        println!("no secret values found");
        return Ok(());
    }
    for hit in &found {
        println!("{}:{}  {}", hit.path.display(), hit.line, hit.key);
    }
    let mut by_key: BTreeMap<&str, usize> = BTreeMap::new();
    for hit in &found {
        *by_key.entry(hit.key.as_str()).or_default() += 1;
    }
    let summary: Vec<String> = by_key
        .iter()
        .map(|(key, count)| format!("{key} ×{count}"))
        .collect();
    println!(
        "{} secret value{} on disk ({}); values are never printed, nothing was changed",
        found.len(),
        if found.len() == 1 { "" } else { "s" },
        summary.join(", ")
    );
    Ok(())
}
