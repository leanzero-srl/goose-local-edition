use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ignore::WalkBuilder;
use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::Deserialize;

use super::shell::{truncate_output, truncation_notice, OUTPUT_SLOTS};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TreeParams {
    pub path: String,
    #[serde(default = "default_depth")]
    pub depth: u32,
}

fn default_depth() -> u32 {
    2
}

/// The listing is cut by the shell tool's rule (`OUTPUT_LIMIT_LINES` / `OUTPUT_LIMIT_BYTES`): the
/// whole listing goes to a file here, the result shows its two ends and says where the rest is.
/// MEASURED (Forge 2.0 run A, 2026-10-16): one `tree` result was 161,890 chars with node_modules in
/// it — 64% of the chat's context — while a shell command's output was already cut at 50,000 bytes.
pub struct TreeTool {
    output_dir: tempfile::TempDir,
    call_index: AtomicUsize,
}

impl TreeTool {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            output_dir: tempfile::tempdir()?,
            call_index: AtomicUsize::new(0),
        })
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

        let slot = self.call_index.fetch_add(1, Ordering::Relaxed) % OUTPUT_SLOTS;
        let cut = match truncate_output(&output, &format!("tree-{slot}"), self.output_dir.path()) {
            Ok(cut) => cut,
            Err(error) => {
                return CallToolResult::error(vec![Content::text(format!(
                    "The tree of {} was {} bytes, over the output limit, and could not be saved \
                     to a file to read in parts: {error}",
                    root.display(),
                    output.len()
                ))
                .with_priority(0.0)])
            }
        };
        let mut content = vec![Content::text(cut.text).with_priority(0.0)];
        if let Some(info) = &cut.truncation {
            content.push(Content::text(truncation_notice(info)).with_priority(0.0));
        }
        CallToolResult::success(content)
    }
}

#[derive(Default)]
struct DirectoryNode {
    dirs: BTreeMap<String, DirectoryNode>,
    files: BTreeMap<String, usize>,
    /// Dependency and build folders (`skills::should_skip_dir`) named here but not walked.
    not_walked: BTreeSet<String>,
    total_lines: usize,
}

impl DirectoryNode {
    fn insert_dir(&mut self, components: &[String]) {
        let mut node = self;
        for component in components {
            node = node.dirs.entry(component.clone()).or_default();
        }
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

    fn insert_not_walked(&mut self, components: &[String]) {
        let Some((name, parents)) = components.split_last() else {
            return;
        };
        let mut node = self;
        for component in parents {
            node = node.dirs.entry(component.clone()).or_default();
        }
        node.not_walked.insert(name.clone());
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
            out.push_str(&format!(
                "{}{}/  {}\n",
                indent,
                name,
                format_lines(dir.total_lines)
            ));
            dir.render_into(depth + 1, out);
        }

        for name in &self.not_walked {
            out.push_str(&format!(
                "{indent}{name}/  [dependency or build folder, not listed; tree it by its path to \
                 see inside]\n"
            ));
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

    // The folder asked for is always walked, even when it is one of these (`tree node_modules/x`).
    let not_walked: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let seen = not_walked.clone();
    builder.filter_entry(move |entry| {
        let skip = entry.depth() > 0
            && entry.file_type().is_some_and(|t| t.is_dir())
            && crate::skills::should_skip_dir(entry.path());
        if skip {
            if let Ok(mut seen) = seen.lock() {
                seen.push(entry.path().to_path_buf());
            }
        }
        !skip
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

    let not_walked = match not_walked.lock() {
        Ok(paths) => paths.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    for path in not_walked {
        if let Some(components) = path.strip_prefix(root).ok().and_then(relative_components) {
            tree.insert_not_walked(&components);
        }
    }

    tree
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
        let tool = TreeTool::new().unwrap();

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

        let tool = TreeTool::new().unwrap();
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

        let tool = TreeTool::new().unwrap();
        let result = tool.tree(TreeParams {
            path: dir.path().display().to_string(),
            depth: 2,
        });

        let text = extract_text(&result);
        assert!(text.contains("visible.rs"));
        assert!(!text.contains("ignored"));
        assert!(!text.contains("debug.log"));
    }

    fn texts(result: &CallToolResult) -> Vec<&str> {
        result
            .content
            .iter()
            .map(|c| match &c.raw {
                RawContent::Text(t) => t.text.as_str(),
                _ => panic!("expected text"),
            })
            .collect()
    }

    /// A dependency folder is named with what it is, never walked — and no .gitignore is needed for
    /// it (Forge 2.0 run A's app had none, so its node_modules filled the listing).
    #[test]
    fn a_dependency_folder_is_named_but_not_walked() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("node_modules/react/cjs")).unwrap();
        fs::write(dir.path().join("node_modules/react/index.js"), "x\n").unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/index.js"), "export {};\n").unwrap();

        let tool = TreeTool::new().unwrap();
        let result = tool.tree(TreeParams {
            path: dir.path().display().to_string(),
            depth: 0,
        });
        let text = extract_text(&result);
        assert!(text.contains("src/"), "{text}");
        assert!(text.contains("index.js"), "{text}");
        assert!(
            text.contains("node_modules/  [dependency or build folder, not listed"),
            "{text}"
        );
        assert!(!text.contains("react"), "{text}");

        let inside = tool.tree(TreeParams {
            path: dir.path().join("node_modules").display().to_string(),
            depth: 0,
        });
        let inside = extract_text(&inside);
        assert!(
            inside.contains("react/") && inside.contains("index.js"),
            "the folder asked for is walked: {inside}"
        );
    }

    /// Over the shell tool's limit the listing is cut the shell's way: its two ends, a marker for
    /// what is left out, and a notice naming the file that holds all of it.
    #[test]
    fn a_listing_over_the_shell_limit_says_it_was_cut_and_where_the_rest_is() {
        use super::super::shell::OUTPUT_LIMIT_LINES;
        let dir = tempfile::tempdir().unwrap();
        let files = OUTPUT_LIMIT_LINES + 10;
        for i in 0..files {
            fs::write(dir.path().join(format!("f{i:05}.txt")), "").unwrap();
        }

        let tool = TreeTool::new().unwrap();
        let result = tool.tree(TreeParams {
            path: dir.path().display().to_string(),
            depth: 1,
        });
        assert_ne!(result.is_error, Some(true));
        let texts = texts(&result);
        assert_eq!(texts.len(), 2, "{texts:?}");
        assert!(texts[0].starts_with("f00000.txt"), "{}", texts[0]);
        assert!(texts[0].contains("lines omitted"), "{}", texts[0]);
        assert!(
            texts[0]
                .trim_end()
                .ends_with(&format!("f{:05}.txt  [0]", files - 1)),
            "{}",
            texts[0]
        );
        let notice = texts[1];
        assert!(
            notice.contains(&format!("Output exceeded {OUTPUT_LIMIT_LINES} line limit")),
            "{notice}"
        );
        let saved = notice
            .split("Full output saved to ")
            .nth(1)
            .and_then(|rest| rest.split(". Read it").next())
            .expect("the notice names the file");
        let whole = fs::read_to_string(saved).unwrap();
        assert_eq!(whole.lines().count(), files);
    }

    #[test]
    fn a_listing_under_the_limit_is_whole_and_carries_no_notice() {
        let dir = setup_tree();
        let result = TreeTool::new().unwrap().tree(TreeParams {
            path: dir.path().display().to_string(),
            depth: 2,
        });
        assert_eq!(texts(&result).len(), 1);
    }
}
