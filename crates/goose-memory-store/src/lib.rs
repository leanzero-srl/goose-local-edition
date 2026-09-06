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
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub score: f64,
    pub matched_terms: usize,
    pub rare_terms: usize,
    pub phrase: bool,
    pub name_terms: usize,
    pub occurrences: usize,
    pub entry: MemoryEntry,
}

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

    /// Keyword search over category, tags and content, ranked by rarity-weighted score (see
    /// [`SearchHit`]): an entry ABOUT the topic — the term in its name, or several rare terms — outranks
    /// one that shares common words with the query. Ties break on category name.
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
                let name_tokens = tokenize(&name);
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

        let mut hits = Vec::new();
        for (entry, name_tokens, tokens, haystack) in corpus {
            let mut score = 0.0;
            let mut matched_terms = 0;
            let mut rare_terms = 0;
            let mut name_terms = 0;
            let mut occurrences = 0;
            for (i, term) in terms.iter().enumerate() {
                let count = term_occurrences(term, &tokens);
                if count == 0 {
                    continue;
                }
                matched_terms += 1;
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
            hits.push(SearchHit {
                score,
                matched_terms,
                rare_terms,
                phrase,
                name_terms,
                occurrences,
                entry,
            });
        }
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
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
