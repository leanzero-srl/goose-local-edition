import { useState, useEffect, useMemo, useCallback } from 'react';
import { BookOpen, Pencil, Sparkles, Trash2, Zap } from 'lucide-react';
import { errorMessage } from '../../utils/conversionUtils';
import { getInitialWorkingDir } from '../../utils/workingDir';
import { defineMessages, useIntl } from '../../i18n';
import { listSkillSources } from '../../acp/sources';
import type { SourceEntry } from '@aaif/goose-sdk';
import { SkillDetail } from './SkillDetail';
import { TreeContextMenu } from '../Layout/tree';
import { useStartChatAbout } from '../Layout/useStartChatAbout';
import {
  isEditable,
  PERSONA_USER_MARKER,
  readError,
  skillOrigin,
  type SkillOrigin,
} from './skillKinds';
import { Button, EmptyState, TYPE, cx } from '../lz';
import { LibraryGroup, LibraryRow, LibraryShell, shownSelection } from '../library/Library';

const i18n = defineMessages({
  errorLoadingSkills: {
    id: 'skillsView.errorLoadingSkills',
    defaultMessage: 'Error Loading Skills',
  },
  tryAgain: {
    id: 'skillsView.tryAgain',
    defaultMessage: 'Try Again',
  },
  noSkillsInstalled: {
    id: 'skillsView.noSkillsInstalled',
    defaultMessage: 'No skills installed',
  },
  noSkillsDescription: {
    id: 'skillsView.noSkillsDescription',
    defaultMessage:
      'Skills are loaded from SKILL.md files in ~/.config/agents/skills/, .goose/skills/, or other supported directories.',
  },
  noMatchingSkills: {
    id: 'skillsView.noMatchingSkills',
    defaultMessage: 'No matching skills found',
  },
  adjustSearchTerms: {
    id: 'skillsView.adjustSearchTerms',
    defaultMessage: 'Try adjusting your search terms',
  },
  skillsTitle: {
    id: 'skillsView.skillsTitle',
    defaultMessage: 'Skills',
  },
  skillsSubtitle: {
    id: 'skillsView.subtitle',
    defaultMessage:
      'Installed skills that extend what goose can do — yours, this project’s, and the lessons goose wrote.',
  },
  searchSkillsPlaceholder: {
    id: 'skillsView.searchSkillsPlaceholder',
    defaultMessage: 'Search skills...',
  },
  searchSkillsLabel: {
    id: 'skillsView.searchSkillsLabel',
    defaultMessage: 'Search skills by name or description',
  },
});

/**
 * A row in the explorer.
 *
 * `SkillEntry` used to be `{name, description}` — the view fetched the full `SourceEntry`, which carries the
 * whole SKILL.md body on the wire (`content: body`, skills/mod.rs:354), and threw everything but two strings
 * away. Nothing needed a new backend call to make skills readable; the content was already here.
 */
type SkillEntry = SourceEntry;

/** How many of a skill's files the prompt names before it counts the rest — the same bound goose's
 *  own load_skill manifest uses (MAX_LISTED_SUPPORTING_FILES, skills/mod.rs), so the chat is never
 *  handed a longer list than loading the skill would print. */
const PROMPT_LISTED_FILES = 60;

const ASK_FIRST =
  'Ask me what I want changed before you write anything, then make the edit and show me the result.';

/**
 * What is asked of the model when a skill is opened as a chat about it. Every sentence is a fact the
 * app holds for THIS entry — where its SKILL.md is, which other files goose's scan found in its folder
 * (or that it found none), its scope — never an assumed structure. The old text told the model
 * "sibling files in the same folder are its references" and called the folder "the file"; on a folder
 * holding only SKILL.md a local model answered, with no tool call, that it had read "four sibling
 * reference files" and cited two invented ones (UX audit T2, 2026-09-23).
 */
export function askAboutSkillPrompt(skill: SkillEntry, projectDir: string): string {
  const head = `I want to work on my goose skill "${skill.name}" (${skill.description}).`;
  const project = projectDir.replace(/\/+$/, '');
  const forkRoots = `a global skill goes in ~/.agents/skills/<new-name>/SKILL.md, a project skill in ${project || '<project>'}/.agents/skills/<new-name>/SKILL.md`;
  const origin = skillOrigin(skill);
  if (origin === 'builtin') {
    return [
      head,
      `It is built into goose (${skill.path}): there is no file on disk to read or edit. load_skill with the name "${skill.name}" shows its instructions.`,
      `It cannot be changed in place. To make your own version, fork it as a new skill with YAML frontmatter (name, description) followed by the instructions — ${forkRoots}.`,
      ASK_FIRST,
    ].join('\n');
  }

  const dir = skill.path.replace(/\/+$/, '');
  const skillMd = `${dir}/SKILL.md`;
  const files = (skill.supportingFiles ?? [])
    .map((abs) => (abs.startsWith(`${dir}/`) ? abs.slice(dir.length + 1) : abs))
    .sort();
  const listed = files.slice(0, PROMPT_LISTED_FILES);
  const unlisted = files.length - listed.length;
  const folder =
    files.length === 0
      ? `goose's scan of its folder ${dir} found no other files (dependency folders such as node_modules are never listed): SKILL.md is all there is.`
      : `goose's scan of its folder ${dir} found ${files.length} other file${files.length === 1 ? '' : 's'}: ${listed.join(', ')}${unlisted > 0 ? `, and ${unlisted} more not named here` : ''}. Open a file before you say anything about what it holds.`;
  const scope =
    origin === 'persona'
      ? `goose wrote this skill about itself: after each successful build of its stack the swarm rewrites everything above the "${PERSONA_USER_MARKER}" heading and keeps what is under it, so a lasting change goes under that heading.`
      : origin === 'global'
        ? 'It is a global skill: goose sees it in every project.'
        : `It is a project skill: goose sees it only when working in ${project || 'its project'}.`;
  return [
    head,
    `Its instructions are the file ${skillMd} — YAML frontmatter (name, description) followed by the instructions goose follows when the skill is loaded. ${folder}`,
    scope,
    `Read ${skillMd} first with the developer tools. You can edit it in place, or fork it as a new skill with its own frontmatter — ${forkRoots}.`,
    ASK_FIRST,
  ].join('\n');
}

function SkillItem({
  skill,
  selected,
  onSelect,
  onEdit,
  onDelete,
  onAsk,
}: {
  skill: SkillEntry;
  selected: boolean;
  onSelect: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onAsk: () => void;
}) {
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const editable = isEditable(skill);
  return (
    <div>
      <LibraryRow
        testId="skill-row"
        title={skill.name}
        preview={skill.description.slice(0, 400)}
        selected={selected}
        onSelect={onSelect}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      />
      {menu && (
        <TreeContextMenu
          x={menu.x}
          y={menu.y}
          testId="skill-context-menu"
          onClose={() => setMenu(null)}
          items={[
            {
              key: 'open',
              label: 'Open',
              icon: <BookOpen />,
              onClick: () => {
                setMenu(null);
                onSelect();
              },
            },
            {
              key: 'edit',
              label: 'Edit',
              icon: <Pencil />,
              disabled: !editable,
              title: editable ? undefined : 'Built-in skills ship with goose and cannot be edited',
              onClick: () => {
                setMenu(null);
                onEdit();
              },
            },
            {
              key: 'ask',
              label: 'Start an AI session about this skill',
              icon: <Sparkles />,
              onClick: () => {
                setMenu(null);
                onAsk();
              },
            },
            {
              key: 'delete',
              label: 'Delete',
              icon: <Trash2 />,
              danger: true,
              separator: true,
              disabled: !editable,
              title: editable ? undefined : 'Built-in skills ship with goose and cannot be deleted',
              onClick: () => {
                setMenu(null);
                onDelete();
              },
            },
          ]}
        />
      )}
    </div>
  );
}

export default function SkillsView() {
  const intl = useIntl();
  const [skills, setSkills] = useState<SkillEntry[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchTerm, setSearchTerm] = useState('');
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [editRequest, setEditRequest] = useState(0);
  const [deleteRequest, setDeleteRequest] = useState(0);
  const startChat = useStartChatAbout();

  const filteredSkills = useMemo(() => {
    const q = searchTerm.trim().toLowerCase();
    if (!q) return skills;
    return skills.filter(
      (skill) => skill.name.toLowerCase().includes(q) || skill.description.toLowerCase().includes(q)
    );
  }, [skills, searchTerm]);

  // Grouped by where it came from — the roots are the folders, and which root a skill lives in is the thing
  // that decides whether it is yours, goose's, or shipped. Personas lead: a lesson goose wrote about itself
  // is the one the user most needs to notice appearing.
  const groups = useMemo(() => {
    const order: SkillOrigin[] = ['persona', 'project', 'global', 'builtin'];
    const titles: Record<SkillOrigin, string> = {
      persona: 'Learned by goose',
      project: 'This project',
      global: 'Yours',
      builtin: 'Built in',
    };
    return order
      .map((origin) => ({
        origin,
        title: titles[origin],
        items: filteredSkills.filter((s) => !readError(s) && skillOrigin(s) === origin),
      }))
      .filter((g) => g.items.length > 0);
  }, [filteredSkills]);

  // Files goose found and could not read lead the list: each is a skill the user wrote that the model
  // cannot see, and this row is the one place that says so.
  const unreadable = useMemo(() => filteredSkills.filter((s) => readError(s)), [filteredSkills]);

  const visible = useMemo(
    () => [...unreadable, ...groups.flatMap((g) => g.items)],
    [unreadable, groups]
  );
  const selected = shownSelection(visible, selectedPath, (s) => s.path);

  const loadSkills = useCallback(async () => {
    try {
      setError(null);
      setSkills(await listSkillSources(getInitialWorkingDir()));
    } catch (err) {
      setError(errorMessage(err, 'Failed to load skills'));
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    loadSkills();
  }, [loadSkills]);

  // The swarm rewrites a persona's SKILL.md from another process whenever a build of that stack succeeds, so
  // a list fetched once on mount goes stale on its own — and a run takes hours, which is exactly how long
  // this window tends to sit open. Re-read on focus so the lesson on screen is the lesson on disk.
  useEffect(() => {
    const onFocus = () => loadSkills();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [loadSkills]);

  const renderList = () => {
    if (!loaded) {
      return <p className={cx(TYPE.meta, 'px-2 py-2')}>Reading skills…</p>;
    }
    if (error) {
      return (
        <div className="flex flex-col items-start gap-2 px-2 py-2">
          <p role="alert" className="text-lz-body text-lz-err">
            {intl.formatMessage(i18n.errorLoadingSkills)}: {error}
          </p>
          <Button size="sm" variant="secondary" onClick={loadSkills}>
            {intl.formatMessage(i18n.tryAgain)}
          </Button>
        </div>
      );
    }
    if (skills.length === 0) {
      return (
        <p className={cx(TYPE.bodyMuted, 'px-2 py-2')}>
          {intl.formatMessage(i18n.noSkillsDescription)}
        </p>
      );
    }
    if (visible.length === 0) {
      return (
        <div className="px-2 py-2">
          <p className={TYPE.body}>{intl.formatMessage(i18n.noMatchingSkills)}</p>
          <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.adjustSearchTerms)}</p>
        </div>
      );
    }
    const unreadableGroup =
      unreadable.length > 0 ? (
        <LibraryGroup key="unreadable" title="Couldn't read" count={unreadable.length}>
          {unreadable.map((skill) => (
            <LibraryRow
              key={skill.path}
              testId="skill-unreadable-row"
              title={skill.name}
              label="not loaded"
              preview={skill.description}
              selected={skill.path === selected?.path}
              onSelect={() => setSelectedPath(skill.path)}
            />
          ))}
        </LibraryGroup>
      ) : null;
    return [unreadableGroup].concat(groups.map((group) => (
      <LibraryGroup key={group.origin} title={group.title} count={group.items.length}>
        {group.items.map((skill) => (
          <SkillItem
            key={skill.path}
            skill={skill}
            selected={skill.path === selected?.path}
            onSelect={() => setSelectedPath(skill.path)}
            onEdit={() => {
              setSelectedPath(skill.path);
              setEditRequest((n) => n + 1);
            }}
            onDelete={() => {
              setSelectedPath(skill.path);
              setDeleteRequest((n) => n + 1);
            }}
            onAsk={() => void startChat(askAboutSkillPrompt(skill, getInitialWorkingDir()))}
          />
        ))}
      </LibraryGroup>
    )));
  };

  return (
    <LibraryShell
      testId="skills-view"
      title={intl.formatMessage(i18n.skillsTitle)}
      subtitle={intl.formatMessage(i18n.skillsSubtitle)}
      search={{
        value: searchTerm,
        onChange: setSearchTerm,
        placeholder: intl.formatMessage(i18n.searchSkillsPlaceholder),
        label: intl.formatMessage(i18n.searchSkillsLabel),
      }}
      list={renderList()}
      detail={
        selected && readError(selected) ? (
          <div className="h-full min-h-0 px-lz-page pt-4" data-testid="skill-unreadable-detail">
            <p className={TYPE.body}>{selected.description}</p>
            <p className={cx(TYPE.bodyMuted, 'mt-2')}>
              goose does not offer this skill to the model until the file can be read. Fix the
              frontmatter at the top of {selected.path}/SKILL.md and this page picks it up on the next
              look.
            </p>
          </div>
        ) : selected ? (
          <div className="h-full min-h-0 px-lz-page pt-4">
            <SkillDetail
              entry={selected}
              origin={skillOrigin(selected)}
              projectDir={getInitialWorkingDir()}
              requestEdit={editRequest}
              requestDelete={deleteRequest}
              onAsk={() => void startChat(askAboutSkillPrompt(selected, getInitialWorkingDir()))}
              onSaved={(updated) =>
                setSkills((prev) => prev.map((s) => (s.path === updated.path ? updated : s)))
              }
              onDeleted={() => {
                setSelectedPath(null);
                // Re-list rather than splice: `scan_skills_from_dir` keeps a `seen` set and DROPS a
                // same-named skill in a lower-priority root, so deleting a visible one can UNSHADOW a
                // different skill that was never in this list. Only the backend knows what is there now.
                loadSkills();
              }}
            />
          </div>
        ) : loaded && !error && skills.length === 0 ? (
          <EmptyState
            icon={<Zap />}
            title={intl.formatMessage(i18n.noSkillsInstalled)}
            body={intl.formatMessage(i18n.noSkillsDescription)}
          />
        ) : null
      }
    />
  );
}
