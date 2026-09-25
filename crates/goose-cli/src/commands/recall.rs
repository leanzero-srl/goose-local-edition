//! `goose recall "<request>"` — what the recall extension would add to the turn context for that
//! request, against the real memory store and skill catalogue, computed by the same functions the
//! extension runs. The instrument behind `evals/memory-recall/probe.py`, and the answer to "why was
//! this recalled".

use anyhow::Result;
use goose::agents::platform_extensions::recall::{
    autoload_pick, past_session_candidates, query_terms, relevant_skill_hits, relevant_skills,
    render, select_hits, select_past_session, skill_hits, skill_term_frequency, PAST_SESSION_ROWS,
};
use goose::config::paths::Paths;
use goose::session::session_manager::{SessionManager, SessionType};
use goose_memory_store::MemoryStore;

pub async fn run(text: &str, session: Option<&str>) -> Result<()> {
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
    let frequency: Vec<String> = terms
        .iter()
        .zip(skill_term_frequency(&catalogue, &terms))
        .map(|(term, (df, nf))| format!("{term} {df}/{nf}"))
        .collect();
    println!(
        "\nskills ({} in catalogue, {} suggested):",
        catalogue.len(),
        skills.len()
    );
    for skill in &skills {
        println!("  {}", skill.name);
    }
    println!(
        "  catalogue frequency (text/names): {}",
        frequency.join(", ")
    );
    if let Some(named) = autoload_pick(&relevant_skill_hits(&catalogue, &terms), usize::MAX / 8) {
        println!(
            "  names {} — loaded without a call when its {} chars fit a thirty-second of the window",
            named.name,
            named.content.chars().count()
        );
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
        let topic = if hit.topic_in_name { " [topic]" } else { "" };
        let body = if hit.body_carries_request {
            " [body]"
        } else {
            ""
        };
        println!(
            "  {mark} {}/{} terms, {} rare, {} in name ({} name words, {} its own), score {:5.1}  {}{together}{topic}{body}",
            hit.matched_terms,
            terms.len(),
            hit.rare_terms,
            hit.name_terms,
            hit.name_words,
            hit.own_name_terms,
            hit.score,
            hit.skill.name
        );
    }
    let sessions = SessionManager::instance();
    let before = match session {
        Some(id) => Some(sessions.get_session(id, false).await?.created_at),
        None => None,
    };
    let history = sessions
        .search_chat_history(
            &terms.join(" "),
            Some(PAST_SESSION_ROWS),
            None,
            before,
            session.map(String::from),
            vec![SessionType::User, SessionType::Scheduled],
        )
        .await?;
    let past = select_past_session(&history.results, &terms);
    println!(
        "\npast session ({} rows share a word, {}):",
        history
            .results
            .iter()
            .map(|r| r.messages.len())
            .sum::<usize>(),
        past.as_ref().map_or("none named".to_string(), |p| format!(
            "names {}",
            p.session_id
        ))
    );
    for line in past_session_candidates(&history.results, &terms)
        .iter()
        .take(6)
    {
        println!("  {line}");
    }
    match render(&selected, &skills, past.as_ref()) {
        Some(block) => println!("\n--- turn context part ---\n{block}"),
        None => println!("\n--- turn context part: none ---"),
    }
    Ok(())
}
