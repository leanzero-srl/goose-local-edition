use etcetera::{choose_app_strategy, AppStrategy};
use goose_memory_store::{
    scope_label, search_covering, search_terms, MemoryStore, RememberOutcome, STOPWORDS,
};
use indoc::formatdoc;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, ErrorCode, ErrorData, Implementation, InitializeResult, Meta,
        ServerCapabilities, ServerInfo,
    },
    schemars::JsonSchema,
    service::RequestContext,
    tool, tool_handler, tool_router, RoleServer, ServerHandler,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{self, Read},
    path::PathBuf,
};

const WORKING_DIR_HEADER: &str = "agent-working-dir";
const SESSION_ID_HEADER: &str = "agent-session-id";

fn extract_working_dir_from_meta(meta: &Meta) -> Option<PathBuf> {
    meta.0
        .get(WORKING_DIR_HEADER)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn extract_session_id_from_meta(meta: &Meta) -> Option<String> {
    meta.0
        .get(SESSION_ID_HEADER)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn memory_error(error: io::Error) -> ErrorData {
    let code = if error.kind() == io::ErrorKind::InvalidInput {
        ErrorCode::INVALID_PARAMS
    } else {
        ErrorCode::INTERNAL_ERROR
    };
    ErrorData::new(code, error.to_string(), None)
}

// ratio: ten full entries is about the size of the whole index, so one search never outweighs it.
const SEARCH_DEFAULT_LIMIT: usize = 10;

/// Parameters for the remember_memory tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RememberMemoryParams {
    /// The category to store the memory in
    pub category: String,
    /// The data to remember. Its FIRST LINE is the headline shown in the memory index — make it one
    /// specific sentence; details go on the following lines. Only what the user said or what you
    /// verified: no reason the user did not give.
    pub data: String,
    /// Tags; put the kind first: user, feedback, project or reference
    #[serde(default)]
    pub tags: Vec<String>,
    /// true = user-wide (global); false or omitted = this project only (local)
    #[serde(default)]
    pub is_global: bool,
}

/// Parameters for the retrieve_memories tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RetrieveMemoriesParams {
    /// The category to retrieve memories from (use "*" for all)
    pub category: String,
    /// true = global only, false = project-local only; omit to read both scopes
    #[serde(default)]
    pub is_global: Option<bool>,
}

/// Parameters for the remove_memory_category tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RemoveMemoryCategoryParams {
    /// The category to remove (use "*" for all)
    pub category: String,
    /// Whether to remove from global or local storage
    pub is_global: bool,
}

/// Parameters for the remove_specific_memory tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RemoveSpecificMemoryParams {
    /// The category containing the memory
    pub category: String,
    /// The content of the memory to remove
    pub memory_content: String,
    /// Whether to remove from global or local storage
    pub is_global: bool,
}

/// Parameters for the propose_knowledge tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProposeKnowledgeParams {
    /// The category (a short kebab-case topic) the knowledge piece belongs to
    pub category: String,
    /// The knowledge piece. First line = one specific statement (the headline); then the detail.
    pub data: String,
    /// REQUIRED and non-empty: what GROUNDS this — the lookups you made (tool names, URLs, file
    /// paths). A piece with no source is your own reasoning and is not knowledge; use remember_memory
    /// for a preference or a project fact instead.
    pub sources: Vec<String>,
    /// Tags after the kind; `reference` is added first automatically
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether to store globally (user-wide) or project-local
    #[serde(default)]
    pub is_global: bool,
    /// Optional: one sentence on why this is worth keeping for next time, shown on the card as
    /// "Why:". Omit it rather than restate the sources.
    #[serde(default)]
    pub why: Option<String>,
}

/// Parameters for the search_memories tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchMemoriesParams {
    /// Words to look for, any order; matched against category, tags and content. Use several related
    /// terms and synonyms.
    pub query: String,
    /// true = global memories only, false = project-local only; omit to search both
    #[serde(default)]
    pub is_global: Option<bool>,
    /// Maximum entries to return (default 10)
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Memory MCP Server using official RMCP SDK
#[derive(Clone)]
pub struct MemoryServer {
    tool_router: ToolRouter<Self>,
    instructions: String,
    global_memory_dir: PathBuf,
    /// Where `propose_knowledge` files a PROPOSAL instead of writing an entry — the owner's
    /// `memory_proposals` (default ON, frame 1.14). None = write directly, as `remember_memory` does.
    proposals_dir: Option<PathBuf>,
}

impl Default for MemoryServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router(router = tool_router)]
impl MemoryServer {
    pub fn new() -> Self {
        let global_memory_dir = choose_app_strategy(crate::APP_STRATEGY.clone())
            .map(|strategy| strategy.in_config_dir("memory"))
            .unwrap_or_else(|_| PathBuf::from(".config/goose/memory"));
        Self::with_global_dir(global_memory_dir)
    }

    /// The same server with `propose_knowledge` filing proposals (the owner's default) or writing
    /// entries directly. The caller reads the config; this process reads none.
    pub fn with_proposals(memory_proposals: bool) -> Self {
        let mut server = Self::new();
        if !memory_proposals {
            server.proposals_dir = None;
        }
        server
    }

    /// Build the server over a given global directory. The project-local directory is resolved from the
    /// process working directory, which the extension manager sets to the session's working dir when it
    /// spawns this server, so the startup index covers both scopes.
    pub fn with_global_dir(global_memory_dir: PathBuf) -> Self {
        let instructions = formatdoc! {r#"
             This extension stores and retrieves categorized information with tagging support — it is YOUR
             long-term memory. Write to it PROACTIVELY, on your own initiative and WITHOUT asking permission
             first, whenever you learn something worth remembering across sessions.

             Storage:
             - Local: .goose/memory/ (project-specific)
             - Global: ~/.config/goose/memory/ (user-wide)

             CALL remember_memory (do NOT ask the user first) the moment you learn any of these:
             - a durable USER PREFERENCE or taste (how they like things done; tools, styles, or conventions
               they favor or reject)
             - a CORRECTION the user makes ("no, actually…", "don't do X", "always Y") — capture the rule,
               and its reason only when the user gave one
             - a stable PROJECT or ENVIRONMENT fact (paths, hosts, where credentials live, build/run/test
               commands, naming conventions)
             - a recurring COMMAND or workflow you had to figure out and would want again next time
             Choose a fitting category + tags, and the right scope (local for project-specific, global for
             user-wide). Write the data so its FIRST LINE is one specific sentence — that line is the
             headline the index shows — and put the kind first among the tags: user, feedback, project or
             reference. Saving data whose first line matches an existing memory's headline UPDATES that
             memory in place, so restate the headline when you correct a fact. Store what the user SAID: a
             reason, a context or a who-asked-for-it the user did not state is your guess, and a saved guess
             is read back next session as the user's word — leave it out. Do NOT store secrets/tokens
             verbatim, transient chatter, or anything already obvious from the code or repo.

             HOW TO READ IT: below is the INDEX of every saved memory — one line per entry, in the form
             `category [tags]: headline`. Only the headlines are loaded here, never the bodies. When a line
             looks relevant to the task, call retrieve_memories(category, is_global) to read that memory in
             full. When the task touches a topic and you are not sure which entry covers it, call
             search_memories(query) — it returns the best-matching entries in full. Search BEFORE
             remember_memory so a correction lands on the existing memory instead of beside it. Do not bring
             memories up unless they are relevant.

             Use category "*" with retrieve_memories or remove_memory_category to access all entries.
            "#};

        let proposals_dir = global_memory_dir
            .parent()
            .map(|config| config.join("proposals"));
        let mut memory_router = Self {
            tool_router: Self::tool_router(),
            instructions: String::new(),
            global_memory_dir,
            proposals_dir,
        };

        let mut updated_instructions = instructions;
        updated_instructions.push_str("\n\nMemory index:\n");
        updated_instructions.push_str(&memory_router.index(None));
        memory_router.set_instructions(updated_instructions);

        memory_router
    }

    // Add a setter method for instructions
    pub fn set_instructions(&mut self, new_instructions: String) {
        self.instructions = new_instructions;
    }

    pub fn get_instructions(&self) -> &str {
        &self.instructions
    }

    fn store(&self, working_dir: Option<&PathBuf>) -> MemoryStore {
        let working_dir = working_dir
            .cloned()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        MemoryStore::new(self.global_memory_dir.clone(), &working_dir)
    }

    fn get_memory_file(
        &self,
        category: &str,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<PathBuf> {
        self.store(working_dir).category_file(category, is_global)
    }

    pub fn entries(
        &self,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<Vec<goose_memory_store::MemoryEntry>> {
        self.store(working_dir).entries(is_global)
    }

    pub fn index(&self, working_dir: Option<&PathBuf>) -> String {
        self.store(working_dir).index()
    }

    pub fn search(
        &self,
        query: &str,
        is_global: Option<bool>,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<Vec<goose_memory_store::SearchHit>> {
        self.store(working_dir).search(query, is_global)
    }

    pub fn retrieve_all(
        &self,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<HashMap<String, Vec<String>>> {
        let base_dir = self.store(working_dir).scope_dir(is_global).to_path_buf();
        let mut memories = HashMap::new();
        if base_dir.exists() {
            for entry in fs::read_dir(&base_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    let file_name = entry.file_name();
                    let Some(category) = file_name
                        .to_str()
                        .and_then(|name| name.strip_suffix(".txt"))
                    else {
                        continue;
                    };
                    if self
                        .get_memory_file(category, is_global, working_dir)
                        .is_err()
                    {
                        continue;
                    }
                    let category_memories = self.retrieve(category, is_global, working_dir)?;
                    memories.insert(
                        category.to_string(),
                        category_memories.into_values().flatten().collect(),
                    );
                }
            }
        }
        Ok(memories)
    }

    pub fn remember(
        &self,
        _context: &str,
        category: &str,
        data: &str,
        tags: &[&str],
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<RememberOutcome> {
        let tags: Vec<String> = tags.iter().map(|tag| tag.to_string()).collect();
        self.store(working_dir)
            .remember(category, data, &tags, is_global)
    }

    pub fn retrieve(
        &self,
        category: &str,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<HashMap<String, Vec<String>>> {
        let memory_file_path = self.get_memory_file(category, is_global, working_dir)?;
        if !memory_file_path.exists() {
            return Ok(HashMap::new());
        }

        let mut file = fs::File::open(memory_file_path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;

        let mut memories = HashMap::new();
        for entry in content.split("\n\n") {
            let mut lines = entry.lines();
            if let Some(first_line) = lines.next() {
                if let Some(stripped) = first_line.strip_prefix('#') {
                    let tags = stripped
                        .split_whitespace()
                        .map(String::from)
                        .collect::<Vec<_>>();
                    memories.insert(tags.join(" "), lines.map(String::from).collect());
                } else {
                    let entry_data: Vec<String> = std::iter::once(first_line.to_string())
                        .chain(lines.map(String::from))
                        .collect();
                    memories
                        .entry("untagged".to_string())
                        .or_insert_with(Vec::new)
                        .extend(entry_data);
                }
            }
        }

        Ok(memories)
    }

    pub fn remove_specific_memory_internal(
        &self,
        category: &str,
        memory_content: &str,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<()> {
        let memory_file_path = self.get_memory_file(category, is_global, working_dir)?;
        if !memory_file_path.exists() {
            return Ok(());
        }

        goose_memory_store::update_file_locked(&memory_file_path, |content| {
            let Some(content) = content else {
                return Ok(((), None));
            };
            let kept: Vec<&str> = content
                .split("\n\n")
                .filter(|entry| !entry.contains(memory_content))
                .collect();
            Ok(((), Some(kept.join("\n\n"))))
        })
    }

    pub fn clear_memory(
        &self,
        category: &str,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<()> {
        let memory_file_path = self.get_memory_file(category, is_global, working_dir)?;
        if memory_file_path.exists() {
            fs::remove_file(memory_file_path)?;
        }

        Ok(())
    }

    pub fn clear_all_global_or_local_memories(
        &self,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<()> {
        let base_dir = self.store(working_dir).scope_dir(is_global).to_path_buf();
        if base_dir.exists() {
            fs::remove_dir_all(&base_dir)?;
        }
        Ok(())
    }

    /// Stores a memory with optional tags in a specified category
    #[tool(
        name = "remember_memory",
        description = "Save something worth remembering across sessions to your long-term memory. Call this \
                       PROACTIVELY (without asking first) the moment you learn a durable user preference, a \
                       correction the user made, a stable project/environment fact (paths, hosts, build/run \
                       commands, conventions), or a recurring command/workflow. The data's first line is the \
                       headline the index shows: one specific sentence. Store the user's own rule; add a \
                       reason only when the user gave one, never one you inferred. Re-saving with the same headline \
                       updates that memory. Pick a category, tags (kind first: user/feedback/project/reference) \
                       and scope. Do not store secrets verbatim or transient chatter."
    )]
    pub async fn remember_memory(
        &self,
        params: Parameters<RememberMemoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);

        if params.data.is_empty() {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "Data must not be empty when remembering a memory".to_string(),
                None,
            ));
        }

        let tags: Vec<&str> = params.tags.iter().map(|s| s.as_str()).collect();
        let outcome = self
            .remember(
                "context",
                &params.category,
                &params.data,
                &tags,
                params.is_global,
                working_dir.as_ref(),
            )
            .map_err(memory_error)?;

        let scope = scope_label(params.is_global);
        let message = match outcome {
            RememberOutcome::Added => format!(
                "Stored a new {scope} memory in category \"{}\"; it joins the index next session.",
                params.category
            ),
            RememberOutcome::Updated => format!(
                "Updated the existing {scope} memory in category \"{}\" whose headline matched.",
                params.category
            ),
            RememberOutcome::Unchanged => format!(
                "Already remembered in category \"{}\" ({scope}) — nothing changed.",
                params.category
            ),
        };
        tracing::info!(category = %params.category, scope, ?outcome, "memory remembered");
        Ok(CallToolResult::success(vec![Content::text(message)]))
    }

    /// FRAME 1.14, event A on the session path: knowledge that RESEARCH creates. The instructions
    /// above tell the model to write memories "WITHOUT asking permission first" — that paragraph
    /// is load-bearing (it is why the store has hundreds of entries in the measured recall work)
    /// and is deliberately left alone. This is a DIFFERENT event: a piece grounded in a lookup,
    /// which under `memory_proposals` becomes a proposal the human answers, never a silent write.
    #[tool(
        name = "propose_knowledge",
        description = "File a KNOWLEDGE PIECE you learned by RESEARCHING — a fact you looked up (web search, \
                       library docs, a document, a file you read) that would be worth having next time. \
                       Unlike remember_memory this REQUIRES `sources`: the lookups that ground it. Call it \
                       after a successful lookup, not for your own reasoning. When memory proposals are on \
                       (the default) it is PROPOSED to the user, who decides whether to keep it; otherwise it \
                       is stored as a `reference` entry with its sources. The data's first line is the \
                       headline: one specific statement."
    )]
    pub async fn propose_knowledge(
        &self,
        params: Parameters<ProposeKnowledgeParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let working_dir = extract_working_dir_from_meta(&context.meta);
        let session_id = extract_session_id_from_meta(&context.meta);
        let message = self.propose_knowledge_inner(params.0, working_dir, session_id)?;
        Ok(CallToolResult::success(vec![Content::text(message)]))
    }

    fn propose_knowledge_inner(
        &self,
        params: ProposeKnowledgeParams,
        working_dir: Option<PathBuf>,
        session_id: Option<String>,
    ) -> Result<String, ErrorData> {
        if params.data.trim().is_empty() {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "Data must not be empty when proposing knowledge".to_string(),
                None,
            ));
        }
        let sources: Vec<String> = params
            .sources
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if sources.is_empty() {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "A knowledge piece needs at least one source — the lookup that grounds it. Your own \
                 reasoning is not knowledge; use remember_memory for a preference or a project fact."
                    .to_string(),
                None,
            ));
        }
        let mut tags = vec!["reference".to_string()];
        tags.extend(params.tags.iter().filter(|t| *t != "reference").cloned());
        let sources_line = format!("\nSources: {}", sources.join(", "));
        let content = format!("{}{sources_line}", params.data.trim());

        if let Some(dir) = &self.proposals_dir {
            // A card holds PROPOSAL_TEXT_MAX_CHARS; the store refuses a longer proposal rather than
            // cut it mid-word (Q-93). Say here what the model can change: its own text.
            let limit = goose_memory_store::proposals::PROPOSAL_TEXT_MAX_CHARS;
            let length = content.chars().count();
            if length > limit {
                let room = limit.saturating_sub(sources_line.chars().count());
                return Err(ErrorData::new(
                    ErrorCode::INVALID_PARAMS,
                    format!(
                        "Not proposed: with its Sources line this knowledge piece is {length} \
                         characters and a proposal card holds {limit}. Shorten `data` to at most \
                         {room} characters (it is {} now), keeping the one fact worth keeping, and \
                         call propose_knowledge again.",
                        params.data.trim().chars().count()
                    ),
                    None,
                ));
            }
            // The chat that asked owns the card: goose sends its session id with every tool call.
            // Keyed by the working dir, one chat's proposal showed at the bottom of every other
            // chat in the project (E2E #2, Q-82). The dir key stays for a caller with no session.
            let key = session_id.unwrap_or_else(|| {
                goose_memory_store::working_dir_key(
                    working_dir
                        .as_deref()
                        .unwrap_or_else(|| std::path::Path::new(".")),
                )
            });
            let outcome = goose_memory_store::ProposalStore::new(dir.clone())
                .add(
                    &key,
                    goose_memory_store::ProposalKind::Knowledge,
                    None,
                    &content,
                    params.why.as_deref().unwrap_or_default(),
                    &params.category,
                    &tags,
                    params.is_global,
                    &sources,
                )
                .map_err(memory_error)?;
            let message = match outcome {
                goose_memory_store::ProposeOutcome::Added => format!(
                    "Proposed as knowledge in category \"{}\" — the user decides whether to keep it; \
                     nothing is stored until they save it.",
                    params.category
                ),
                goose_memory_store::ProposeOutcome::Duplicate => {
                    "Already proposed — not raised again.".to_string()
                }
                goose_memory_store::ProposeOutcome::Refused => {
                    "Not proposed: three proposals are already waiting for the user's answer."
                        .to_string()
                }
            };
            tracing::info!(category = %params.category, ?outcome, "knowledge proposed");
            return Ok(message);
        }

        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        let outcome = self
            .remember(
                "context",
                &params.category,
                &content,
                &tag_refs,
                params.is_global,
                working_dir.as_ref(),
            )
            .map_err(memory_error)?;
        let scope = scope_label(params.is_global);
        tracing::info!(category = %params.category, scope, ?outcome, "knowledge stored");
        Ok(format!(
            "Stored a {scope} reference entry in category \"{}\" ({outcome:?}) with its sources.",
            params.category
        ))
    }

    /// Retrieves all memories from a specified category
    #[tool(
        name = "retrieve_memories",
        description = "Retrieves all memories from a specified category"
    )]
    pub async fn retrieve_memories(
        &self,
        params: Parameters<RetrieveMemoriesParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);

        let scopes: Vec<bool> = match params.is_global {
            Some(scope) => vec![scope],
            None => vec![true, false],
        };
        let mut entries = Vec::new();
        for is_global in scopes {
            entries.extend(
                self.entries(is_global, working_dir.as_ref())
                    .map_err(memory_error)?,
            );
        }
        if params.category != "*" {
            goose_memory_store::validate_category(&params.category).map_err(memory_error)?;
            entries.retain(|entry| entry.category == params.category);
        }

        let scope = match params.is_global {
            Some(is_global) => scope_label(is_global),
            None => "global or local",
        };
        if entries.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No {scope} memories saved in category \"{}\".",
                params.category
            ))]));
        }
        let mut out = format!(
            "{} memories in category \"{}\":\n",
            entries.len(),
            params.category
        );
        for entry in &entries {
            out.push('\n');
            out.push_str(&entry.render());
        }
        tracing::info!(category = %params.category, count = entries.len(), "memories retrieved");

        Ok(CallToolResult::success(vec![Content::text(out)]))
    }

    /// Searches memories by keywords and returns the matching entries in full
    #[tool(
        name = "search_memories",
        description = "Search your long-term memory by keywords: the entries ABOUT them come IN FULL, best \
                       match first (category, tags and content are all searched); entries that only share a \
                       word come as one headline line each, and when none is about them it says so. Use it when a line of \
                       the memory index looks relevant, when the user refers to something from an earlier \
                       session, and BEFORE remember_memory so you update an existing memory instead of \
                       duplicating it."
    )]
    pub async fn search_memories(
        &self,
        params: Parameters<SearchMemoriesParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);
        let report = self.search_report(
            &params.query,
            params.is_global,
            params.limit,
            working_dir.as_ref(),
        )?;
        Ok(CallToolResult::success(vec![Content::text(report)]))
    }

    /// What `search_memories` answers. The entries the query is ABOUT
    /// (`goose_memory_store::search_covering` — recall's law, plus an entry whose name carries two
    /// of the query's words, plus every hit of a one-word query) come in full; an entry that only
    /// shares a word with it comes as one headline line, so the model can still open it but does
    /// not read its body as an answer; when nothing covers the query it says so first. Function
    /// words and one-letter tokens are dropped from the query, as recall drops them.
    ///
    /// Why (Q-98, E2E #2 turn 1): "ISO dates British spelling client deliverables Node zero
    /// dependencies scripts" — two rules the user had just stated, neither saved yet — answered
    /// "10 of 32 matching memories", in full, topped by `jira-mentions-indexed-by-accountid` (4/10:
    /// client, node, scripts, zero — "finds ZERO mentions", "a mention node"), then an article
    /// playbook (3/10) and a production-config rule (1/10); recall had put none of them in the turn.
    fn search_report(
        &self,
        query: &str,
        is_global: Option<bool>,
        limit: Option<usize>,
        working_dir: Option<&PathBuf>,
    ) -> Result<String, ErrorData> {
        let terms: Vec<String> = search_terms(query)
            .into_iter()
            .filter(|t| t.chars().count() > 1 && !STOPWORDS.contains(&t.as_str()))
            .collect();
        if terms.is_empty() {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "query must contain at least one word that is not a function word".to_string(),
                None,
            ));
        }
        let hits = self
            .search(&terms.join(" "), is_global, working_dir)
            .map_err(memory_error)?;
        if hits.is_empty() {
            return Ok(format!(
                "No memory matched \"{query}\". The memory index in your instructions lists every saved entry \
                 by category; retrieve_memories(category, is_global) loads one in full."
            ));
        }

        let limit = limit.unwrap_or(SEARCH_DEFAULT_LIMIT).max(1);
        let covers = search_covering(&hits, terms.len());
        let (about, sharing): (Vec<_>, Vec<_>) = hits
            .into_iter()
            .zip(covers)
            .partition(|(_, covers)| *covers);
        tracing::info!(
            query,
            covering = about.len(),
            sharing = sharing.len(),
            "memories searched"
        );

        let mut out = if about.is_empty() {
            format!(
                "No memory covers \"{query}\": {} share a word with it, and none is named by it, \
                 says its words together, or carries its rarest word in its name.\n",
                sharing.len()
            )
        } else {
            format!(
                "{} of {} memories covering \"{query}\":\n",
                about.len().min(limit),
                about.len()
            )
        };
        for (hit, _) in about.iter().take(limit) {
            let phrase = if hit.phrase { ", exact phrase" } else { "" };
            out.push_str(&format!(
                "\n{}/{} terms ({} rare, {} in name){phrase}, score {:.2} — ",
                hit.matched_terms,
                terms.len(),
                hit.rare_terms,
                hit.name_terms,
                hit.score
            ));
            out.push_str(&hit.entry.render());
        }
        if about.len() > limit {
            out.push_str(&format!(
                "\n{} more cover it; narrow the query or raise limit.\n",
                about.len() - limit
            ));
        }
        if !sharing.is_empty() {
            out.push_str(&format!(
                "\n{} {} share a word with the query without being about it — headlines only; \
                 retrieve_memories(category, is_global) reads one in full:\n",
                sharing.len(),
                if sharing.len() == 1 {
                    "entry"
                } else {
                    "entries"
                }
            ));
            for (hit, _) in sharing.iter().take(limit) {
                out.push_str(&format!(
                    "- {} ({}, {}/{} terms): {}\n",
                    hit.entry.category,
                    hit.entry.scope_label(),
                    hit.matched_terms,
                    terms.len(),
                    hit.entry.headline()
                ));
            }
            if sharing.len() > limit {
                out.push_str(&format!("- … and {} more\n", sharing.len() - limit));
            }
        }
        Ok(out)
    }

    /// Removes all memories within a specified category
    #[tool(
        name = "remove_memory_category",
        description = "Removes all memories within a specified category"
    )]
    pub async fn remove_memory_category(
        &self,
        params: Parameters<RemoveMemoryCategoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);

        let message = if params.category == "*" {
            self.clear_all_global_or_local_memories(params.is_global, working_dir.as_ref())
                .map_err(memory_error)?;
            format!(
                "Cleared all memory {} categories",
                if params.is_global { "global" } else { "local" }
            )
        } else {
            self.clear_memory(&params.category, params.is_global, working_dir.as_ref())
                .map_err(memory_error)?;
            format!("Cleared memories in category: {}", params.category)
        };

        Ok(CallToolResult::success(vec![Content::text(message)]))
    }

    /// Removes a specific memory within a specified category
    #[tool(
        name = "remove_specific_memory",
        description = "Removes a specific memory within a specified category"
    )]
    pub async fn remove_specific_memory(
        &self,
        params: Parameters<RemoveSpecificMemoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);

        self.remove_specific_memory_internal(
            &params.category,
            &params.memory_content,
            params.is_global,
            working_dir.as_ref(),
        )
        .map_err(memory_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Removed specific memory from category: {}",
            params.category
        ))]))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MemoryServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "goose-memory",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(self.instructions.clone())
    }
}

// Remove the old MemoryArgs struct since we're using the new parameter structs

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn server(dir: &std::path::Path, proposals: bool) -> MemoryServer {
        MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: dir.join("memory"),
            proposals_dir: proposals.then(|| dir.join("proposals")),
        }
    }

    fn call(
        server: &MemoryServer,
        working_dir: &std::path::Path,
        sources: Vec<&str>,
    ) -> Result<String, ErrorData> {
        call_in(server, working_dir, None, sources)
    }

    fn call_in(
        server: &MemoryServer,
        working_dir: &std::path::Path,
        session_id: Option<&str>,
        sources: Vec<&str>,
    ) -> Result<String, ErrorData> {
        let params = ProposeKnowledgeParams {
            category: "vendor-api".to_string(),
            data: "The vendor API returns 409 on a conflict.\nBody is the error envelope."
                .to_string(),
            sources: sources.into_iter().map(String::from).collect(),
            tags: vec!["api".to_string()],
            is_global: false,
            why: None,
        };
        server.propose_knowledge_inner(
            params,
            Some(working_dir.to_path_buf()),
            session_id.map(str::to_string),
        )
    }

    /// Q-93: E2E #1's end-of-life piece (513 characters of data plus its Sources line, 645 in all)
    /// was cut at character 350 — the card and the saved memory ended `by exception only". Bi` —
    /// and every card said "Why: grounded by a lookup this turn" whatever had happened. Now the
    /// piece is refused whole with the room the model has, and the why is the model's own or none.
    #[test]
    fn a_knowledge_piece_longer_than_the_card_is_refused_whole_and_the_why_is_never_a_stock_line() {
        let dir = tempdir().unwrap();
        let wd = dir.path().join("project");
        let on = server(dir.path(), true);
        let data = "Atlassian Data Center End of Life timeline (official): EOL = 28 Mar 2029 23:59 PST (all DC licences expire, products go read-only, support ends); end of sale to NEW customers = 30 Mar 2026; end of sale to EXISTING customers = 30 Mar 2028. Support + critical security fixes continue through 28 Mar 2029. Extensions past EOL are \"by exception only\". Bitbucket DC and Jira Align DC are EXCLUDED from EOL. Sources: atlassian.com/licensing/data-center-end-of-life and atlassian.com/blog/announcements/atlassian-ascend.";
        let sources = vec![
            "https://www.atlassian.com/licensing/data-center-end-of-life".to_string(),
            "https://www.atlassian.com/blog/announcements/atlassian-ascend".to_string(),
        ];
        let params = |data: &str, why: Option<&str>| ProposeKnowledgeParams {
            category: "atlassian-migration".to_string(),
            data: data.to_string(),
            sources: sources.clone(),
            tags: vec!["atlassian".to_string()],
            is_global: false,
            why: why.map(str::to_string),
        };
        let err = on
            .propose_knowledge_inner(params(data, None), Some(wd.clone()), Some("s1".into()))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(
            err.message
                .contains("645 characters and a proposal card holds 350")
                && err
                    .message
                    .contains("at most 218 characters (it is 513 now)"),
            "{}",
            err.message
        );
        let store = goose_memory_store::ProposalStore::new(dir.path().join("proposals"));
        assert!(store.list("s1").unwrap().is_empty(), "no stump was filed");

        let short = "Atlassian Data Center end of life is 28 Mar 2029; end of sale to new customers 30 Mar 2026, to existing 30 Mar 2028.";
        on.propose_knowledge_inner(params(short, None), Some(wd.clone()), Some("s1".into()))
            .unwrap();
        on.propose_knowledge_inner(
            params(
                "Bitbucket DC and Jira Align DC are excluded from the EOL.",
                Some("the client runs Bitbucket DC"),
            ),
            Some(wd),
            Some("s1".into()),
        )
        .unwrap();
        let rows = store.list("s1").unwrap();
        assert!(rows[0].text.starts_with(short) && rows[0].text.ends_with("atlassian-ascend"));
        assert_eq!(rows[0].why, "", "no stock reason where the model gave none");
        assert_eq!(rows[1].why, "the client runs Bitbucket DC");
    }

    /// Q-82: a piece proposed in one chat is that chat's card — filed under the session goose
    /// names on the call, never under the working dir every chat of the project lists.
    #[test]
    fn propose_knowledge_files_under_the_asking_chat() {
        let dir = tempdir().unwrap();
        let wd = dir.path().join("project");
        let on = server(dir.path(), true);
        let reply = call_in(&on, &wd, Some("20260925_31"), vec!["web-search__search"]).unwrap();
        assert!(reply.contains("Proposed as knowledge"), "{reply}");
        let store = goose_memory_store::ProposalStore::new(dir.path().join("proposals"));
        assert_eq!(store.list("20260925_31").unwrap().len(), 1);
        let wd_key = goose_memory_store::working_dir_key(&wd);
        assert!(store.list(&wd_key).unwrap().is_empty());
    }

    #[test]
    fn the_session_id_is_read_from_the_call_meta() {
        let mut meta = Meta::default();
        assert_eq!(extract_session_id_from_meta(&meta), None);
        meta.0.insert(
            SESSION_ID_HEADER.to_string(),
            serde_json::Value::String("20260925_31".to_string()),
        );
        assert_eq!(
            extract_session_id_from_meta(&meta).as_deref(),
            Some("20260925_31")
        );
    }

    /// FRAME 1.14 G2: with proposals on (the default) a grounded piece is PROPOSED — nothing in
    /// the memory store — keyed by the working dir; with them off it is a `reference` entry
    /// carrying its sources; and a piece with no source is refused as not-knowledge.
    #[test]
    fn propose_knowledge_proposes_by_default_stores_when_off_and_needs_a_source() {
        let dir = tempdir().unwrap();
        let wd = dir.path().join("project");
        let on = server(dir.path(), true);
        let reply = call(&on, &wd, vec!["web-search__search"]).unwrap();
        assert!(reply.contains("Proposed as knowledge"), "{reply}");
        assert!(!wd.join(".goose/memory").exists());
        let key = goose_memory_store::working_dir_key(&wd);
        let rows = goose_memory_store::ProposalStore::new(dir.path().join("proposals"))
            .list(&key)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, goose_memory_store::ProposalKind::Knowledge);
        assert!(
            rows[0].text.ends_with("Sources: web-search__search"),
            "{}",
            rows[0].text
        );
        assert_eq!(
            rows[0].tags,
            vec!["reference".to_string(), "api".to_string()]
        );

        let off = server(dir.path(), false);
        let reply = call(&off, &wd, vec!["context7__get-library-docs"]).unwrap();
        assert!(reply.contains("Stored a local reference entry"), "{reply}");
        let text = std::fs::read_to_string(wd.join(".goose/memory/vendor-api.txt")).unwrap();
        assert!(
            text.starts_with("# reference api\nThe vendor API returns 409"),
            "{text}"
        );
        assert!(text.contains("Sources: context7__get-library-docs"));

        let err = call(&on, &wd, vec!["  "]).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn test_lazy_directory_creation() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("test_memory");
        let working_dir = memory_base.join("working");

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: memory_base.join("global"),
            proposals_dir: None,
        };

        let local_memory_dir = working_dir.join(".goose").join("memory");

        assert!(!router.global_memory_dir.exists());
        assert!(!local_memory_dir.exists());

        router
            .remember(
                "test_context",
                "test_category",
                "test_data",
                &["tag1"],
                false,
                Some(&working_dir),
            )
            .unwrap();

        assert!(local_memory_dir.exists());
        assert!(!router.global_memory_dir.exists());

        router
            .remember(
                "test_context",
                "global_category",
                "global_data",
                &["global_tag"],
                true,
                None,
            )
            .unwrap();

        assert!(router.global_memory_dir.exists());
    }

    #[test]
    fn test_clear_nonexistent_directories() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("nonexistent_memory");
        let working_dir = memory_base.join("working");

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: memory_base.join("global"),
            proposals_dir: None,
        };

        assert!(router
            .clear_all_global_or_local_memories(false, Some(&working_dir))
            .is_ok());
        assert!(router
            .clear_all_global_or_local_memories(true, None)
            .is_ok());
    }

    #[test]
    fn test_remember_retrieve_clear_workflow() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("workflow_test");
        let working_dir = memory_base.join("working");

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: memory_base.join("global"),
            proposals_dir: None,
        };

        router
            .remember(
                "context",
                "test_category",
                "test_data_content",
                &["test_tag"],
                false,
                Some(&working_dir),
            )
            .unwrap();

        let memories = router
            .retrieve("test_category", false, Some(&working_dir))
            .unwrap();
        assert!(!memories.is_empty());

        let has_content = memories.values().any(|v| {
            v.iter()
                .any(|content| content.contains("test_data_content"))
        });
        assert!(has_content);

        router
            .clear_memory("test_category", false, Some(&working_dir))
            .unwrap();

        let memories_after_clear = router
            .retrieve("test_category", false, Some(&working_dir))
            .unwrap();
        assert!(memories_after_clear.is_empty());
    }

    #[test]
    fn test_directory_creation_on_write() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("write_test");
        let working_dir = memory_base.join("working");

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: memory_base.join("global"),
            proposals_dir: None,
        };

        let local_memory_dir = working_dir.join(".goose").join("memory");
        assert!(!local_memory_dir.exists());

        router
            .remember(
                "context",
                "category",
                "data",
                &[],
                false,
                Some(&working_dir),
            )
            .unwrap();

        assert!(local_memory_dir.exists());
        assert!(local_memory_dir.join("category.txt").exists());
    }

    #[test]
    fn test_remove_specific_memory() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("remove_test");
        let working_dir = memory_base.join("working");

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: memory_base.join("global"),
            proposals_dir: None,
        };

        router
            .remember(
                "context",
                "category",
                "keep_this",
                &[],
                false,
                Some(&working_dir),
            )
            .unwrap();
        router
            .remember(
                "context",
                "category",
                "remove_this",
                &[],
                false,
                Some(&working_dir),
            )
            .unwrap();

        let memories = router
            .retrieve("category", false, Some(&working_dir))
            .unwrap();
        assert_eq!(memories.len(), 1);

        router
            .remove_specific_memory_internal("category", "remove_this", false, Some(&working_dir))
            .unwrap();

        let memories_after = router
            .retrieve("category", false, Some(&working_dir))
            .unwrap();
        let has_removed = memories_after
            .values()
            .any(|v| v.iter().any(|content| content.contains("remove_this")));
        assert!(!has_removed);

        let has_kept = memories_after
            .values()
            .any(|v| v.iter().any(|content| content.contains("keep_this")));
        assert!(has_kept);
    }

    #[test]
    fn test_memory_operations_reject_escape_capable_categories() {
        let temp_dir = tempdir().unwrap();
        let working_dir = temp_dir.path().join("working");
        let outside_file = temp_dir.path().join("outside.txt");
        fs::write(&outside_file, "secret").unwrap();

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: temp_dir.path().join("global"),
            proposals_dir: None,
        };

        for category in [
            "",
            "*",
            ".",
            "..",
            "../../../outside",
            "/tmp/outside",
            r"..\..\outside",
            "C:outside",
            r"C:\outside",
            "NUL",
            "con",
            "AUX.log",
            "COM1",
            "lpt9",
        ] {
            assert_eq!(
                router
                    .remember(
                        "context",
                        category,
                        "malicious",
                        &[],
                        false,
                        Some(&working_dir)
                    )
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(
                router
                    .retrieve(category, false, Some(&working_dir))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(
                router
                    .remove_specific_memory_internal(category, "secret", false, Some(&working_dir),)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(
                router
                    .clear_memory(category, false, Some(&working_dir))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }

        assert_eq!(fs::read_to_string(outside_file).unwrap(), "secret");
        assert!(!working_dir.join(".goose").exists());
    }

    #[test]
    fn test_memory_category_allows_safe_filename_characters() {
        let temp_dir = tempdir().unwrap();
        let working_dir = temp_dir.path().join("working");
        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: temp_dir.path().join("global"),
            proposals_dir: None,
        };

        router
            .remember(
                "context",
                "project notes_2026",
                "safe",
                &[],
                false,
                Some(&working_dir),
            )
            .unwrap();

        assert!(working_dir
            .join(".goose/memory/project notes_2026.txt")
            .is_file());
    }

    #[cfg(unix)]
    #[test]
    fn test_retrieve_all_skips_invalid_legacy_categories() {
        let temp_dir = tempdir().unwrap();
        let working_dir = temp_dir.path().join("working");
        let memory_dir = working_dir.join(".goose/memory");
        fs::create_dir_all(&memory_dir).unwrap();
        fs::write(memory_dir.join("valid.txt"), "kept").unwrap();
        fs::write(memory_dir.join("work:api.txt"), "legacy").unwrap();
        fs::write(memory_dir.join(r"work\api.txt"), "legacy").unwrap();

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: temp_dir.path().join("global"),
            proposals_dir: None,
        };

        let memories = router.retrieve_all(false, Some(&working_dir)).unwrap();

        assert_eq!(memories.len(), 1);
        assert!(memories["valid"].iter().any(|entry| entry == "kept"));
    }

    #[test]
    fn test_memory_error_preserves_invalid_parameter_distinction() {
        let invalid = memory_error(io::Error::new(io::ErrorKind::InvalidInput, "bad category"));
        assert_eq!(invalid.code, ErrorCode::INVALID_PARAMS);

        let filesystem = memory_error(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        assert_eq!(filesystem.code, ErrorCode::INTERNAL_ERROR);
    }

    #[test]
    fn instructions_carry_the_index_and_not_the_bodies() {
        let temp_dir = tempdir().unwrap();
        let global = temp_dir.path().join("global");
        fs::create_dir_all(&global).unwrap();
        fs::write(
            global.join("note-25a42f.txt"),
            "# feedback imported:claude-code\nA green counter proved nothing while two thirds were broken.\nOpen the WORST case, never a convenient one.\n\n",
        )
        .unwrap();

        let server = MemoryServer::with_global_dir(global);
        let instructions = server.get_instructions();

        assert!(instructions.contains("Memory index:"));
        assert!(instructions.contains(
            "- note-25a42f [feedback imported:claude-code]: A green counter proved nothing while two thirds were broken."
        ));
        assert!(
            !instructions.contains("Open the WORST case"),
            "{instructions}"
        );
        assert!(instructions.contains("search_memories(query)"));
    }

    /// Q-92: E2E #2b saved "British spelling is the client's own convention" and "the receiving team
    /// often has restricted tooling" — reasons the user never gave — because the instructions said
    /// "capture the rule and the reason behind it". The words now ask for the user's rule and a
    /// reason only when the user gave one, in the instructions and the tool description alike.
    #[test]
    fn the_memory_words_never_ask_for_a_reason_the_user_did_not_give() {
        let temp_dir = tempdir().unwrap();
        let server = MemoryServer::with_global_dir(temp_dir.path().join("global"));
        let instructions = server.get_instructions();
        assert!(
            !instructions.contains("the reason behind it"),
            "{instructions}"
        );
        assert!(instructions.contains("its reason only when the user gave one"));
        assert!(
            instructions.contains("a saved guess\nis read back next session as the user's word")
        );
        let description = MemoryServer::tool_router()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "remember_memory")
            .and_then(|tool| tool.description)
            .unwrap();
        assert!(
            description
                .contains("add a reason only when the user gave one, never one you inferred"),
            "{description}"
        );
    }

    /// Q-98: E2E #2 searched "ISO dates British spelling client deliverables Node zero dependencies
    /// scripts" before saving two rules nobody had saved yet, and read "10 of 32 matching memories"
    /// in full, topped by `jira-mentions-indexed-by-accountid` (client, node, scripts, zero). The
    /// entries here carry the matched words where the real ones do.
    #[test]
    fn search_says_none_covers_a_query_and_lists_word_sharers_by_headline_only() {
        const QUERY: &str =
            "ISO dates British spelling client deliverables Node zero dependencies scripts";
        let temp_dir = tempdir().unwrap();
        let wd = temp_dir.path().join("project");
        let server = MemoryServer::with_global_dir(temp_dir.path().join("global"));
        let save = |category: &str, data: &str, tags: &[&str]| {
            server
                .remember("context", category, data, tags, true, Some(&wd))
                .unwrap();
        };
        save(
            "jira-mentions-indexed-by-accountid",
            "Jira indexes @mentions by accountId, NOT display name — a display-name search finds ZERO mentions.\nA mention node renders the accountId. The client's mention sweep in scripts/sweep.py returns 0 and looks like an empty queue.",
            &["reference", "imported:claude-code"],
        );
        save(
            "article-authoring-unlocked",
            "Community article authoring is live via the Contributors group.\nThe weekly flag resets on ISO-week rollover; the discover script is scripts/discover.mjs. Every article carries ZERO app references.",
            &["project"],
        );
        save(
            "ask-before-client-prod-config",
            "Ask the CLIENT before any config change on their production system.\nSandbox: go ahead. Production: ask, wait for their yes, then act.",
            &["feedback"],
        );
        for (category, data) in [
            ("vendor-port", "The vendor API listens on 8850."),
            ("fmt-first", "Run cargo fmt before every commit."),
            (
                "tick-cadence",
                "Tick every five minutes during a benchmark run.",
            ),
            ("workhorse", "The workhorse is a Mac Studio on the LAN."),
            ("reaping", "Kill pids, never a process group."),
            ("plans", "A plan opens with the phases before and after."),
            (
                "keyring",
                "Secrets come from secrets.yaml; the keyring is off.",
            ),
        ] {
            save(category, data, &["project"]);
        }

        let report = server.search_report(QUERY, None, None, Some(&wd)).unwrap();
        assert!(
            report.starts_with(&format!(
                "No memory covers \"{QUERY}\": 3 share a word with it"
            )),
            "{report}"
        );
        assert!(
            report.contains("- jira-mentions-indexed-by-accountid (global, 4/10 terms): Jira indexes @mentions by accountId"),
            "{report}"
        );
        assert!(
            !report.contains("mention sweep"),
            "a body that only shares words is not read as an answer: {report}"
        );

        save(
            "client-deliverables",
            "Client-facing output uses ISO dates (YYYY-MM-DD) and British spelling.\nApplies to reports, PDFs and every client deliverable; internal notes are exempt.",
            &["user", "preference", "iso-dates", "british-spelling"],
        );
        save(
            "handover-scripts",
            "Handover scripts are always plain Node with zero npm dependencies.\nNode stdlib only: no package installs, no node_modules.",
            &["user", "preference", "node", "zero-dependencies", "scripts"],
        );
        let report = server.search_report(QUERY, None, None, Some(&wd)).unwrap();
        assert!(
            report.starts_with(&format!("2 of 2 memories covering \"{QUERY}\":")),
            "{report}"
        );
        let dates = report.find("## client-deliverables").unwrap();
        let scripts = report.find("## handover-scripts").unwrap();
        let sharers = report.find("share a word with the query").unwrap();
        assert!(dates < sharers && scripts < sharers, "{report}");
        assert!(
            report.contains("Node stdlib only"),
            "both rules come in full"
        );
        assert!(!report.contains("mention sweep"), "{report}");

        let one_word = server
            .search_report("postgres", None, None, Some(&wd))
            .unwrap();
        assert!(one_word.starts_with("No memory matched"), "{one_word}");
        let one_word = server
            .search_report("the scripts", None, None, Some(&wd))
            .unwrap();
        assert!(
            one_word.starts_with("3 of 3 memories covering"),
            "a one-word query (function words out) is covered by every entry with the word: {one_word}"
        );
    }

    #[test]
    fn remember_reports_added_updated_and_unchanged() {
        let temp_dir = tempdir().unwrap();
        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: temp_dir.path().join("global"),
            proposals_dir: None,
        };
        let first = router
            .remember(
                "ctx",
                "editor",
                "Indentation: tabs.\nSaid on Monday.",
                &["user"],
                true,
                None,
            )
            .unwrap();
        assert_eq!(first, RememberOutcome::Added);
        let same = router
            .remember(
                "ctx",
                "editor",
                "Indentation: tabs.\nSaid on Monday.",
                &["user"],
                true,
                None,
            )
            .unwrap();
        assert_eq!(same, RememberOutcome::Unchanged);
        let corrected = router
            .remember(
                "ctx",
                "editor",
                "Indentation: tabs.\nExcept Python: four spaces.",
                &["user"],
                true,
                None,
            )
            .unwrap();
        assert_eq!(corrected, RememberOutcome::Updated);
        let entries = router.entries(true, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].content.contains("four spaces"));
    }
}
