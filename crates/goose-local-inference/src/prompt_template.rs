use include_dir::{include_dir, Dir};
use minijinja::{Environment, Error as MiniJinjaError, Value as MJValue};
use serde::Serialize;
use std::path::Path;

use crate::paths::Paths;

static CORE_PROMPTS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/prompts");

pub fn render_string<T: Serialize>(
    template_str: &str,
    context: &T,
) -> Result<String, MiniJinjaError> {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env.add_template("template", template_str)?;
    let tmpl = env.get_template("template")?;
    let ctx = MJValue::from_serialize(context);
    let rendered = tmpl.render(ctx)?;
    Ok(rendered.trim().to_string())
}

pub fn render_template<T: Serialize>(name: &str, context: &T) -> Result<String, MiniJinjaError> {
    let user_path = Paths::config_dir().join("prompts").join(name);
    let template_str = if user_path.exists() {
        std::fs::read_to_string(&user_path).map_err(|e| {
            MiniJinjaError::new(
                minijinja::ErrorKind::InvalidOperation,
                format!("Failed to read user template: {}", e),
            )
        })?
    } else {
        let file = CORE_PROMPTS_DIR.get_file(name).ok_or_else(|| {
            MiniJinjaError::new(
                minijinja::ErrorKind::TemplateNotFound,
                format!("Built-in template '{}' not found", name),
            )
        })?;
        String::from_utf8_lossy(file.contents()).to_string()
    };

    render_string(&template_str, context)
}

/// The system prompt a small local model that emulates tools is given — shared by the MLX and
/// llama.cpp backends (two copies until Q-284). `working_dir` is the SESSION's folder: each copy
/// rendered `env::current_dir()`, goosed's cwd, which since Q-257 is $HOME for every window, so the
/// model was told to work in a folder its chat was not in.
pub(crate) fn tiny_model_prompt(working_dir: &Path) -> String {
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    };

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

    let context = serde_json::json!({
        "os": os,
        "working_directory": working_dir.display().to_string(),
        "shell": shell,
    });

    render_template("tiny_model_system.md", &context).unwrap_or_else(|e| {
        tracing::warn!("Failed to load tiny_model_system.md: {:?}", e);
        "You are Goose, an AI assistant. You can execute shell commands by starting lines with $."
            .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Q-284: the model is told the SESSION's folder, not the process cwd (goosed's, $HOME since
    // Q-257).
    #[test]
    fn the_tiny_prompt_names_the_sessions_folder_not_the_process_cwd() {
        let project = tempfile::tempdir().unwrap();
        let cwd = std::env::current_dir().unwrap();
        assert_ne!(cwd, project.path());

        let prompt = tiny_model_prompt(project.path());

        assert!(
            prompt.contains(&format!(
                "the working directory is {}",
                project.path().display()
            )),
            "{prompt}"
        );
        assert!(
            !prompt.contains(&format!("the working directory is {}\n", cwd.display())),
            "{prompt}"
        );
    }
}
