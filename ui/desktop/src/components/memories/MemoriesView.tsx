import { useState, useEffect, useMemo, useCallback } from 'react';
import { Brain, AlertCircle } from 'lucide-react';
import { ScrollArea } from '../ui/scroll-area';
import { Card } from '../ui/card';
import { Skeleton } from '../ui/skeleton';
import { Button } from '../ui/button';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { errorMessage } from '../../utils/conversionUtils';
import { getInitialWorkingDir } from '../../utils/workingDir';
import { SearchView } from '../conversation/SearchView';
import { getSearchShortcutText } from '../../utils/keyboardShortcuts';

/** One stored memory — mirrors the shape returned by the `list-memories` IPC in main.ts. */
export interface MemoryEntry {
  id: string;
  category: string;
  scope: 'global' | 'local';
  tags: string[];
  content: string;
  updatedAt: number;
}

// Solid, saturated hues for the common memory-type tags (goose writes a `# <type> ...` header on each entry).
// A memory's first tag is usually its type; anything unrecognised falls back to a neutral slate chip.
const TYPE_COLOR: Record<string, string> = {
  feedback: '#b45309',
  project: '#1d4ed8',
  user: '#0f766e',
  reference: '#7c3aed',
  memory: '#0f766e',
};

function tagColor(tag: string): string {
  const head = tag.split(':')[0].toLowerCase();
  return TYPE_COLOR[head] ?? '#475569';
}

/** kebab-case category → readable title ("absorb-human-voice" → "Absorb Human Voice"). */
function prettyTitle(category: string): string {
  return category
    .replace(/[-_]+/g, ' ')
    .replace(/\b\w/g, (c) => c.toUpperCase())
    .trim();
}

function firstLine(content: string): string {
  const line = content.split('\n').find((l) => l.trim().length > 0) ?? '';
  // Strip markdown emphasis so the snippet reads as plain prose.
  return line.replace(/\*\*/g, '').replace(/^#+\s*/, '').trim();
}

const SCOPE_DOT: Record<MemoryEntry['scope'], string> = {
  global: 'bg-[#1d4ed8]',
  local: 'bg-[#b45309]',
};

function MemoryCardItem({
  memory,
  selected,
  onSelect,
}: {
  memory: MemoryEntry;
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
        <span className={`w-2 h-2 shrink-0 ${SCOPE_DOT[memory.scope]}`} />
        <div className="min-w-0 flex-1">
          <h3 className="text-sm truncate">{prettyTitle(memory.category)}</h3>
          <p className={`text-xs line-clamp-1 ${selected ? 'text-white/80' : 'text-text-secondary'}`}>
            {firstLine(memory.content)}
          </p>
        </div>
      </div>
    </Card>
  );
}

function MemorySkeleton() {
  return (
    <Card className="p-2 mb-2 bg-background-primary">
      <div className="min-w-0 flex-1">
        <Skeleton className="h-5 w-3/4 mb-2" />
        <Skeleton className="h-4 w-full" />
      </div>
    </Card>
  );
}

function TagChip({ tag }: { tag: string }) {
  return (
    <span
      className="text-[10px] px-1.5 py-0.5 text-white font-medium"
      style={{ backgroundColor: tagColor(tag), borderRadius: 2 }}
    >
      {tag}
    </span>
  );
}

function MemoryDetail({ memory }: { memory: MemoryEntry }) {
  return (
    <div className="h-full flex flex-col min-h-0">
      <div className="mb-3">
        <div className="flex items-center gap-2 mb-1">
          <span className={`w-2.5 h-2.5 shrink-0 ${SCOPE_DOT[memory.scope]}`} />
          <h2 className="text-xl font-medium truncate">{prettyTitle(memory.category)}</h2>
        </div>
        <div className="flex items-center gap-1.5 flex-wrap">
          <span className="text-[10px] uppercase tracking-wide text-text-tertiary">
            {memory.scope === 'global' ? 'Yours · global' : 'This project'}
          </span>
          {memory.tags.map((t) => (
            <TagChip key={t} tag={t} />
          ))}
        </div>
      </div>
      <ScrollArea className="flex-1 min-h-0">
        <div className="text-sm text-text-primary whitespace-pre-wrap break-words leading-relaxed pr-2">
          {memory.content}
        </div>
      </ScrollArea>
    </div>
  );
}

export default function MemoriesView() {
  const [memories, setMemories] = useState<MemoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [showSkeleton, setShowSkeleton] = useState(true);
  const [showContent, setShowContent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchTerm, setSearchTerm] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const loadMemories = useCallback(async () => {
    try {
      setLoading(true);
      setShowSkeleton(true);
      setShowContent(false);
      setError(null);
      const list = await window.electron.listMemories(getInitialWorkingDir());
      setMemories(list);
    } catch (err) {
      setError(errorMessage(err, 'Failed to load memories'));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadMemories();
  }, [loadMemories]);

  // Goose writes new memories from its own process while a window sits open — re-read on focus so the list on
  // screen matches disk (same reasoning as the Skills view).
  useEffect(() => {
    const onFocus = () => loadMemories();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [loadMemories]);

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

  const filtered = useMemo(() => {
    if (!searchTerm) return memories;
    const q = searchTerm.toLowerCase();
    return memories.filter(
      (m) =>
        m.category.toLowerCase().includes(q) ||
        m.content.toLowerCase().includes(q) ||
        m.tags.some((t) => t.toLowerCase().includes(q))
    );
  }, [memories, searchTerm]);

  const groups = useMemo(() => {
    const order: MemoryEntry['scope'][] = ['global', 'local'];
    const titles: Record<MemoryEntry['scope'], string> = {
      global: 'Yours',
      local: 'This project',
    };
    return order
      .map((scope) => ({ scope, title: titles[scope], items: filtered.filter((m) => m.scope === scope) }))
      .filter((g) => g.items.length > 0);
  }, [filtered]);

  const selected = useMemo(
    () => memories.find((m) => m.id === selectedId) ?? null,
    [memories, selectedId]
  );

  const renderList = () => {
    if (loading || showSkeleton) {
      return (
        <div className="space-y-2">
          <MemorySkeleton />
          <MemorySkeleton />
          <MemorySkeleton />
        </div>
      );
    }
    if (error) {
      return (
        <div className="flex flex-col items-center justify-center h-full text-text-secondary">
          <AlertCircle className="h-12 w-12 text-red-500 mb-4" />
          <p className="text-lg mb-2">Error loading memories</p>
          <p className="text-sm text-center mb-4">{error}</p>
          <Button onClick={loadMemories} variant="default">
            Try Again
          </Button>
        </div>
      );
    }
    if (memories.length === 0) {
      return (
        <div className="flex flex-col justify-center pt-2 h-full">
          <p className="text-lg">No memories yet</p>
          <p className="text-sm text-text-secondary">
            Memories are stored in ~/.config/goose/memory/ — imported from your cloud profile and learned by
            goose as it works.
          </p>
        </div>
      );
    }
    if (filtered.length === 0 && searchTerm) {
      return (
        <div className="flex flex-col items-center justify-center h-full text-text-secondary mt-4">
          <Brain className="h-12 w-12 mb-4" />
          <p className="text-lg mb-2">No matching memories</p>
          <p className="text-sm">Try adjusting your search terms</p>
        </div>
      );
    }
    return (
      <div className="space-y-4">
        {groups.map((group) => (
          <div key={group.scope}>
            <h2 className="text-[10px] font-bold tracking-wider text-text-tertiary mb-1 px-1">
              {group.title.toUpperCase()} · {group.items.length}
            </h2>
            {group.items.map((m) => (
              <MemoryCardItem
                key={m.id}
                memory={m}
                selected={m.id === selectedId}
                onSelect={() => setSelectedId(m.id)}
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
              <h1 className="text-4xl font-light">Memories</h1>
            </div>
            <p className="text-sm text-text-secondary mb-1">
              What goose remembers — imported from your cloud profile and learned as it works.{' '}
              {getSearchShortcutText()} to search.
            </p>
          </div>
        </div>

        <div className="flex-1 min-h-0 relative px-8 flex gap-6">
          <div className="w-[300px] shrink-0 min-h-0">
            <ScrollArea className="h-full">
              <SearchView onSearch={(term) => setSearchTerm(term)} placeholder="Search memories...">
                <div
                  className={`h-full relative transition-all duration-300 ${
                    showContent || showSkeleton ? 'opacity-100 animate-in fade-in' : 'opacity-0'
                  }`}
                >
                  {renderList()}
                </div>
              </SearchView>
            </ScrollArea>
          </div>

          <div className="flex-1 min-w-0 min-h-0 border-l border-borderSubtle pl-6">
            {selected ? (
              <MemoryDetail memory={selected} />
            ) : (
              <div className="h-full flex flex-col items-center justify-center text-text-tertiary">
                <Brain className="h-10 w-10 mb-3" />
                <p className="text-sm">Select a memory to read it</p>
              </div>
            )}
          </div>
        </div>
      </div>
    </MainPanelLayout>
  );
}
