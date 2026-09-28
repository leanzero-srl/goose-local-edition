import {
  createRecipeFromSession as acpCreateRecipeFromSession,
  deleteRecipe as acpDeleteRecipe,
  listRecipes as acpListRecipes,
  recipeToYaml as acpRecipeToYaml,
  saveRecipe as acpSaveRecipe,
  scheduleRecipe as acpScheduleRecipe,
  setRecipeSlashCommand as acpSetRecipeSlashCommand,
} from '../acp/recipe';
import { stripEmptyExtensions } from '.';
import type { Recipe, RecipeManifest } from '.';
import { getInitialWorkingDir } from '../utils/workingDir';

/** AI-author a recipe from a live session's conversation, for review/edit in the create modal. */
export const createRecipeFromSession = async (sessionId: string): Promise<Recipe> => {
  return acpCreateRecipeFromSession(sessionId);
};

export const saveRecipe = async (
  recipe: Recipe,
  recipeId?: string | null
): Promise<{ id: string; fileName: string; filePath: string }> => {
  try {
    const response = await acpSaveRecipe(stripEmptyExtensions(recipe), recipeId);
    return {
      id: response.id,
      fileName: response.file_name,
      filePath: response.file_path,
    };
  } catch (error) {
    let error_message = 'unknown error';
    if (typeof error === 'object' && error !== null && 'message' in error) {
      error_message = error.message as string;
    }
    throw new Error(error_message);
  }
};

// A failed listing reaches the caller: the Recipes view shows it instead of an empty library.
export const listSavedRecipes = async (): Promise<RecipeManifest[]> =>
  acpListRecipes(getInitialWorkingDir());

export const deleteRecipe = async (id: string): Promise<void> => {
  await acpDeleteRecipe(id);
};

// A new schedule runs in this window's folder (Q-282), where the Recipes view listed the recipe.
export const scheduleRecipe = async (id: string, cronSchedule?: string | null): Promise<void> => {
  await acpScheduleRecipe(id, getInitialWorkingDir(), cronSchedule);
};

export const setRecipeSlashCommand = async (
  id: string,
  slashCommand?: string | null
): Promise<void> => {
  await acpSetRecipeSlashCommand(id, slashCommand);
};

export const recipeToYaml = async (recipe: Recipe): Promise<string> => {
  return await acpRecipeToYaml(recipe);
};

const parseLastModified = (val: string | Date): Date => {
  return val instanceof Date ? val : new Date(val);
};

export const convertToLocaleDateString = (lastModified: string): string => {
  if (lastModified) {
    return parseLastModified(lastModified).toLocaleDateString();
  }
  return '';
};

export const getStorageDirectory = (isGlobal: boolean): string => {
  if (isGlobal) {
    const pathRoot = window.appConfig.get('GOOSE_PATH_ROOT') as string | undefined;
    if (pathRoot) {
      return `${pathRoot}/config/recipes`;
    }
    const configDir = window.appConfig.get('GOOSE_CONFIG_DIR') as string | undefined;
    if (configDir) {
      return `${configDir}/recipes`;
    }
    return '~/.config/goose/recipes';
  } else {
    // For directory recipes, build absolute path using working directory
    const workingDir = window.appConfig.get('GOOSE_WORKING_DIR') as string;
    return `${workingDir}/.goose/recipes`;
  }
};
