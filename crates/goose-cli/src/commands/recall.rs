//! `goose recall "<request>"` — what the recall extension would add to the turn context for that
//! request, against the real memory store and skill catalogue, computed by the same functions the
//! extension runs. The instrument behind `evals/memory-recall/probe.py`, and the answer to "why was
//! this recalled".

use anyhow::Result;
use goose::agents::platform_extensions::recall::{
    query_terms, relevant_skills, render, select_hits, skill_hits,
};
use goose::config::paths::Paths;
use goose_memory_store::MemoryStore;

pub fn run(text: &str) -> Result<()> {
    let working_dir = std::env::current_dir()?;
    let terms = query_terms(text);
    println!("terms: {}", terms.join(" "));
    if terms.is_empty() {
        println!("nothing to search: every word is a function word");
        return Ok(());
    }
    let store = MemoryStore::new(Paths::config_dir().join("memory"), &working_dir);
    let hits = store.search(&terms.join(" "), None)?;
    let selected = select_hits(hits.clone(), terms.len());
    println!(
        "\nmemories ({} matched, {} would ride along):",
        hits.len(),
        selected.len()
    );
    for hit in hits.iter().take(8) {
        let mark = if selected
            .iter()
            .any(|s| s.entry.category == hit.entry.category && s.entry.content == hit.entry.content)
        {
            "RECALL"
        } else {
            "      "
        };
        let named = if hit.named { " [named]" } else { "" };
        let topic = if hit.topic_in_name { " [topic]" } else { "" };
        let identifier = if hit.identifier_in_name || hit.identifier_in_body {
            " [identifier]"
        } else {
            ""
        };
        let whole = match (hit.matched_terms >= terms.len(), hit.together) {
            (true, true) => " [together]",
            (true, false) => " [apart]",
            _ => "",
        };
        println!(
            "  {mark} {}/{} terms, {}/{} specific, {} rare, {} in name, score {:5.1}  {} ({}){named}{identifier}{topic}{whole}",
            hit.matched_terms,
            terms.len(),
            hit.matched_specific,
            hit.specific_terms,
            hit.rare_terms,
            hit.name_terms,
            hit.score,
            hit.entry.category,
            hit.entry.scope_label()
        );
    }
    let catalogue = goose::skills::discover_skills(Some(&working_dir));
    let skills = relevant_skills(&catalogue, &terms);
    println!(
        "\nskills ({} in catalogue, {} suggested):",
        catalogue.len(),
        skills.len()
    );
    for skill in &skills {
        println!("  {}", skill.name);
    }
    let candidates = skill_hits(&catalogue, &terms);
    if !candidates.is_empty() {
        println!("  candidates:");
    }
    for hit in candidates.iter().take(6) {
        let mark = if skills.iter().any(|s| s.name == hit.skill.name) {
            "SUGGEST"
        } else {
            "       "
        };
        let together = if hit.together {
            " [together]"
        } else {
            " [apart]"
        };
        println!(
            "  {mark} {}/{} terms, {} rare, {} in name ({} its own), score {:5.1}  {}{together}",
            hit.matched_terms,
            terms.len(),
            hit.rare_terms,
            hit.name_terms,
            hit.own_name_terms,
            hit.score,
            hit.skill.name
        );
    }
    match render(&selected, &skills, None) {
        Some(block) => println!("\n--- turn context part (past-session line omitted: it needs the session DB) ---\n{block}"),
        None => println!("\n--- turn context part: none ---"),
    }
    Ok(())
}
