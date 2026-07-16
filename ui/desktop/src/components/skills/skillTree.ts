/**
 * Turn a skill's flat `supporting_files` list into the folder structure it actually is on disk.
 *
 * The backend hands back absolute paths in one flat array (`skills/mod.rs` walks the dir and pushes every
 * non-SKILL.md file), so a skill with 950 files across `scripts/browser/`, `references/` and `state/drafts/`
 * arrives as 950 unrelated strings. Rendering that flat is how a real folder becomes a wall of text — the
 * thing this view exists to stop being.
 */
export interface TreeNode {
  name: string;
  /** Path relative to the skill directory. '' for the root. */
  path: string;
  /** Absolute path — what a reader needs to actually open the file. Directories have none. */
  abs?: string;
  children?: TreeNode[];
  /** Total files at or below this node, so a collapsed folder still says how much it hides. */
  fileCount: number;
}

/**
 * Build the tree. `skillDir` is the entry's own path, used to make the file paths relative.
 *
 * Robust to a supporting file that does not sit under skillDir (a symlink, or a backend that changes its
 * mind): those keep their full path rather than being dropped, because silently losing a file from a view
 * whose whole job is "show me every file" is the failure this is meant to prevent.
 */
export function buildSkillTree(skillDir: string, files: string[]): TreeNode {
  const root: TreeNode = { name: '', path: '', children: [], fileCount: 0 };
  const prefix = skillDir.endsWith('/') ? skillDir : `${skillDir}/`;

  for (const abs of [...files].sort()) {
    const rel = abs.startsWith(prefix) ? abs.slice(prefix.length) : abs;
    const parts = rel.split('/').filter(Boolean);
    if (parts.length === 0) continue;
    let node = root;
    parts.forEach((part, i) => {
      const isFile = i === parts.length - 1;
      node.fileCount += 1;
      if (isFile) {
        (node.children ??= []).push({
          name: part,
          path: parts.slice(0, i + 1).join('/'),
          abs,
          fileCount: 1,
        });
        return;
      }
      const dirPath = parts.slice(0, i + 1).join('/');
      let next = node.children?.find((c) => c.path === dirPath && c.children);
      if (!next) {
        next = { name: part, path: dirPath, children: [], fileCount: 0 };
        (node.children ??= []).push(next);
      }
      node = next;
    });
  }
  sortTree(root);
  return root;
}

/** Directories first, then files, each alphabetical — the order every file explorer uses. */
function sortTree(node: TreeNode): void {
  if (!node.children) return;
  node.children.sort((a, b) => {
    const ad = a.children ? 0 : 1;
    const bd = b.children ? 0 : 1;
    return ad !== bd ? ad - bd : a.name.localeCompare(b.name);
  });
  node.children.forEach(sortTree);
}

/**
 * Which folders to open on first render: the top level only, and only when the tree is small enough that
 * expanding it does not itself become the wall of text. A 950-file skill opens collapsed.
 */
export function defaultExpanded(root: TreeNode, maxAutoExpandFiles = 25): Set<string> {
  const open = new Set<string>();
  if (root.fileCount <= maxAutoExpandFiles) {
    const walk = (n: TreeNode) => {
      if (n.children) {
        if (n.path) open.add(n.path);
        n.children.forEach(walk);
      }
    };
    walk(root);
  }
  return open;
}
