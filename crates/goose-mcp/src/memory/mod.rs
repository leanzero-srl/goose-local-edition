use etcetera::{choose_app_strategy, AppStrategy};
use goose_memory_store::{scope_label, search_terms, MemoryStore, RememberOutcome};
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

fn extract_working_dir_from_meta(meta: &Meta) -> Option<PathBuf> {
    meta.0
        .get(WORKING_DIR_HEADER)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
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
    /// specific sentence; details go on the following lines.
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
             - a CORRECTION the user makes ("no, actually…", "don't do X", "always Y") — capture the rule
               and the reason behind it
             - a stable PROJECT or ENVIRONMENT fact (paths, hosts, where credentials live, build/run/test
               commands, naming conventions)
             - a recurring COMMAND or workflow you had to figure out and would want again next time
             Choose a fitting category + tags, and the right scope (local for project-specific, global for
             user-wide). Write the data so its FIRST LINE is one specific sentence — that line is the
             headline the index shows — and put the kind first among the tags: user, feedback, project or
             reference. Saving data whose first line matches an existing memory's headline UPDATES that
             memory in place, so restate the headline when you correct a fact. Do NOT store secrets/tokens
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

        let mut memory_router = Self {
            tool_router: Self::tool_router(),
            instructions: String::new(),
            global_memory_dir,
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

        let mut file = fs::File::open(&memory_file_path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;

        let memories: Vec<&str> = content.split("\n\n").collect();
        let new_content: Vec<String> = memories
            .into_iter()
            .filter(|entry| !entry.contains(memory_content))
            .map(|s| s.to_string())
            .collect();

        fs::write(memory_file_path, new_content.join("\n\n"))?;

        Ok(())
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
                       headline the index shows: one specific sentence. Re-saving with the same headline \
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
        description = "Search your long-term memory by keywords and get the matching entries IN FULL, best \
                       match first (category, tags and content are all searched). Use it when a line of \
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

        let terms = search_terms(&params.query);
        if terms.is_empty() {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "query must contain at least one word".to_string(),
                None,
            ));
        }
        let hits = self
            .search(&params.query, params.is_global, working_dir.as_ref())
            .map_err(memory_error)?;
        if hits.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No memory matched \"{}\". The memory index in your instructions lists every saved entry \
                 by category; retrieve_memories(category, is_global) loads one in full.",
                params.query
            ))]));
        }

        let limit = params.limit.unwrap_or(SEARCH_DEFAULT_LIMIT).max(1);
        let total = hits.len();
        tracing::info!(query = %params.query, hits = total, "memories searched");
        let mut out = format!(
            "{} of {} matching memories for \"{}\":\n",
            total.min(limit),
            total,
            params.query
        );
        for hit in hits.into_iter().take(limit) {
            let phrase = if hit.phrase { ", exact phrase" } else { "" };
            out.push_str(&format!(
                "\n{}/{} terms ({} rare){phrase}, score {:.2} — ",
                hit.matched_terms,
                terms.len(),
                hit.rare_terms,
                hit.score
            ));
            out.push_str(&hit.entry.render());
        }
        if total > limit {
            out.push_str(&format!(
                "\n{} more matched; narrow the query or raise limit.\n",
                total - limit
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(out)]))
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

    #[test]
    fn test_lazy_directory_creation() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("test_memory");
        let working_dir = memory_base.join("working");

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: memory_base.join("global"),
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

    #[test]
    fn remember_reports_added_updated_and_unchanged() {
        let temp_dir = tempdir().unwrap();
        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: temp_dir.path().join("global"),
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
