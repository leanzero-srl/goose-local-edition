use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignore::WalkBuilder;
use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TreeParams {
    pub path: String,
    #[serde(default = "default_depth")]
    pub depth: u32,
}

fn default_depth() -> u32 {
    2
}

/// Directories a package manager fills. Below the requested root they are shown by name only, even when no
/// .gitignore names them: a Forge benchmark run dir has none, and one `tree` at depth 3 dumped 162k chars of
/// node_modules into a model's first turn (RUN-ISSUES I-26). Asking for such a directory as the root lists it.
const DEPENDENCY_DIRS: &[&str] = &["node_modules", "bower_components", "__pycache__"];

/// Most lines one call returns; the rest is summarised so the model narrows the path or the depth.
const MAX_TREE_LINES: usize = 2000;

pub struct TreeTool;

impl TreeTool {
    pub fn new() -> Self {
        Self
    }

    pub fn tree(&self, params: TreeParams) -> CallToolResult {
        let root = PathBuf::from(&params.path);
        self.tree_at(root, params.depth)
    }

    pub fn tree_with_cwd(&self, params: TreeParams, working_dir: Option<&Path>) -> CallToolResult {
        self.tree_at(
            super::edit::resolve_path(&params.path, working_dir),
            params.depth,
        )
    }

    fn tree_at(&self, root: PathBuf, depth: u32) -> CallToolResult {
        if !root.exists() {
            return CallToolResult::error(vec![Content::text(format!(
                "Path does not exist: {}",
                root.display()
            ))
            .with_priority(0.0)]);
        }

        if !root.is_dir() {
            return CallToolResult::error(vec![Content::text(format!(
                "Path is not a directory: {}",
                root.display()
            ))
            .with_priority(0.0)]);
        }

        let max_depth = if depth == 0 {
            None
        } else {
            Some(depth as usize)
        };

        let mut tree = collect_tree(&root, max_depth);
        tree.compute_total_lines();

        let mut output = String::new();
        tree.render_into(0, &mut output);
        if output.is_empty() {
            output.push_str("(empty directory)");
        }
        let output = cap_lines(output, MAX_TREE_LINES);

        CallToolResult::success(vec![Content::text(output).with_priority(0.0)])
    }
}

impl Default for TreeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
struct DirectoryNode {
    dirs: BTreeMap<String, DirectoryNode>,
    files: BTreeMap<String, usize>,
    total_lines: usize,
    dependencies: bool,
}

impl DirectoryNode {
    fn insert_dir(&mut self, components: &[String]) {
        let mut node = self;
        for component in components {
            node = node.dirs.entry(component.clone()).or_default();
        }
    }

    fn insert_dependency_dir(&mut self, components: &[String]) {
        let mut node = self;
        for component in components {
            node = node.dirs.entry(component.clone()).or_default();
        }
        node.dependencies = true;
    }

    fn insert_file(&mut self, components: &[String], line_count: usize) {
        if components.is_empty() {
            return;
        }

        let mut node = self;
        for component in &components[..components.len() - 1] {
            node = node.dirs.entry(component.clone()).or_default();
        }

        let filename = components[components.len() - 1].clone();
        node.files.insert(filename, line_count);
    }

    fn compute_total_lines(&mut self) -> usize {
        let dir_lines: usize = self
            .dirs
            .values_mut()
            .map(DirectoryNode::compute_total_lines)
            .sum();
        let file_lines: usize = self.files.values().copied().sum();
        self.total_lines = dir_lines + file_lines;
        self.total_lines
    }

    fn render_into(&self, depth: usize, out: &mut String) {
        let indent = "  ".repeat(depth);

        for (name, dir) in &self.dirs {
            if dir.dependencies {
                out.push_str(&format!(
                    "{}{}/  (installed dependencies, not listed; call tree on it to see inside)\n",
                    indent, name
                ));
                continue;
            }
            out.push_str(&format!(
                "{}{}/  {}\n",
                indent,
                name,
                format_lines(dir.total_lines)
            ));
            dir.render_into(depth + 1, out);
        }

        for (name, line_count) in &self.files {
            out.push_str(&format!(
                "{}{}  {}\n",
                indent,
                name,
                format_lines(*line_count)
            ));
        }
    }
}

fn collect_tree(root: &Path, max_depth: Option<usize>) -> DirectoryNode {
    let mut builder = WalkBuilder::new(root);
    builder.git_ignore(true);
    builder.git_exclude(true);
    builder.git_global(true);
    builder.require_git(false);
    builder.ignore(true);
    builder.hidden(true);

    if let Some(depth) = max_depth {
        builder.max_depth(Some(depth + 1));
    }

    let skipped: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let seen = Arc::clone(&skipped);
    builder.filter_entry(move |entry| {
        let is_dependency_dir = entry.depth() > 0
            && entry.file_type().is_some_and(|t| t.is_dir())
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| DEPENDENCY_DIRS.contains(&name));
        if is_dependency_dir {
            if let Ok(mut seen) = seen.lock() {
                seen.push(entry.path().to_path_buf());
            }
        }
        !is_dependency_dir
    });

    let mut tree = DirectoryNode::default();
    for entry in builder.build().flatten() {
        let path = entry.path();
        if path == root {
            continue;
        }

        let rel = match path.strip_prefix(root) {
            Ok(rel) => rel,
            Err(_) => continue,
        };

        let components = match relative_components(rel) {
            Some(components) => components,
            None => continue,
        };

        if entry.file_type().is_some_and(|t| t.is_dir()) {
            tree.insert_dir(&components);
        } else if entry.file_type().is_some_and(|t| t.is_file()) {
            tree.insert_file(&components, count_file_lines(path));
        }
    }

    let skipped = skipped.lock().map(|s| s.clone()).unwrap_or_default();
    for path in skipped {
        if let Some(components) = path.strip_prefix(root).ok().and_then(relative_components) {
            tree.insert_dependency_dir(&components);
        }
    }

    tree
}

fn cap_lines(output: String, max_lines: usize) -> String {
    let total = output.lines().count();
    if total <= max_lines {
        return output;
    }
    let mut capped: String = output
        .lines()
        .take(max_lines)
        .flat_map(|line| [line, "\n"])
        .collect();
    capped.push_str(&format!(
        "... {} more lines not shown. Call tree on a subdirectory or with a smaller depth.\n",
        total - max_lines
    ));
    capped
}

fn relative_components(path: &Path) -> Option<Vec<String>> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => components.push(value.to_string_lossy().into_owned()),
            _ => return None,
        }
    }

    if components.is_empty() {
        None
    } else {
        Some(components)
    }
}

fn count_file_lines(path: &Path) -> usize {
    match fs::read_to_string(path) {
        Ok(content) => content.lines().count(),
        Err(_) => 0,
    }
}

fn format_lines(lines: usize) -> String {
    if lines >= 1000 {
        format!("[{}K]", lines / 1000)
    } else {
        format!("[{}]", lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::RawContent;
    use tempfile::TempDir;

    fn extract_text(result: &CallToolResult) -> &str {
        match &result.content[0].raw {
            RawContent::Text(t) => &t.text,
            _ => panic!("expected text"),
        }
    }

    fn setup_tree() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join("tests")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
        fs::write(dir.path().join("tests/test.rs"), "#[test]\nfn t() {}\n").unwrap();
        dir
    }

    #[test]
    fn tree_lists_files_and_directories() {
        let dir = setup_tree();
        let tool = TreeTool::new();

        let result = tool.tree(TreeParams {
            path: dir.path().display().to_string(),
            depth: 2,
        });

        let text = extract_text(&result);
        assert!(text.contains("src/"));
        assert!(text.contains("tests/"));
        assert!(text.contains("main.rs"));
    }

    #[test]
    fn tree_respects_depth() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
        fs::write(dir.path().join("a/b/c/deep.rs"), "fn deep() {}\n").unwrap();

        let tool = TreeTool::new();
        let result = tool.tree(TreeParams {
            path: dir.path().display().to_string(),
            depth: 1,
        });

        let text = extract_text(&result);
        assert!(text.contains("a/"));
        assert!(text.contains("b/"));
        assert!(!text.contains("deep.rs"));
    }

    #[test]
    fn tree_uses_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored/\n*.log\n").unwrap();
        fs::create_dir_all(dir.path().join("ignored")).unwrap();
        fs::write(dir.path().join("ignored/secret.rs"), "fn secret() {}\n").unwrap();
        fs::write(dir.path().join("visible.rs"), "fn visible() {}\n").unwrap();
        fs::write(dir.path().join("debug.log"), "hidden\n").unwrap();

        let tool = TreeTool::new();
        let result = tool.tree(TreeParams {
            path: dir.path().display().to_string(),
            depth: 2,
        });

        let text = extract_text(&result);
        assert!(text.contains("visible.rs"));
        assert!(!text.contains("ignored"));
        assert!(!text.contains("debug.log"));
    }

    #[test]
    fn tree_names_dependency_dirs_without_listing_them() {
        let dir = setup_tree();
        fs::create_dir_all(dir.path().join("node_modules/@atlaskit/adf-schema/dist")).unwrap();
        fs::write(
            dir.path()
                .join("node_modules/@atlaskit/adf-schema/dist/index.js"),
            "x\n",
        )
        .unwrap();
        let result = TreeTool::new().tree(TreeParams {
            path: dir.path().to_string_lossy().into_owned(),
            depth: 3,
        });
        let text = extract_text(&result);
        assert!(text.contains("main.rs"), "{text}");
        assert!(
            text.contains("node_modules/  (installed dependencies"),
            "{text}"
        );
        assert!(!text.contains("adf-schema"), "{text}");
        assert!(!text.contains("index.js"), "{text}");
    }

    #[test]
    fn tree_lists_a_dependency_dir_asked_for_as_the_root() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("node_modules/left-pad")).unwrap();
        fs::write(dir.path().join("node_modules/left-pad/index.js"), "x\n").unwrap();
        let result = TreeTool::new().tree(TreeParams {
            path: dir
                .path()
                .join("node_modules")
                .to_string_lossy()
                .into_owned(),
            depth: 2,
        });
        let text = extract_text(&result);
        assert!(text.contains("left-pad/"), "{text}");
        assert!(text.contains("index.js"), "{text}");
    }

    #[test]
    fn tree_caps_a_huge_listing() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(MAX_TREE_LINES + 50) {
            fs::write(dir.path().join(format!("f{i:05}.txt")), "").unwrap();
        }
        let result = TreeTool::new().tree(TreeParams {
            path: dir.path().to_string_lossy().into_owned(),
            depth: 1,
        });
        let text = extract_text(&result);
        assert_eq!(
            text.lines().count(),
            MAX_TREE_LINES + 1,
            "cap + one summary line"
        );
        assert!(text.ends_with(
            "50 more lines not shown. Call tree on a subdirectory or with a smaller depth.\n"
        ));
    }
}
