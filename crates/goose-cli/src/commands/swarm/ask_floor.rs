//! The ASK floor's planner-size heuristics: how many billion parameters are ACTIVE in the planner's
//! model id, and how much sooner a weaker planner should ask. Sibling module under the
//! incremental-split law (development_gates::swarm_rs_line_count_only_decreases). Moved verbatim
//! from swarm.rs with its test — behavior unchanged — paying for the VA-013/VA-019 wiring at the
//! judge's summon site (evidence-only looks on build lanes; node / secs / forming bytes on every
//! look) landing in the same commit.

/// Parse the ACTIVE parameter count (in billions) from a model id, for GOOSE_SWARM_ASK floor scaling. A MoE
/// id like `qwen3.6-35b-a3b` exposes ~3B ACTIVE (weaker than a 27B dense despite 35 total), so the `a<N>b`
/// active marker WINS over the leading dense `<N>b` size. Returns None if unparseable. HEURISTIC — fuzzy.
pub(super) fn model_active_params_b(model_id: &str) -> Option<u32> {
    let id = model_id.to_lowercase();
    let tokens: Vec<&str> = id.split(|c: char| !c.is_ascii_alphanumeric()).collect();
    // 1) MoE active marker "a<N>b" takes precedence (the real compute size).
    for t in &tokens {
        if let Some(rest) = t.strip_prefix('a') {
            if let Some(num) = rest.strip_suffix('b') {
                if let Ok(n) = num.parse::<u32>() {
                    if (1..=2000).contains(&n) {
                        return Some(n);
                    }
                }
            }
        }
    }
    // 2) Mixtral-style "NxMb" dense-expert MoE: the per-expert size M is a rough ACTIVE proxy (only a couple
    // of experts fire per token), so read M, not the N×M total.
    for t in &tokens {
        if let Some((_, rest)) = t.split_once('x') {
            if let Some(num) = rest.strip_suffix('b') {
                if let Ok(n) = num.parse::<u32>() {
                    if (1..=2000).contains(&n) {
                        return Some(n);
                    }
                }
            }
        }
    }
    // 3) else the dense size "<N>b".
    for t in &tokens {
        if let Some(num) = t.strip_suffix('b') {
            if let Ok(n) = num.parse::<u32>() {
                if (1..=2000).contains(&n) {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// How much to RAISE the ask floor for a planner of `active_b` billion active params — weaker -> ask sooner.
/// HEURISTIC; small + bounded. None (unknown) gets a mild bump.
pub(super) fn ask_floor_weak_bump(active_b: Option<u32>) -> u8 {
    match active_b {
        Some(n) if n >= 30 => 0, // strong dense (e.g. 30B+)
        Some(n) if n >= 13 => 5, // mid (e.g. 13-27B)
        Some(n) if n >= 7 => 10, // small dense (7-12B)
        Some(_) => 15,           // <7B active (e.g. an a3b MoE) -> ask much sooner
        None => 5,               // unknown id -> mild bump
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_active_params_and_weak_bump() {
        // MoE active marker wins over the dense total (a3b = 3B active, weaker than the 35B total).
        assert_eq!(model_active_params_b("qwen/qwen3.6-35b-a3b"), Some(3));
        // dense size when there is no active marker.
        assert_eq!(model_active_params_b("qwen/qwen3.6-27b"), Some(27));
        assert_eq!(model_active_params_b("llama-3.1-8b-instruct"), Some(8));
        // Mixtral-style NxMb -> the per-expert size M as the active proxy.
        assert_eq!(model_active_params_b("mixtral-8x7b-instruct"), Some(7));
        assert_eq!(model_active_params_b("some-unsized-model"), None);
        // Weaker (fewer active params) -> bigger bump (ask sooner); strong -> no bump.
        assert_eq!(ask_floor_weak_bump(Some(27)), 5);
        assert_eq!(ask_floor_weak_bump(Some(3)), 15); // a3b MoE
        assert_eq!(ask_floor_weak_bump(Some(70)), 0); // strong
        assert_eq!(ask_floor_weak_bump(None), 5);
        assert!(ask_floor_weak_bump(Some(3)) > ask_floor_weak_bump(Some(27)));
    }
}
