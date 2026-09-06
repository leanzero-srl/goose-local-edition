//! `goose recall "<request>"` — what the recall extension would add to the turn context for that
//! request, against the real memory store and skill catalogue, computed by the same functions the
//! extension runs. The instrument behind `evals/memory-recall/probe.py`, and the answer to "why was
//! this recalled".

use anyhow::Result;
use goose::agents::platform_extensions::recall::{
    query_terms, relevant_skills, render, select_hits,
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
        println!(
            "  {mark} {}/{} terms, {} rare, {} in name, score {:5.1}  {} ({}){named}{topic}",
            hit.matched_terms,
            terms.len(),
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
    match render(&selected, &skills, None) {
        Some(block) => println!("\n--- turn context part (past-session line omitted: it needs the session DB) ---\n{block}"),
        None => println!("\n--- turn context part: none ---"),
    }
    Ok(())
}
