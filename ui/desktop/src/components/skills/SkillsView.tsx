import { useState, useEffect, useMemo, useCallback } from 'react';
import { Zap, AlertCircle, Plus } from 'lucide-react';
import { ScrollArea } from '../ui/scroll-area';
import { Card } from '../ui/card';
import { Button } from '../ui/button';
import { Skeleton } from '../ui/skeleton';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { errorMessage } from '../../utils/conversionUtils';
import { getInitialWorkingDir } from '../../utils/workingDir';
import { defineMessages, useIntl } from '../../i18n';
import { SearchView } from '../conversation/SearchView';
import { getSearchShortcutText } from '../../utils/keyboardShortcuts';
import { listSkillSources } from '../../acp/sources';
import type { SourceEntry } from '@aaif/goose-sdk';
import { SkillDetail } from './SkillDetail';
import { skillOrigin, type SkillOrigin } from './skillKinds';

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
  addSkill: {
    id: 'skillsView.addSkill',
    defaultMessage: 'Add Skill',
  },
  skillsDescription: {
    id: 'skillsView.skillsDescription',
    defaultMessage: 'View installed skills that extend Goose capabilities. {shortcut} to search.',
  },
  searchSkillsPlaceholder: {
    id: 'skillsView.searchSkillsPlaceholder',
    defaultMessage: 'Search skills...',
  },
  comingSoon: {
    id: 'skillsView.comingSoon',
    defaultMessage: 'Coming soon',
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

const ORIGIN_DOT: Record<SkillOrigin, string> = {
  persona: 'bg-[#7c3aed]',
  builtin: 'bg-[#0f766e]',
  global: 'bg-[#1d4ed8]',
  project: 'bg-[#b45309]',
};

function SkillItem({
  skill,
  selected,
  onSelect,
}: {
  skill: SkillEntry;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <Card
      onClick={onSelect}
      className={`py-2 px-3 mb-1 border-none cursor-pointer transition-all duration-150 ${
        selected ? 'bg-background-accent text-white' : 'bg-background-primary hover:bg-background-secondary'
      }`}
    >
      <div className="flex items-center gap-2 min-w-0">
        <span className={`w-2 h-2 shrink-0 ${ORIGIN_DOT[skillOrigin(skill)]}`} />
        <div className="min-w-0 flex-1">
          <h3 className="text-sm truncate">{skill.name}</h3>
          <p
            className={`text-xs line-clamp-1 ${selected ? 'text-white/80' : 'text-text-secondary'}`}
          >
            {skill.description}
          </p>
        </div>
      </div>
    </Card>
  );
}

function SkillSkeleton() {
  return (
    <Card className="p-2 mb-2 bg-background-primary">
      <div className="flex justify-between items-start gap-4">
        <div className="min-w-0 flex-1">
          <Skeleton className="h-5 w-3/4 mb-2" />
          <Skeleton className="h-4 w-full" />
        </div>
      </div>
    </Card>
  );
}

export default function SkillsView() {
  const intl = useIntl();
  const [skills, setSkills] = useState<SkillEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [showSkeleton, setShowSkeleton] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showContent, setShowContent] = useState(false);
  const [searchTerm, setSearchTerm] = useState('');
  const [selectedPath, setSelectedPath] = useState<string | null>(null);

  const filteredSkills = useMemo(() => {
    if (!searchTerm) return skills;
    const searchLower = searchTerm.toLowerCase();
    return skills.filter(
      (skill) =>
        skill.name.toLowerCase().includes(searchLower) ||
        skill.description.toLowerCase().includes(searchLower) ||
        skill.content.toLowerCase().includes(searchLower)
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
        items: filteredSkills.filter((s) => skillOrigin(s) === origin),
      }))
      .filter((g) => g.items.length > 0);
  }, [filteredSkills]);

  const selected = useMemo(
    () => skills.find((s) => s.path === selectedPath) ?? null,
    [skills, selectedPath]
  );

  const loadSkills = useCallback(async () => {
    try {
      setLoading(true);
      setShowSkeleton(true);
      setShowContent(false);
      setError(null);
      setSkills(await listSkillSources(getInitialWorkingDir()));
    } catch (err) {
      setError(errorMessage(err, 'Failed to load skills'));
    } finally {
      setLoading(false);
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

  useEffect(() => {
    if (!loading && showSkeleton) {
      const timer = setTimeout(() => {
        setShowSkeleton(false);
        setTimeout(() => setShowContent(true), 50);
      }, 300);
      return () => clearTimeout(timer);
    }
    return undefined;
  }, [loading, showSkeleton]);

  const renderContent = () => {
    if (loading || showSkeleton) {
      return (
        <div className="space-y-2">
          <SkillSkeleton />
          <SkillSkeleton />
          <SkillSkeleton />
        </div>
      );
    }

    if (error) {
      return (
        <div className="flex flex-col items-center justify-center h-full text-text-secondary">
          <AlertCircle className="h-12 w-12 text-red-500 mb-4" />
          <p className="text-lg mb-2">{intl.formatMessage(i18n.errorLoadingSkills)}</p>
          <p className="text-sm text-center mb-4">{error}</p>
          <Button onClick={loadSkills} variant="default">
            {intl.formatMessage(i18n.tryAgain)}
          </Button>
        </div>
      );
    }

    if (skills.length === 0) {
      return (
        <div className="flex flex-col justify-center pt-2 h-full">
          <p className="text-lg">{intl.formatMessage(i18n.noSkillsInstalled)}</p>
          <p className="text-sm text-text-secondary">
            {intl.formatMessage(i18n.noSkillsDescription)}
          </p>
        </div>
      );
    }

    if (filteredSkills.length === 0 && searchTerm) {
      return (
        <div className="flex flex-col items-center justify-center h-full text-text-secondary mt-4">
          <Zap className="h-12 w-12 mb-4" />
          <p className="text-lg mb-2">{intl.formatMessage(i18n.noMatchingSkills)}</p>
          <p className="text-sm">{intl.formatMessage(i18n.adjustSearchTerms)}</p>
        </div>
      );
    }

    return (
      <div className="space-y-4">
        {groups.map((group) => (
          <div key={group.origin}>
            <h2 className="text-[10px] font-bold tracking-wider text-text-tertiary mb-1 px-1">
              {group.title.toUpperCase()} · {group.items.length}
            </h2>
            {group.items.map((skill) => (
              <SkillItem
                key={skill.path}
                skill={skill}
                selected={skill.path === selectedPath}
                onSelect={() => setSelectedPath(skill.path)}
              />
            ))}
          </div>
        ))}
      </div>
    );
  };

  return (
    <MainPanelLayout>
      <div className="flex-1 flex flex-col min-h-0">
        <div className="bg-background-primary px-8 pb-8 pt-16">
          <div className="flex flex-col page-transition">
            <div className="flex justify-between items-center mb-1">
              <h1 className="text-4xl font-light">{intl.formatMessage(i18n.skillsTitle)}</h1>
              <Button
                variant="outline"
                size="sm"
                className="flex items-center gap-2"
                hidden
                title={intl.formatMessage(i18n.comingSoon)}
              >
                <Plus className="w-4 h-4" />
                {intl.formatMessage(i18n.addSkill)}
              </Button>
            </div>
            <p className="text-sm text-text-secondary mb-1">
              {intl.formatMessage(i18n.skillsDescription, {
                shortcut: getSearchShortcutText(),
              })}
            </p>
          </div>
        </div>

        <div className="flex-1 min-h-0 relative px-8 flex gap-6">
          <div className="w-[300px] shrink-0 min-h-0">
            <ScrollArea className="h-full">
              <SearchView
                onSearch={(term) => setSearchTerm(term)}
                placeholder={intl.formatMessage(i18n.searchSkillsPlaceholder)}
              >
                <div
                  className={`h-full relative transition-all duration-300 ${
                    showContent || showSkeleton ? 'opacity-100 animate-in fade-in' : 'opacity-0'
                  }`}
                >
                  {renderContent()}
                </div>
              </SearchView>
            </ScrollArea>
          </div>

          <div className="flex-1 min-w-0 min-h-0 border-l border-borderSubtle pl-6">
            {selected ? (
              <SkillDetail
                entry={selected}
                origin={skillOrigin(selected)}
                projectDir={getInitialWorkingDir()}
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
            ) : (
              <div className="h-full flex flex-col items-center justify-center text-text-tertiary">
                <Zap className="h-10 w-10 mb-3" />
                <p className="text-sm">Select a skill to read it</p>
              </div>
            )}
          </div>
        </div>
      </div>
    </MainPanelLayout>
  );
}
