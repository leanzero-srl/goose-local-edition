use anyhow::{anyhow, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::paths::Paths;
use crate::recipe::read_recipe_file_content::{read_recipe_file, RecipeFile};
use crate::recipe::Recipe;
use crate::recipe::RECIPE_FILE_EXTENSIONS;

const GOOSE_RECIPE_PATH_ENV_VAR: &str = "GOOSE_RECIPE_PATH";

/// The user's recipe library, `<config>/recipes`.
pub fn get_recipe_library_dir() -> PathBuf {
    Paths::config_dir().join("recipes")
}

/// The folders a recipe is looked up in. `project_dir` is the folder of the chat or window asking —
/// its root, `.goose/recipes` and `.agents/recipes`; `None` means no project (the user's library,
/// `GOOSE_RECIPE_PATH` and goose's `.agents` home only). Q-265: the project folders were the
/// process cwd, and since Q-257 the desktop's one goosed runs in $HOME for every window.
fn local_recipe_dirs(project_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut local_dirs: Vec<PathBuf> = project_dir.map(Path::to_path_buf).into_iter().collect();

    if let Ok(recipe_path_env) = env::var(GOOSE_RECIPE_PATH_ENV_VAR) {
        let path_separator = if cfg!(windows) { ';' } else { ':' };
        local_dirs.extend(recipe_path_env.split(path_separator).map(PathBuf::from));
    }
    local_dirs.push(get_recipe_library_dir());
    if let Some(project) = project_dir {
        local_dirs.push(project.join(".goose/recipes"));
        // Also scan .agents/recipes/ for consistency with the .agents/ convention
        local_dirs.push(project.join(".agents/recipes"));
    }
    // goose's own `.agents` home, under GOOSE_PATH_ROOT when set (Q-197).
    local_dirs.push(Paths::in_agents_home_dir("recipes"));

    let mut dirs: Vec<PathBuf> = local_dirs
        .into_iter()
        .map(|dir| dir.canonicalize().unwrap_or(dir))
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// A recipe by path or by name, for the chat or window in `project_dir` (see `local_recipe_dirs`):
/// a relative path is the project's.
pub fn load_local_recipe_file(recipe_name: &str, project_dir: Option<&Path>) -> Result<RecipeFile> {
    if RECIPE_FILE_EXTENSIONS
        .iter()
        .any(|ext| recipe_name.ends_with(&format!(".{}", ext)))
    {
        let path = PathBuf::from(recipe_name);
        let path = match project_dir {
            Some(project) if path.is_relative() => project.join(path),
            Some(_) => path,
            None if path.is_relative() => {
                return Err(anyhow!(
                    "the recipe path {recipe_name} is relative and no project folder was given to resolve it"
                ))
            }
            None => path,
        };
        return read_recipe_file(path);
    }

    if is_file_path(recipe_name) || is_file_name(recipe_name) {
        return Err(anyhow!(
            "Recipe file {} is not a json or yaml file",
            recipe_name
        ));
    }

    let search_dirs = local_recipe_dirs(project_dir);
    for dir in &search_dirs {
        if let Ok(result) = load_recipe_file_from_dir(dir, recipe_name) {
            return Ok(result);
        }
    }

    let search_dirs_str = search_dirs
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(":");
    Err(anyhow!(
        "ℹ️  Failed to retrieve {}.yaml or {}.json in {}",
        recipe_name,
        recipe_name,
        search_dirs_str
    ))
}

pub fn list_local_recipes(project_dir: Option<&Path>) -> Result<Vec<(PathBuf, Recipe)>> {
    let mut recipes = Vec::new();
    for dir in local_recipe_dirs(project_dir) {
        if let Ok(dir_recipes) = scan_directory_for_recipes(&dir) {
            recipes.extend(dir_recipes);
        }
    }

    Ok(recipes)
}

fn is_file_path(recipe_name: &str) -> bool {
    recipe_name.contains('/')
        || recipe_name.contains('\\')
        || recipe_name.starts_with('~')
        || recipe_name.starts_with('.')
}

fn is_file_name(recipe_name: &str) -> bool {
    Path::new(recipe_name).extension().is_some()
}

fn load_recipe_file_from_dir(dir: &Path, recipe_name: &str) -> Result<RecipeFile> {
    for ext in RECIPE_FILE_EXTENSIONS {
        let recipe_path = dir.join(format!("{}.{}", recipe_name, ext));
        if let Ok(result) = read_recipe_file(recipe_path) {
            return Ok(result);
        }
    }
    Err(anyhow!(format!(
        "No {}.yaml or {}.json recipe file found in directory: {}",
        recipe_name,
        recipe_name,
        dir.display()
    )))
}

fn scan_directory_for_recipes(dir: &Path) -> Result<Vec<(PathBuf, Recipe)>> {
    let mut recipes = Vec::new();

    if !dir.exists() || !dir.is_dir() {
        return Ok(recipes);
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            if let Some(extension) = path.extension() {
                if RECIPE_FILE_EXTENSIONS.contains(&extension.to_string_lossy().as_ref()) {
                    match Recipe::from_file_path(&path) {
                        Ok(recipe) => recipes.push((path.clone(), recipe)),
                        Err(e) => {
                            let error_message = format!(
                                "Failed to load recipe from file {}: {}",
                                path.display(),
                                e
                            );
                            tracing::error!("{}", error_message);
                        }
                    }
                }
            }
        }
    }

    Ok(recipes)
}

fn generate_recipe_filename(title: &str, recipe_library_dir: &Path) -> PathBuf {
    let base_name = title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join("-");

    let filename = if base_name.is_empty() {
        "untitled-recipe".to_string()
    } else {
        base_name
    };

    let mut candidate = recipe_library_dir.join(format!("{}.yaml", filename));
    if !candidate.exists() {
        return candidate;
    }

    let mut counter = 1;
    loop {
        candidate = recipe_library_dir.join(format!("{}-{}.yaml", filename, counter));
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

pub fn save_recipe_to_file(recipe: Recipe, file_path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let recipe_library_dir = get_recipe_library_dir();

    let file_path_value = match file_path {
        Some(path) => path,
        None => generate_recipe_filename(&recipe.title, &recipe_library_dir),
    };

    if let Some(parent) = file_path_value.parent() {
        fs::create_dir_all(parent)?;
    }

    let yaml_content = recipe.to_yaml()?;
    fs::write(&file_path_value, yaml_content)?;
    Ok(file_path_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECIPE: &str =
        "title: Ship it\ndescription: The project's deploy\ninstructions: Deploy.\n";

    /// Q-265: the one goosed serves every window from $HOME (Q-257); a window's recipe list names
    /// its own project, whatever folder the process runs in.
    #[test]
    fn a_projects_recipes_are_listed_and_loaded_from_the_named_folder() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(project.join(".goose/recipes")).unwrap();
        fs::create_dir_all(project.join(".agents/recipes")).unwrap();
        fs::write(project.join(".goose/recipes/ship-it.yaml"), RECIPE).unwrap();
        fs::write(project.join(".agents/recipes/agents-one.yaml"), RECIPE).unwrap();
        let project = project.canonicalize().unwrap();
        assert_ne!(env::current_dir().unwrap().canonicalize().unwrap(), project);

        let listed: Vec<PathBuf> = list_local_recipes(Some(&project))
            .unwrap()
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert!(
            listed.contains(&project.join(".goose/recipes/ship-it.yaml")),
            "{listed:?}"
        );
        assert!(
            listed.contains(&project.join(".agents/recipes/agents-one.yaml")),
            "{listed:?}"
        );

        assert!(load_local_recipe_file("ship-it", Some(&project)).is_ok());
        assert!(load_local_recipe_file(".goose/recipes/ship-it.yaml", Some(&project)).is_ok());

        let no_project: Vec<PathBuf> = list_local_recipes(None)
            .unwrap()
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert!(!no_project.iter().any(|path| path.starts_with(&project)));
        assert!(load_local_recipe_file(".goose/recipes/ship-it.yaml", None).is_err());
    }
}
