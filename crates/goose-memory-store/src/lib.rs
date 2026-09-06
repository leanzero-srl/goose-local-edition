//! The on-disk memory store goose's memory extension writes and its recall reads.
//!
//! One category per `<category>.txt` file, entries separated by a blank line, an optional first line
//! `# tag tag` carrying the tags. Two scopes: a global directory (user-wide) and a local one under the
//! project's `.goose/memory`. This crate is the single owner of that format, the index rendering and the
//! keyword search, so the extension that serves the tools and the recall that runs every turn cannot
//! drift apart.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One memory as stored on disk: an optionally tagged entry inside a category file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryEntry {
    pub is_global: bool,
    pub category: String,
    pub tags: Vec<String>,
    pub content: String,
}

impl MemoryEntry {
    /// The one index line an entry gets: its first non-empty line, cut at a word boundary past the
    /// headline width so a paragraph-shaped entry cannot swallow the index.
    pub fn headline(&self) -> String {
        headline(&self.content)
    }

    pub fn scope_label(&self) -> &'static str {
        scope_label(self.is_global)
    }

    /// `## category (scope [tags])` followed by the full content.
    pub fn render(&self) -> String {
        let tags = if self.tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", self.tags.join(" "))
        };
        format!(
            "## {} ({}{})\n{}\n",
            self.category,
            self.scope_label(),
            tags,
            self.content
        )
    }
}

/// A search hit. `score` is the sum of the matched terms' rarity weights (a term found in few of the
/// searched entries weighs more than one found in most; a term in the entry's NAME — category, tags,
/// headline — counts twice; a whole-phrase match adds every term's weight again). `rare_terms` counts the
/// matched terms that occur in at most half of the searched entries — the ones that carry information.
///
/// `named` says the query NAMES the entry: at least [`NAMED_MIN_NAME_TERMS`] of its terms sit in the
/// entry's name, more than half of its terms match, and at least half of its SPECIFIC terms match —
/// the terms no commoner than the query's median term (`specific_terms`, `matched_specific`; a term
/// no searched entry has is neither). A named hit ranks above every unnamed one whatever the body
/// score — a body that happens to contain every query word is a vocabulary match, a headline carrying
/// two of the query's words is the topic. The name is read by [`name_tokens`]: a hyphenated compound
/// in a headline is one word, so "goose-local" is not "local". The specific half is what refuses a
/// name made of the query's GENERIC words (VA-182): "search Jira issues with JQL over the Jira Cloud
/// REST API" matched api, cloud, jira and rest — four of seven terms, the four commonest, one of the
/// specific four (jql, issues, cloud, search) — in a note on classification licensing named by
/// "Cloud REST endpoints"; every true name on the probe matches half of its request's specific terms
/// or more. Ranks, not weights: on a ten-note store one filler adverb ("properly", in one note) is 58%
/// of the request's weight, and a weight floor un-named "During a benchmark run".
///
/// `topic_in_name` says the entry's name carries the query's TOPIC WORD — its rarest term among those
/// found in at least one searched entry (a term nobody has names no topic in this store; ties count
/// every tied term). It is what separates an entry ABOUT a request from one that shares its words
/// when only one of the query's terms sits in the name. Measured (VA-181, 233 entries): "connect to the
/// workhorse over SSH" has the topic "ssh" — the two notes named only by "workhorse" (JACCL cluster,
/// WindowServer OOM) carry it nowhere in their names; "deploy the Forge app" has the topic "deploy",
/// which the named Forge-deployment note carries and the bank-desk note ("Forge + DC-to-Cloud
/// migration engagement", named only by "forge") does not.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub score: f64,
    pub matched_terms: usize,
    pub rare_terms: usize,
    pub phrase: bool,
    pub name_terms: usize,
    pub specific_terms: usize,
    pub matched_specific: usize,
    pub named: bool,
    pub topic_in_name: bool,
    pub occurrences: usize,
    pub entry: MemoryEntry,
}

// measured: on the 233-entry store (13 probe requests, 2026-09-06) one request term in a headline is a
// shared word ("goose" names both a release-loop note and the branch map for "which git identity"), two
// is the request's topic ("benchmark run" in the 5-minute-tick note, "run… starting" in the fleet-check
// note, for "how do I start a benchmark run properly?" — where a note on screenshotting frontends held
// slot 1 on body words alone); the same pair the skill auto-load needs, rarity not required because the
// common word of the pair ("run") is the topic's own word. The more-than-half floor keeps a half-match
// out of the tier: "forge" + "app" in a note about the Assets API (2/4) must not outrank the
// whole-request bank-approval note (4/4, nameless) for "deploy the Forge app to production".
// measured (VA-180, same store): rarity of the pair does NOT separate a false name from a true one —
// "local" (60 of 233) and "models" (25) are both rare and together weigh 3.57, more than
// "benchmark" + "run" (18 + 131, 3.12) or "run" + "start" (2.52); what named the tool-call note for
// "write a blog post about local models" was the compound "goose-local" split into "goose" + "local".
// measured (VA-182, same store): a count majority lets a request's common words outvote its specific
// ones — "search Jira issues with JQL over the Jira Cloud REST API" (jql df 3, issues 10, cloud 20,
// search 26 | jira 40, api 45, rest 55) named the Guard-Premium classification note on api + cloud +
// jira + rest, 4 of 7 by count but one of the four terms no commoner than the median (cloud); the
// thirteen true names match half of their request's specific terms (nine of them exactly half: one
// of two on a four-term request) to all of them. A rarity-WEIGHT majority was refuted on the way: it
// drops the same note (43%) but on a ten-note store "properly" in one note is 58% of "start a
// benchmark run properly" and un-names "During a benchmark run". Neither separates the two notes
// named fix + test for "fix the failing test in the scheduler" (3/4 each, one of the two specific
// words each — "scheduler_mock tests" in one, "the exact failing invocation" in the other, the two
// tie at df 6): by every measurement the store takes they are the same shape.
pub const NAMED_MIN_NAME_TERMS: usize = 2;

/// Rarity weight of a term found in `df` of `n` documents: `ln((n + 1) / (df + 0.5))` — large for a term
/// few documents carry, small but never zero for one every document carries, so a name match still
/// breaks a tie between entries that share only common words.
pub fn rarity_weight(n: usize, df: usize) -> f64 {
    ((n as f64 + 1.0) / (df as f64 + 0.5)).ln()
}

// measured: 171 imported entries on the first machine — first-line median 153 chars, p90 234, max 422;
// p90 keeps nine in ten headlines whole and cuts only the paragraph-shaped outliers.
pub const INDEX_HEADLINE_CHARS: usize = 240;

pub fn scope_label(is_global: bool) -> &'static str {
    if is_global {
        "global"
    } else {
        "local"
    }
}

/// Split a category file into `(tags, content)` entries.
pub fn parse_entries(content: &str) -> Vec<(Vec<String>, String)> {
    content
        .split("\n\n")
        .filter_map(|entry| {
            let entry = entry.trim_matches('\n');
            if entry.trim().is_empty() {
                return None;
            }
            let mut lines = entry.lines();
            let first = lines.next()?;
            match first.strip_prefix('#') {
                Some(stripped) => {
                    let tags = stripped.split_whitespace().map(String::from).collect();
                    let body: Vec<&str> = lines.collect();
                    Some((tags, body.join("\n")))
                }
                None => Some((Vec::new(), entry.to_string())),
            }
        })
        .collect()
}

/// Serialize one entry the way `remember` writes it: the tag line (when tagged), the content, and the
/// blank line that separates entries.
pub fn format_entry(tags: &[String], content: &str) -> String {
    let mut out = String::new();
    if !tags.is_empty() {
        out.push_str(&format!("# {}\n", tags.join(" ")));
    }
    out.push_str(content);
    out.push_str("\n\n");
    out
}

pub fn headline(content: &str) -> String {
    let line = content
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.chars().count() <= INDEX_HEADLINE_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(INDEX_HEADLINE_CHARS).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{cut}…")
}

/// Lower-cased alphanumeric word tokens of a text, in order, repeats kept.
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect()
}

// ratio: a prefix needs four characters before it may stand for a longer word — "dock" is not
// "docker", but "docker" is "dockerized" and "compact" is "compaction".
const PREFIX_MIN_CHARS: usize = 4;

/// How many tokens a query term matches: the whole word, or a longer word it is a prefix of. A term is
/// never matched inside another word ("rest" is not in "research").
pub fn term_occurrences(term: &str, tokens: &[String]) -> usize {
    tokens
        .iter()
        .filter(|token| {
            token.as_str() == term
                || (term.chars().count() >= PREFIX_MIN_CHARS && token.starts_with(term))
        })
        .count()
}

/// The words of a headline or tag line as a NAME reads them: a hyphenated compound is one word
/// ("goose-local" is not "local", "plan-confidence" is not "plan"), kept as its joined form; its
/// parts count only when the request writes the same compound — `search_terms` gives "load-bearing"
/// as load, bearing and loadbearing, so the parts reach a headline's "load-bearing" while
/// "local models" does not reach "goose-local". Measured (VA-180): the compound split named
/// `improve-toolcall-reliability` ("the goose-local swarm workers' … weak models") for "write a blog
/// post about local models"; the twelve other probe requests keep their recalled sets and order.
pub fn name_tokens(text: &str, terms: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.to_lowercase().split_whitespace() {
        let word = word.trim_matches(|c: char| !c.is_alphanumeric());
        let compound = word.contains('-')
            && word.chars().all(|c| c.is_alphanumeric() || c == '-')
            && !word.split('-').any(str::is_empty);
        if compound {
            let joined: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
            if terms.contains(&joined) {
                out.extend(word.split('-').map(String::from));
            }
            out.push(joined);
        } else {
            out.extend(tokenize(word));
        }
    }
    out
}

/// Lower-cased, de-duplicated alphanumeric terms of a query. A hyphenated compound contributes its
/// parts AND its joined form, so "hard-coded" reaches an entry that says "hardcoded".
pub fn search_terms(query: &str) -> Vec<String> {
    let lower = query.to_lowercase();
    let mut terms: Vec<String> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect();
    for word in lower.split_whitespace() {
        let word = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
        if word.contains('-') {
            let joined: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
            if !joined.is_empty() {
                terms.push(joined);
            }
        }
    }
    terms.sort();
    terms.dedup();
    terms
}

fn is_reserved_windows_category(category: &str) -> bool {
    let basename = category
        .split('.')
        .next()
        .unwrap_or(category)
        .trim_end_matches([' ', '.']);
    let uppercase = basename.to_ascii_uppercase();

    matches!(uppercase.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || ["COM", "LPT"].iter().any(|prefix| {
            uppercase.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
}

/// A category is one plain file-name component; anything that could escape the directory or name a
/// device is refused.
pub fn validate_category(category: &str) -> io::Result<()> {
    if category.is_empty()
        || category == "*"
        || category == "."
        || category == ".."
        || category.contains('/')
        || category.contains('\\')
        || category.contains(':')
        || is_reserved_windows_category(category)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "memory category must be a single filename component",
        ));
    }
    Ok(())
}

/// The two directories one session's memories live in.
#[derive(Debug, Clone)]
pub struct MemoryStore {
    pub global_dir: PathBuf,
    pub local_dir: PathBuf,
}

impl MemoryStore {
    /// `local_dir` is the project's `.goose/memory`, derived from the working directory.
    pub fn new(global_dir: PathBuf, working_dir: &Path) -> Self {
        Self {
            global_dir,
            local_dir: working_dir.join(".goose").join("memory"),
        }
    }

    pub fn scope_dir(&self, is_global: bool) -> &Path {
        if is_global {
            &self.global_dir
        } else {
            &self.local_dir
        }
    }

    pub fn category_file(&self, category: &str, is_global: bool) -> io::Result<PathBuf> {
        validate_category(category)?;
        Ok(self.scope_dir(is_global).join(format!("{category}.txt")))
    }

    /// Every entry in one scope, category files in name order, entries in file order.
    pub fn entries(&self, is_global: bool) -> io::Result<Vec<MemoryEntry>> {
        let base_dir = self.scope_dir(is_global);
        let mut entries = Vec::new();
        if !base_dir.exists() {
            return Ok(entries);
        }
        let mut files = fs::read_dir(base_dir)?.collect::<io::Result<Vec<_>>>()?;
        files.sort_by_key(|entry| entry.file_name());
        for file in files {
            if !file.file_type()?.is_file() {
                continue;
            }
            let file_name = file.file_name();
            let Some(category) = file_name
                .to_str()
                .and_then(|name| name.strip_suffix(".txt"))
            else {
                continue;
            };
            if validate_category(category).is_err() {
                continue;
            }
            let content = fs::read_to_string(file.path())?;
            for (tags, body) in parse_entries(&content) {
                entries.push(MemoryEntry {
                    is_global,
                    category: category.to_string(),
                    tags,
                    content: body,
                });
            }
        }
        Ok(entries)
    }

    /// The index a model sees at startup: one headline per entry, both scopes, bodies never included.
    /// A scope that cannot be read says so instead of appearing empty.
    pub fn index(&self) -> String {
        let mut out = String::new();
        for (is_global, label) in [
            (true, "Global memories"),
            (false, "Project memories (.goose/memory)"),
        ] {
            match self.entries(is_global) {
                Ok(entries) if entries.is_empty() => {
                    out.push_str(&format!("\n{label}: none saved yet.\n"));
                }
                Ok(entries) => {
                    out.push_str(&format!(
                        "\n{label} ({} entries, is_global={is_global}):\n",
                        entries.len()
                    ));
                    for entry in &entries {
                        let tags = if entry.tags.is_empty() {
                            String::new()
                        } else {
                            format!(" [{}]", entry.tags.join(" "))
                        };
                        out.push_str(&format!(
                            "- {}{}: {}\n",
                            entry.category,
                            tags,
                            entry.headline()
                        ));
                    }
                }
                Err(err) => {
                    out.push_str(&format!(
                        "\n{label}: could not be read ({err}) — this part of the index is missing.\n"
                    ));
                }
            }
        }
        out
    }

    /// Keyword search over category, tags and content: the entries the query NAMES first (see
    /// [`SearchHit::named`]), then by rarity-weighted score — an entry ABOUT the topic (the term in its
    /// name, or several rare terms) outranks one that shares common words with the query. Ties break on
    /// category name.
    pub fn search(&self, query: &str, is_global: Option<bool>) -> io::Result<Vec<SearchHit>> {
        let terms = search_terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let phrase = query.trim().to_lowercase();
        let scopes: Vec<bool> = match is_global {
            Some(scope) => vec![scope],
            None => vec![true, false],
        };
        let mut corpus = Vec::new();
        for scope in scopes {
            for entry in self.entries(scope)? {
                let name = format!(
                    "{} {} {}",
                    entry.category,
                    entry.tags.join(" "),
                    entry.headline()
                );
                let haystack = format!("{} {}", name, entry.content).to_lowercase();
                let name_tokens = [
                    tokenize(&entry.category),
                    name_tokens(&entry.tags.join(" "), &terms),
                    name_tokens(&entry.headline(), &terms),
                ]
                .concat();
                let tokens = tokenize(&haystack);
                corpus.push((entry, name_tokens, tokens, haystack));
            }
        }
        let n = corpus.len();
        let document_frequency: Vec<usize> = terms
            .iter()
            .map(|term| {
                corpus
                    .iter()
                    .filter(|(_, _, tokens, _)| term_occurrences(term, tokens) > 0)
                    .count()
            })
            .collect();
        let weights: Vec<f64> = document_frequency
            .iter()
            .map(|&df| rarity_weight(n, df))
            .collect();
        let all_weights: f64 = weights.iter().sum();
        let mut found_df: Vec<usize> = document_frequency
            .iter()
            .copied()
            .filter(|&df| df >= 1)
            .collect();
        found_df.sort_unstable();
        let median_df_doubled = match found_df.len() {
            0 => 0,
            k if k % 2 == 1 => found_df[k / 2] * 2,
            k => found_df[k / 2 - 1] + found_df[k / 2],
        };
        let specific: Vec<bool> = document_frequency
            .iter()
            .map(|&df| df >= 1 && df * 2 <= median_df_doubled)
            .collect();
        let specific_terms = specific.iter().filter(|&&s| s).count();
        let topic_weight = weights
            .iter()
            .zip(&document_frequency)
            .filter(|(_, &df)| df >= 1)
            .map(|(&weight, _)| weight)
            .fold(0.0_f64, f64::max);
        let topic_terms: Vec<&String> = terms
            .iter()
            .zip(&document_frequency)
            .zip(&weights)
            .filter(|((_, &df), &weight)| df >= 1 && weight == topic_weight)
            .map(|((term, _), _)| term)
            .collect();

        let mut hits = Vec::new();
        for (entry, name_tokens, tokens, haystack) in corpus {
            let mut score = 0.0;
            let mut matched_terms = 0;
            let mut matched_specific = 0;
            let mut rare_terms = 0;
            let mut name_terms = 0;
            let mut occurrences = 0;
            for (i, term) in terms.iter().enumerate() {
                let count = term_occurrences(term, &tokens);
                if count == 0 {
                    continue;
                }
                matched_terms += 1;
                if specific[i] {
                    matched_specific += 1;
                }
                occurrences += count;
                score += weights[i];
                if document_frequency[i] * 2 <= n {
                    rare_terms += 1;
                }
                if term_occurrences(term, &name_tokens) > 0 {
                    name_terms += 1;
                    score += weights[i];
                }
            }
            if matched_terms == 0 {
                continue;
            }
            let phrase = terms.len() > 1 && haystack.contains(&phrase);
            if phrase {
                score += all_weights;
            }
            let named = name_terms >= NAMED_MIN_NAME_TERMS
                && matched_terms * 2 > terms.len()
                && matched_specific * 2 >= specific_terms;
            let topic_in_name = topic_terms
                .iter()
                .any(|term| term_occurrences(term, &name_tokens) > 0);
            hits.push(SearchHit {
                score,
                matched_terms,
                rare_terms,
                phrase,
                name_terms,
                specific_terms,
                matched_specific,
                named,
                topic_in_name,
                occurrences,
                entry,
            });
        }
        hits.sort_by(|a, b| {
            b.named
                .cmp(&a.named)
                .then_with(|| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.entry.category.cmp(&b.entry.category))
        });
        Ok(hits)
    }

    /// Append an entry, or replace the entry in the same category and scope whose headline matches
    /// the new content's headline. An entry identical to one already stored is left alone.
    pub fn remember(
        &self,
        category: &str,
        content: &str,
        tags: &[String],
        is_global: bool,
    ) -> io::Result<RememberOutcome> {
        let path = self.category_file(category, is_global)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let existing = if path.exists() {
            fs::read_to_string(&path)?
        } else {
            String::new()
        };
        let mut entries = parse_entries(&existing);
        let content = content.trim_matches('\n');
        let incoming_headline = headline(content);

        if entries
            .iter()
            .any(|(stored_tags, stored)| stored == content && stored_tags == tags)
        {
            return Ok(RememberOutcome::Unchanged);
        }
        let outcome = match entries.iter().position(|(_, stored)| {
            !incoming_headline.is_empty() && headline(stored) == incoming_headline
        }) {
            Some(index) => {
                entries[index] = (tags.to_vec(), content.to_string());
                RememberOutcome::Updated
            }
            None => {
                entries.push((tags.to_vec(), content.to_string()));
                RememberOutcome::Added
            }
        };
        let serialized: String = entries
            .iter()
            .map(|(tags, content)| format_entry(tags, content))
            .collect();
        fs::write(&path, serialized)?;
        Ok(outcome)
    }
}

/// What `MemoryStore::remember` did with the entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RememberOutcome {
    Added,
    Updated,
    Unchanged,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn store_in(temp_dir: &tempfile::TempDir) -> MemoryStore {
        MemoryStore::new(
            temp_dir.path().join("global"),
            &temp_dir.path().join("project"),
        )
    }

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn index_lists_one_headline_per_entry_and_never_the_body() {
        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        store
            .remember(
                "build-commands",
                "Run `just release-binary` for a release build.\nThe debug build is `cargo build`.",
                &tags(&["project", "build"]),
                true,
            )
            .unwrap();
        store
            .remember(
                "build-commands",
                "Tests live under crates/<crate>/tests.\nNever put them in src.",
                &[],
                true,
            )
            .unwrap();

        let index = store.index();

        assert!(
            index.contains("Global memories (2 entries, is_global=true):"),
            "{index}"
        );
        assert!(index.contains(
            "- build-commands [project build]: Run `just release-binary` for a release build."
        ));
        assert!(index.contains("- build-commands: Tests live under crates/<crate>/tests."));
        assert!(
            !index.contains("The debug build"),
            "bodies must stay out of the index: {index}"
        );
        assert!(!index.contains("Never put them in src"));
        assert_eq!(index.matches("\n- ").count(), 2);
        assert!(index.contains("Project memories (.goose/memory): none saved yet."));
    }

    #[test]
    fn index_covers_project_memories() {
        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        store
            .remember("ports", "The API listens on 8850.", &tags(&["env"]), false)
            .unwrap();

        let index = store.index();

        assert!(index.contains("Global memories: none saved yet."));
        assert!(index.contains("Project memories (.goose/memory) (1 entries, is_global=false):"));
        assert!(index.contains("- ports [env]: The API listens on 8850."));
    }

    #[test]
    fn index_headline_is_cut_at_a_word_boundary() {
        let long = "word ".repeat(120);
        let cut = headline(&long);
        assert!(cut.ends_with('…'));
        assert!(cut.chars().count() <= INDEX_HEADLINE_CHARS + 1);
        let body = cut.trim_end_matches('…');
        assert!(body.split(' ').all(|token| token == "word"), "{cut}");
        assert_eq!(headline("\n\n  short line  \nsecond"), "short line");
    }

    #[test]
    fn search_returns_full_entries_best_match_first() {
        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        store
            .remember(
                "postgres",
                "The postgres database runs in docker on port 5432.\nUse `make db-up`.",
                &tags(&["env", "database"]),
                true,
            )
            .unwrap();
        store
            .remember(
                "docker",
                "Docker desktop must be running before tests.",
                &[],
                true,
            )
            .unwrap();
        store
            .remember(
                "editor",
                "The user prefers tabs.",
                &tags(&["preference"]),
                false,
            )
            .unwrap();

        let hits = store.search("docker database", None).unwrap();

        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].entry.category, "postgres");
        assert_eq!(hits[0].matched_terms, 2);
        assert!(!hits[0].phrase);
        assert_eq!(
            hits[0].entry.content,
            "The postgres database runs in docker on port 5432.\nUse `make db-up`."
        );
        assert_eq!(hits[1].entry.category, "docker");

        let phrase = store.search("prefers tabs", None).unwrap();
        assert_eq!(phrase.len(), 1);
        assert!(phrase[0].phrase);
        assert!(!phrase[0].entry.is_global);
    }

    #[test]
    fn search_ranks_an_entry_about_the_topic_above_one_that_mentions_it() {
        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        store
            .remember(
                "note-4dc15f",
                "The note-4b870e tick loop runs from launchd.\nA scheduled claude process auto-denies tools.",
                &tags(&["project"]),
                true,
            )
            .unwrap();
        store
            .remember(
                "reaping",
                "Kill pids one by one; a process group kill takes the engine with it.",
                &tags(&["feedback"]),
                true,
            )
            .unwrap();

        let hits = store.search("process", None).unwrap();

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].entry.category, "reaping", "{hits:?}");
        assert_eq!(hits[0].name_terms, 1);
        assert_eq!(hits[1].name_terms, 0);
    }

    #[test]
    fn search_honours_scope_and_empty_queries() {
        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        store
            .remember("hosts", "workhorse is 192.168.8.220", &[], true)
            .unwrap();
        store
            .remember("hosts", "the staging host is workhorse-2", &[], false)
            .unwrap();

        assert_eq!(store.search("workhorse", None).unwrap().len(), 2);
        let local_only = store.search("workhorse", Some(false)).unwrap();
        assert_eq!(local_only.len(), 1);
        assert!(!local_only[0].entry.is_global);
        assert!(store.search("  ,, ", None).unwrap().is_empty());
        assert!(store.search("nothing-like-this", None).unwrap().is_empty());
    }

    #[test]
    fn remember_updates_the_entry_with_the_same_headline_and_ignores_an_identical_one() {
        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        let first = store
            .remember(
                "editor",
                "Indentation: tabs.\nThe user said so on Monday.",
                &tags(&["user"]),
                true,
            )
            .unwrap();
        assert_eq!(first, RememberOutcome::Added);

        let again = store
            .remember(
                "editor",
                "Indentation: tabs.\nThe user said so on Monday.",
                &tags(&["user"]),
                true,
            )
            .unwrap();
        assert_eq!(again, RememberOutcome::Unchanged);

        let corrected = store
            .remember(
                "editor",
                "Indentation: tabs.\nCorrected on Tuesday: four spaces in Python files.",
                &tags(&["user", "correction"]),
                true,
            )
            .unwrap();
        assert_eq!(corrected, RememberOutcome::Updated);

        let other = store
            .remember("editor", "Line width: 100.", &tags(&["user"]), true)
            .unwrap();
        assert_eq!(other, RememberOutcome::Added);

        let entries = store.entries(true).unwrap();
        assert_eq!(entries.len(), 2, "{entries:?}");
        assert_eq!(entries[0].tags, tags(&["user", "correction"]));
        assert!(entries[0].content.contains("Corrected on Tuesday"));
        assert!(!entries[0].content.contains("Monday"));
        assert_eq!(entries[1].content, "Line width: 100.");

        let raw = fs::read_to_string(store.category_file("editor", true).unwrap()).unwrap();
        assert_eq!(
            parse_entries(&raw).len(),
            2,
            "the file round-trips through the parser: {raw:?}"
        );
    }

    #[test]
    fn search_weighs_rare_terms_above_words_every_entry_shares() {
        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        for i in 0..5 {
            store
                .remember(
                    &format!("note-{i}"),
                    &format!("Note {i}: the goose agent, the machine, the tool, the port."),
                    &[],
                    true,
                )
                .unwrap();
        }
        store
            .remember(
                "vendor",
                "The bench vendor answers on port 8850 on this machine.",
                &[],
                true,
            )
            .unwrap();

        let hits = store
            .search(
                "which port does the vendor answer on this machine tool",
                None,
            )
            .unwrap();

        assert_eq!(hits[0].entry.category, "vendor", "{hits:?}");
        assert!(
            hits[0].rare_terms >= 2,
            "vendor, answers, 8850… are rare: {:?}",
            hits[0]
        );
        let note = hits.iter().find(|h| h.entry.category == "note-0").unwrap();
        assert_eq!(
            note.rare_terms, 0,
            "port/machine/tool are in every entry and carry nothing: {note:?}"
        );
        assert!(
            note.score < hits[0].score / 2.0,
            "{} vs {}",
            note.score,
            hits[0].score
        );
        assert!(rarity_weight(6, 6) > 0.0 && rarity_weight(6, 6) < 0.1);
        assert!(
            rarity_weight(6, 0) > rarity_weight(6, 3) && rarity_weight(6, 3) > rarity_weight(6, 6)
        );
    }

    /// The VA-179 shape: a body that happens to contain every request word outscored the notes whose
    /// headlines carry two of the request's words ("How do I start a benchmark run properly?" → a note
    /// about screenshotting frontends at 8.4 over the 5-minute-tick note, "benchmark run" in its
    /// headline, at 8.2). "run" is a common word there; it still names the topic.
    #[test]
    fn search_ranks_an_entry_the_query_names_above_a_body_that_shares_its_words() {
        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        store
            .remember(
                "frontend",
                "Screenshot the rendered page before claiming done.\nOn the benchmark build, start the dev server properly before each run.",
                &tags(&["feedback"]),
                true,
            )
            .unwrap();
        store
            .remember(
                "observe",
                "During a benchmark run, tick every five minutes and read the words.\nStart from the judge.",
                &tags(&["feedback"]),
                false,
            )
            .unwrap();
        for i in 0..8 {
            let body = if i < 3 {
                format!("Note {i}: nothing in particular.\nA benchmark run may start late.")
            } else {
                format!("Note {i}: unrelated words about the weather.\nEvery run is wet.")
            };
            store
                .remember(&format!("note-{i}"), &body, &[], true)
                .unwrap();
        }

        let hits = store.search("benchmark properly run start", None).unwrap();

        let frontend = hits
            .iter()
            .find(|h| h.entry.category == "frontend")
            .unwrap();
        let observe = hits.iter().find(|h| h.entry.category == "observe").unwrap();
        assert_eq!(frontend.matched_terms, 4);
        assert_eq!(frontend.name_terms, 0);
        assert!(!frontend.named);
        assert_eq!(observe.matched_terms, 3);
        assert_eq!(observe.name_terms, 2, "{observe:?}");
        assert_eq!(
            observe.rare_terms, 2,
            "'run' is in every entry: {observe:?}"
        );
        assert!(observe.named);
        assert!(
            frontend.score > observe.score,
            "the body-only hit still outscores: {} vs {}",
            frontend.score,
            observe.score
        );
        assert_eq!(
            hits[0].entry.category, "observe",
            "the named entry ranks first regardless: {hits:?}"
        );
        assert_eq!(hits[1].entry.category, "frontend");
    }

    /// The Forge shape: two rare name terms on a HALF match ("forge" + "app" in a note about the
    /// Assets API) do not name the request; the whole-request body match keeps its rank.
    #[test]
    fn two_name_terms_on_a_half_match_do_not_name_the_query() {
        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        store
            .remember(
                "assets",
                "Whether a Forge app can read Assets is contested.\nNever depend on it.",
                &tags(&["reference"]),
                true,
            )
            .unwrap();
        store
            .remember(
                "bank",
                "Operate read-only at a bank.\nAsk the bank before any production deploy of the Forge app.",
                &tags(&["feedback"]),
                true,
            )
            .unwrap();
        for i in 0..4 {
            store
                .remember(
                    &format!("note-{i}"),
                    &format!("Note {i}: unrelated words about the weather.\nNothing else."),
                    &[],
                    true,
                )
                .unwrap();
        }

        let hits = store.search("app deploy forge production", None).unwrap();

        let assets = hits.iter().find(|h| h.entry.category == "assets").unwrap();
        assert_eq!(assets.matched_terms, 2);
        assert_eq!(assets.name_terms, 2);
        assert!(
            !assets.named,
            "two of four is not more than half: {assets:?}"
        );
        let bank = hits.iter().find(|h| h.entry.category == "bank").unwrap();
        assert_eq!(bank.matched_terms, 4);
        assert!(!bank.named);
        assert_eq!(hits[0].entry.category, "bank", "{hits:?}");

        let named = store.search("forge app", None).unwrap();
        assert_eq!(named[0].entry.category, "assets");
        assert!(named[0].named, "two of two terms in the name: {named:?}");
    }

    /// The VA-182 shapes. JQL: "search Jira issues with JQL over the Jira Cloud REST API" matched the
    /// four commonest of its seven words (api, cloud, jira, rest — two of them in the headline) in a
    /// note about classification licensing; four of seven by count is a majority, one of the four
    /// specific words (jql, issues, search, api here) is not half, so it is not named — while the
    /// note whose headline carries jql/search/issues is. Failing test: the two notes named by
    /// fix + test, one carrying "scheduler_mock tests" and the other "the exact failing invocation",
    /// are the SAME shape on every measurement the store takes.
    #[test]
    fn a_name_needs_half_of_the_request_specific_words_not_only_a_majority_of_its_words() {
        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        store
            .remember(
                "classification",
                "Data classification needs Guard Premium; plus the Cloud REST endpoints.\nJira mirror: PUT /rest/api/3/project/{key}/classification-level.",
                &tags(&["reference"]),
                true,
            )
            .unwrap();
        store
            .remember(
                "jql-sweep",
                "Search Jira issues with JQL only after proving the identity can see the project.\nA count of zero licenses nothing.",
                &tags(&["feedback"]),
                true,
            )
            .unwrap();
        for i in 0..6 {
            let platform = ["Jira", "Cloud", "REST", "API"]
                .iter()
                .take(4 - i.min(3))
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            store
                .remember(
                    &format!("client-{i}"),
                    &format!("Client {i}: a Jira site.\nIts platform: {platform}."),
                    &tags(&["project"]),
                    true,
                )
                .unwrap();
        }

        let hits = store
            .search("api cloud issues jira jql rest search", None)
            .unwrap();
        let classification = hits
            .iter()
            .find(|h| h.entry.category == "classification")
            .unwrap();
        assert_eq!(classification.matched_terms, 4, "{classification:?}");
        assert_eq!(classification.name_terms, 2);
        assert_eq!(classification.specific_terms, 4);
        assert_eq!(
            classification.matched_specific, 1,
            "four common words carry one specific word: {classification:?}"
        );
        assert!(!classification.named);
        let sweep = hits
            .iter()
            .find(|h| h.entry.category == "jql-sweep")
            .unwrap();
        assert_eq!(sweep.matched_terms, 4);
        assert_eq!(sweep.matched_specific, 3, "{sweep:?}");
        assert!(sweep.named);
        assert_eq!(hits[0].entry.category, "jql-sweep");

        let store = store_in(&tempdir().unwrap());
        store
            .remember(
                "loop",
                "How to restart the autonomous evolve-goose swarm test+fix loop.\nThen cargo test -p goose-swarm (12 scheduler_mock tests = pillar gate).",
                &tags(&["project"]),
                true,
            )
            .unwrap();
        store
            .remember(
                "sooner",
                "Never launch a run to validate a fix when a faster test exists.\nReplay the exact failing invocation first.",
                &tags(&["feedback"]),
                false,
            )
            .unwrap();
        for i in 0..4 {
            store
                .remember(
                    &format!("note-{i}"),
                    &format!("Note {i}: a fix landed.\nThe test suite is green."),
                    &tags(&["project"]),
                    true,
                )
                .unwrap();
        }
        let hits = store.search("failing fix scheduler test", None).unwrap();
        let by = |name: &str| hits.iter().find(|h| h.entry.category == name).unwrap();
        let (loop_note, sooner) = (by("loop"), by("sooner"));
        assert!(loop_note.named && sooner.named, "{hits:?}");
        assert_eq!(loop_note.matched_terms, sooner.matched_terms);
        assert_eq!(loop_note.name_terms, sooner.name_terms);
        assert_eq!(loop_note.specific_terms, 2);
        assert_eq!(loop_note.matched_specific, sooner.matched_specific);
        assert_eq!(loop_note.matched_specific, 1);
        assert!((loop_note.score - sooner.score).abs() < 1e-9);
    }

    /// The VA-180 shape: "write a blog post about local models" must not NAME a note whose headline
    /// says "goose-local … models" — the compound is one word — while "start a benchmark run" still
    /// names "During a benchmark run", and a request that writes "load-bearing" still reaches a
    /// headline's "load-bearing" through the parts.
    #[test]
    fn a_compound_in_the_headline_is_one_word_for_naming() {
        let terms = search_terms("blog post local models write");
        assert_eq!(
            name_tokens(
                "Improve the goose-local swarm workers' tool-call reliability; the weak models.",
                &terms
            ),
            vec![
                "improve",
                "the",
                "gooselocal",
                "swarm",
                "workers",
                "toolcall",
                "reliability",
                "the",
                "weak",
                "models"
            ]
        );
        let terms = search_terms("which identifiers are load-bearing");
        assert_eq!(
            name_tokens("Three load-bearing identifiers.", &terms),
            vec!["three", "load", "bearing", "loadbearing", "identifiers"]
        );
        assert_eq!(
            name_tokens(
                "settings.local.json --no-session",
                &["nosession".to_string()]
            ),
            vec!["settings", "local", "json", "no", "session", "nosession"]
        );

        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        store
            .remember(
                "toolcall",
                "STANDING: improve the goose-local swarm workers' tool-call reliability — don't blame the weak models.\nWrite deterministic repairs instead.",
                &tags(&["feedback"]),
                true,
            )
            .unwrap();
        store
            .remember(
                "observe",
                "During a benchmark run, tick every five minutes and read the words.\nStart from the judge.",
                &tags(&["feedback"]),
                false,
            )
            .unwrap();
        store
            .remember(
                "fleet",
                "The three node identifiers are load-bearing.\nNever reconfigure them.",
                &tags(&["feedback"]),
                false,
            )
            .unwrap();
        for i in 0..6 {
            store
                .remember(
                    &format!("note-{i}"),
                    &format!("Note {i}: the weather.\nA run, a post, a model of rain."),
                    &[],
                    true,
                )
                .unwrap();
        }

        let blog = store.search("blog local models post write", None).unwrap();
        let toolcall = blog
            .iter()
            .find(|h| h.entry.category == "toolcall")
            .unwrap();
        assert_eq!(toolcall.matched_terms, 3, "{toolcall:?}");
        assert_eq!(
            toolcall.name_terms, 1,
            "only 'models' is in the name: {toolcall:?}"
        );
        assert!(!toolcall.named);

        let bench = store.search("benchmark run start", None).unwrap();
        assert_eq!(bench[0].entry.category, "observe");
        assert_eq!(bench[0].name_terms, 2);
        assert!(bench[0].named);

        let fleet = store.search("identifiers load-bearing node", None).unwrap();
        assert_eq!(fleet[0].entry.category, "fleet");
        assert_eq!(
            fleet[0].name_terms, 4,
            "load and bearing reach the compound the request writes: {:?}",
            fleet[0]
        );
        assert!(fleet[0].named);
    }

    /// The topic word is the request's rarest term found somewhere in the store (a term nobody has
    /// names no topic); an entry named by one OTHER request word does not carry it. The VA-181 shapes:
    /// "connect ssh workhorse" against a cluster note named by "workhorse" with SSH in its body, and
    /// "blog … local models" against the goose-local tool-call note.
    #[test]
    fn topic_in_name_is_the_rarest_request_term_found_in_the_entry_name() {
        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        store
            .remember(
                "cluster",
                "Distributed mlx-lm inference across the MacBook and the workhorse Mac Studio.\nSSH into every host; the workhorse alias does not resolve.",
                &tags(&["project"]),
                true,
            )
            .unwrap();
        store
            .remember(
                "sshkeys",
                "SSH keys and where they live.\nThe workhorse alias uses id_ed25519_workhorse.",
                &tags(&["reference"]),
                true,
            )
            .unwrap();
        store
            .remember(
                "toolcall",
                "STANDING: improve the goose-local swarm workers' tool-call reliability — don't blame the weak models.\nWrite deterministic repairs instead.",
                &tags(&["feedback"]),
                true,
            )
            .unwrap();
        store
            .remember(
                "article",
                "Publish the blog post when the draft is read.\nWrite it locally first.",
                &tags(&["feedback"]),
                true,
            )
            .unwrap();
        for i in 0..6 {
            let body = if i < 3 {
                format!("Note {i}: connect the workhorse.\nModels of rain.")
            } else {
                format!("Note {i}: the workhorse weather.\nModels of rain.")
            };
            store
                .remember(&format!("note-{i}"), &body, &[], true)
                .unwrap();
        }

        let by = |hits: &[SearchHit], category: &str| {
            hits.iter()
                .find(|h| h.entry.category == category)
                .cloned()
                .unwrap()
        };

        let ssh = store.search("connect ssh workhorse", None).unwrap();
        let cluster = by(&ssh, "cluster");
        assert_eq!(cluster.matched_terms, 2);
        assert_eq!(cluster.name_terms, 1, "{cluster:?}");
        assert!(!cluster.named);
        assert!(
            !cluster.topic_in_name,
            "'workhorse' is the name term, 'ssh' the topic: {cluster:?}"
        );
        let keys = by(&ssh, "sshkeys");
        assert_eq!(keys.matched_terms, 2);
        assert_eq!(keys.name_terms, 1);
        assert!(keys.topic_in_name, "{keys:?}");

        let unknown = store.search("connect ssh workhorse zzzz", None).unwrap();
        assert!(
            by(&unknown, "sshkeys").topic_in_name,
            "a term no entry carries names no topic: {unknown:?}"
        );

        let blog = store.search("blog local models post write", None).unwrap();
        let toolcall = by(&blog, "toolcall");
        assert_eq!(toolcall.matched_terms, 3);
        assert_eq!(toolcall.name_terms, 1);
        assert!(!toolcall.topic_in_name, "{toolcall:?}");
        let article = by(&blog, "article");
        assert!(article.topic_in_name, "{article:?}");
    }

    #[test]
    fn terms_match_whole_words_or_longer_words_they_begin() {
        let tokens = tokenize("Research the REST API; the dockerized DB; compaction runs.");
        assert_eq!(term_occurrences("rest", &tokens), 1, "REST, not reSearch");
        assert_eq!(
            term_occurrences("search", &tokens),
            0,
            "no match inside 'research'"
        );
        assert_eq!(
            term_occurrences("docker", &tokens),
            1,
            "docker stands for dockerized"
        );
        assert_eq!(term_occurrences("dock", &tokens), 1, "four letters may");
        assert_eq!(term_occurrences("doc", &tokens), 0, "three letters do not");
        assert_eq!(term_occurrences("compact", &tokens), 1);
        assert_eq!(term_occurrences("api", &tokens), 1);

        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        store
            .remember("research", "Research articles before drafting.", &[], true)
            .unwrap();
        store
            .remember("rest", "The REST API needs a token.", &[], true)
            .unwrap();
        let hits = store.search("rest api", None).unwrap();
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].entry.category, "rest");
    }

    #[test]
    fn hyphenated_compounds_search_as_parts_and_joined() {
        assert_eq!(
            search_terms("What about hard-coded values?"),
            vec!["about", "coded", "hard", "hardcoded", "values", "what"]
        );
        let tokens = tokenize("no hardcoded times");
        assert_eq!(term_occurrences("hardcoded", &tokens), 1);
    }

    #[test]
    fn parse_entries_reads_tagged_and_untagged_entries() {
        let parsed = parse_entries("# a b\nfirst\nsecond\n\nuntagged one\n\n\n# c\nthird\n");
        assert_eq!(
            parsed,
            vec![
                (tags(&["a", "b"]), "first\nsecond".to_string()),
                (vec![], "untagged one".to_string()),
                (tags(&["c"]), "third".to_string()),
            ]
        );
    }

    #[test]
    fn invalid_categories_are_refused_and_skipped() {
        let temp_dir = tempdir().unwrap();
        let store = store_in(&temp_dir);
        for bad in ["", "*", ".", "..", "a/b", "a\\b", "a:b", "CON", "com1"] {
            assert!(
                store.category_file(bad, true).is_err(),
                "{bad:?} must be refused"
            );
        }
        fs::create_dir_all(&store.global_dir).unwrap();
        fs::write(store.global_dir.join("CON.txt"), "device\n\n").unwrap();
        fs::write(store.global_dir.join("ok.txt"), "fine\n\n").unwrap();
        let entries = store.entries(true).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].category, "ok");
    }
}
