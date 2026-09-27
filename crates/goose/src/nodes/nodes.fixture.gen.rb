#!/usr/bin/env ruby
# Generates crates/goose/src/nodes/nodes.fixture.json. The resolve expectations are computed by an
# encoding of design §6.3 written independently of the Rust and TS implementations, so the fixture
# is a third opinion both suites must agree with (and a static file once written).
require 'json'

OUT = ARGV[0] or abort 'usage: gen_fixture.rb <out.json>'

NAMES = %w[a b c].freeze

def fact_json(sit)
  case sit
  when :servable then { 'kind' => 'servable' }
  when :busy then { 'kind' => 'busy' }
  when :not_loaded then { 'kind' => 'notLoaded' }
  when :cant_run then { 'kind' => 'cantRun', 'reason' => 'Work\'s Mac Studio is not connected' }
  when :load_failed then { 'kind' => 'loadFailed', 'words' => 'the peer left mid-load' }
  end
end

def why(fact)
  case fact['kind']
  when 'busy' then { 'kind' => 'busy' }
  when 'notLoaded' then { 'kind' => 'notLoaded' }
  when 'cantRun' then { 'kind' => 'cantRun', 'reason' => fact['reason'] }
  when 'loadFailed' then { 'kind' => 'loadFailed', 'words' => fact['words'] }
  else { 'kind' => 'unknown' }
  end
end

# The expected decision, rule by rule, in plain Ruby.
def expect(chain, when_rule, if_not_loaded, facts, sticky, share)
  share = share.dup
  rank = ->(n) { chain.index { |l| l['node'] == n } + 1 }
  fact = ->(n) { facts[n] }
  if when_rule == 'share'
    if sticky && chain.any? { |l| l['node'] == sticky }
      f = fact.(sticky)
      return [{ 'kind' => 'serve', 'node' => sticky, 'rank' => rank.(sticky), 'tried' => [] }, share] if f && f['kind'] == 'servable'
      return [{ 'kind' => 'queue', 'nodes' => [sticky], 'tried' => [] }, share] if f && f['kind'] == 'busy'
    end
    takes = ->(n) { f = fact.(n); f && (f['kind'] == 'servable' || (f['kind'] == 'notLoaded' && if_not_loaded == 'load')) }
    cands = chain.select { |l| takes.(l['node']) }
    tried = chain.reject { |l| takes.(l['node']) }.map { |l| { 'node' => l['node'], 'why' => why(fact.(l['node']) || {}) } }
    if cands.empty?
      busy = tried.select { |t| t['why']['kind'] == 'busy' }.map { |t| t['node'] }
      return [{ 'kind' => 'exhausted', 'tried' => tried }, share] if busy.empty?
      return [{ 'kind' => 'queue', 'nodes' => busy, 'tried' => tried.reject { |t| t['why']['kind'] == 'busy' } }, share]
    end
    total = cands.sum { |l| l['weight'] }
    best = nil
    cands.each do |l|
      share[l['node']] = (share[l['node']] || 0) + l['weight']
      best = l['node'] if best.nil? || share[l['node']] > share[best]
    end
    share[best] -= total
    kind = fact.(best)['kind'] == 'notLoaded' ? 'load' : 'serve'
    return [{ 'kind' => kind, 'node' => best, 'rank' => rank.(best), 'tried' => tried }, share]
  end

  tried = []
  busy = []
  chain.each do |l|
    n = l['node']
    f = fact.(n) || {}
    case f['kind']
    when 'servable' then return [{ 'kind' => 'serve', 'node' => n, 'rank' => rank.(n), 'tried' => tried }, share]
    when 'busy'
      return [{ 'kind' => 'queue', 'nodes' => [n], 'tried' => tried }, share] if when_rule == 'failover'
      busy << n
    when 'notLoaded'
      return [{ 'kind' => 'load', 'node' => n, 'rank' => rank.(n), 'tried' => tried }, share] if if_not_loaded == 'load'
    end
    tried << { 'node' => n, 'why' => why(f) }
  end
  return [{ 'kind' => 'exhausted', 'tried' => tried }, share] if busy.empty?
  [{ 'kind' => 'queue', 'nodes' => busy, 'tried' => tried.reject { |t| t['why']['kind'] == 'busy' } }, share]
end

cases = []
SITUATIONS = [
  [:servable, 'load'], [:busy, 'load'], [:cant_run, 'load'],
  [:not_loaded, 'load'], [:not_loaded, 'useNext'], [:load_failed, 'load']
].freeze
%w[failover overflow share].each do |when_rule|
  SITUATIONS.each do |(sit, inl)|
    (1..3).each do |len|
      chain = NAMES.first(len).map { |n| { 'node' => n, 'weight' => 1 } }
      facts = { 'a' => fact_json(sit) }
      NAMES[1...len].each { |n| facts[n] = fact_json(:servable) }
      entry = { 'chain' => chain, 'when' => when_rule, 'ifNotLoaded' => inl }
      decision, after = expect(chain, when_rule, inl, facts, nil, {})
      cases << { 'name' => "#{when_rule} · 1st #{sit}#{sit == :not_loaded ? " (#{inl})" : ''} · chain of #{len}",
                 'entry' => entry, 'facts' => facts, 'share' => {}, 'expect' => decision, 'shareAfter' => after }
    end
  end
end

# Beyond the matrix: the cases a single "1st varies" matrix cannot reach.
extra = [
  ['failover · every entry passed over names each', %w[a b c], 'failover', 'useNext',
   { 'a' => fact_json(:cant_run), 'b' => fact_json(:load_failed), 'c' => fact_json(:not_loaded) }, nil, {}, [1, 1, 1]],
  ['failover · useNext with nothing servable is refused, never loaded', %w[a b], 'failover', 'useNext',
   { 'a' => fact_json(:not_loaded), 'b' => fact_json(:cant_run) }, nil, {}, [1, 1]],
  ['failover · 1st can\'t run, 2nd busy queues on the 2nd', %w[a b c], 'failover', 'load',
   { 'a' => fact_json(:cant_run), 'b' => fact_json(:busy), 'c' => fact_json(:servable) }, nil, {}, [1, 1, 1]],
  ['overflow · every entry busy queues on all', %w[a b c], 'overflow', 'load',
   { 'a' => fact_json(:busy), 'b' => fact_json(:busy), 'c' => fact_json(:busy) }, nil, {}, [1, 1, 1]],
  ['overflow · busy then not loaded loads the 2nd', %w[a b], 'overflow', 'load',
   { 'a' => fact_json(:busy), 'b' => fact_json(:not_loaded) }, nil, {}, [1, 1]],
  ['overflow · busy, can\'t run: queues on the busy one and names the other', %w[a b], 'overflow', 'load',
   { 'a' => fact_json(:busy), 'b' => fact_json(:cant_run) }, nil, {}, [1, 1]],
  ['failover · a node with no fact is never guessed servable', %w[a b], 'failover', 'load',
   { 'b' => fact_json(:servable) }, nil, {}, [1, 1]],
  ['share · 2:1, first pick', %w[a b], 'share', 'load',
   { 'a' => fact_json(:servable), 'b' => fact_json(:servable) }, nil, {}, [2, 1]],
  ['share · 2:1, second pick', %w[a b], 'share', 'load',
   { 'a' => fact_json(:servable), 'b' => fact_json(:servable) }, nil, { 'a' => -1, 'b' => 1 }, [2, 1]],
  ['share · 2:1, third pick', %w[a b], 'share', 'load',
   { 'a' => fact_json(:servable), 'b' => fact_json(:servable) }, nil, { 'a' => 1, 'b' => -1 }, [2, 1]],
  ['share · sticky servable keeps the conversation and leaves the round-robin alone', %w[a b], 'share', 'load',
   { 'a' => fact_json(:servable), 'b' => fact_json(:servable) }, 'b', { 'a' => 5 }, [2, 1]],
  ['share · sticky busy queues on the sticky node', %w[a b], 'share', 'load',
   { 'a' => fact_json(:servable), 'b' => fact_json(:busy) }, 'b', {}, [1, 1]],
  ['share · sticky can\'t run picks anew', %w[a b], 'share', 'load',
   { 'a' => fact_json(:servable), 'b' => fact_json(:cant_run) }, 'b', {}, [1, 1]],
  ['share · sticky not in the chain is ignored', %w[a b], 'share', 'load',
   { 'a' => fact_json(:servable), 'b' => fact_json(:servable) }, 'zz', {}, [1, 3]],
  ['share · all busy queues on all', %w[a b], 'share', 'load',
   { 'a' => fact_json(:busy), 'b' => fact_json(:busy) }, nil, {}, [1, 1]],
  ['share · not loaded under useNext is passed over', %w[a b], 'share', 'useNext',
   { 'a' => fact_json(:not_loaded), 'b' => fact_json(:servable) }, nil, {}, [3, 1]]
]
extra.each do |(name, nodes, when_rule, inl, facts, sticky, share, weights)|
  chain = nodes.each_with_index.map { |n, i| { 'node' => n, 'weight' => weights[i] } }
  entry = { 'chain' => chain, 'when' => when_rule, 'ifNotLoaded' => inl }
  decision, after = expect(chain, when_rule, inl, facts, sticky, share)
  c = { 'name' => name, 'entry' => entry, 'facts' => facts, 'share' => share, 'expect' => decision, 'shareAfter' => after }
  c['sticky'] = sticky if sticky
  cases << c
end

split_mlx = { 'id' => '27b-both', 'name' => '27B Atlassian · both Macs', 'kind' => 'mlx',
              'model' => 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
              'placement' => { 'kind' => 'pipeline', 'macs' => ['local', 'link:studio'], 'link' => 'jaccl' },
              'goal' => 'chat', 'origin' => 'runIt' }
flash = { 'id' => 'flash-here', 'name' => 'Flash · this Mac', 'kind' => 'mlx',
          'model' => 'rapid-mlx/Qwen3.8-Flash-Next-4bit',
          'placement' => { 'kind' => 'single', 'macs' => ['local'] }, 'keepLoaded' => true, 'origin' => 'user' }
remote = { 'id' => 'flash-studio', 'name' => 'Flash · Work\'s Mac Studio', 'kind' => 'mlx',
           'model' => 'rapid-mlx/Qwen3.8-Flash-Next-4bit',
           'placement' => { 'kind' => 'single', 'macs' => ['link:studio'] }, 'origin' => 'user' }
tensor = { 'id' => '27b-tensor', 'name' => '27B · tensor', 'kind' => 'mlx',
           'model' => 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
           'placement' => { 'kind' => 'tensor', 'macs' => ['local', 'link:studio'] }, 'goal' => 'longDocuments', 'origin' => 'user' }
pool_mlx = { 'id' => 'mihai-mlx', 'name' => 'Mihai Macbook engine', 'kind' => 'mlx',
             'placement' => { 'kind' => 'follows' }, 'poolDevice' => 'mihai-mlx', 'origin' => 'pool' }
cloud = { 'id' => 'sonnet', 'name' => 'Claude Sonnet · OpenRouter', 'kind' => 'cloud',
          'model' => 'anthropic/claude-sonnet-4', 'provider' => 'openrouter', 'origin' => 'user' }
endpoint = { 'id' => 'my-server', 'name' => 'My server', 'kind' => 'endpoint', 'model' => 'qwen', 'provider' => 'custom_local', 'origin' => 'user' }
everyday = { 'id' => 'everyday', 'name' => 'Everyday', 'note' => 'Big model thinks and builds; the cloud takes the overflow.',
             'roles' => {
               'chat' => { 'chain' => [{ 'node' => '27b-both', 'weight' => 1 }, { 'node' => 'sonnet', 'weight' => 1 }], 'when' => 'failover', 'ifNotLoaded' => 'load' },
               'planning' => { 'chain' => [{ 'node' => '27b-both', 'weight' => 1 }], 'when' => 'failover', 'ifNotLoaded' => 'load' },
               'build' => { 'chain' => [{ 'node' => '27b-both', 'weight' => 2 }, { 'node' => 'sonnet', 'weight' => 1 }], 'when' => 'share', 'ifNotLoaded' => 'useNext' }
             } }
quick = { 'id' => 'quick', 'name' => 'Quick', 'roles' => {
  'chat' => { 'chain' => [{ 'node' => 'flash-here', 'weight' => 1 }], 'when' => 'overflow', 'ifNotLoaded' => 'load' },
  'build' => { 'chain' => [{ 'node' => '27b-both', 'weight' => 1 }], 'when' => 'failover', 'ifNotLoaded' => 'load' }
} }

configs = [
  { 'name' => 'fresh: auto chats, pool builds', 'config' => { 'version' => 1, 'defs' => [], 'strategies' => [], 'declined' => [],
                                                              'forNewChats' => { 'kind' => 'auto' }, 'forBuilds' => { 'kind' => 'pool' } } },
  { 'name' => 'every node kind and way, two strategies', 'config' => {
    'version' => 1, 'defs' => [split_mlx, flash, remote, tensor, pool_mlx, cloud, endpoint], 'strategies' => [everyday, quick],
    'declined' => ['old-cloud'], 'forNewChats' => { 'kind' => 'strategy', 'id' => 'everyday' },
    'forBuilds' => { 'kind' => 'strategy', 'id' => 'quick' } } },
  { 'name' => 'new chats on one node', 'config' => { 'version' => 1, 'defs' => [flash], 'strategies' => [], 'declined' => [],
                                                     'forNewChats' => { 'kind' => 'node', 'id' => 'flash-here' }, 'forBuilds' => { 'kind' => 'pool' } } }
]

model_ids = [
  ['swarm', { 'kind' => 'auto' }],
  ['swarm-build', { 'kind' => 'build' }],
  ['swarm-build:strategy:everyday', { 'kind' => 'buildStrategy', 'id' => 'everyday' }],
  ['node:27b-both', { 'kind' => 'node', 'id' => '27b-both' }],
  ['strategy:everyday', { 'kind' => 'strategy', 'id' => 'everyday' }],
  ['strategy:everyday@build', { 'kind' => 'strategy', 'id' => 'everyday', 'role' => 'build' }],
  ['strategy:everyday@frontend', { 'kind' => 'strategy', 'id' => 'everyday', 'role' => 'frontend' }],
  ['strategy:everyday@nope', nil],
  ['strategy:', nil],
  ['node:', nil],
  ['node:a:b', nil],
  ['node:a@b', nil],
  ['swarm-build:strategy:', nil],
  ['gpt-4o', nil],
  ['mihai-qwen3.8-27b-atlassian-q8-mlx', nil]
].map { |(id, route)| { 'id' => id, 'route' => route } }

inherit = {
  'chat' => 'build', 'planning' => 'chat', 'build' => 'chat',
  'testing' => 'build', 'frontend' => 'build', 'backend' => 'build'
}
role_sets = [%w[chat], %w[build], %w[chat build], %w[planning build], %w[testing], %w[], %w[chat frontend]]
effective = []
role_sets.each do |set|
  inherit.each_key do |role|
    seen = []
    cur = role
    result = nil
    loop do
      if set.include?(cur) then result = cur; break end
      break if seen.include?(cur)
      seen << cur
      cur = inherit[cur]
    end
    effective << { 'set' => set, 'role' => role, 'expect' => result }
  end
end

names = { '27b-both' => '27B Atlassian · both Macs', 'sonnet' => 'Claude Sonnet · OpenRouter', 'flash-here' => 'Flash · this Mac' }
sentences = [
  { 'strategy' => everyday, 'role' => 'chat', 'names' => names, 'expect' => {
    'role' => 'chat', 'when' => 'failover', 'ifNotLoaded' => 'load',
    'entries' => [{ 'node' => '27b-both', 'name' => '27B Atlassian · both Macs', 'rank' => 1, 'weight' => 1 },
                  { 'node' => 'sonnet', 'name' => 'Claude Sonnet · OpenRouter', 'rank' => 2, 'weight' => 1 }] } },
  { 'strategy' => everyday, 'role' => 'build', 'names' => names, 'expect' => {
    'role' => 'build', 'when' => 'share', 'ifNotLoaded' => 'useNext', 'shareTotal' => 3,
    'entries' => [{ 'node' => '27b-both', 'name' => '27B Atlassian · both Macs', 'rank' => 1, 'weight' => 2 },
                  { 'node' => 'sonnet', 'name' => 'Claude Sonnet · OpenRouter', 'rank' => 2, 'weight' => 1 }] } },
  { 'strategy' => everyday, 'role' => 'testing', 'names' => names, 'expect' => {
    'role' => 'testing', 'sameAs' => 'build', 'when' => 'share', 'ifNotLoaded' => 'useNext', 'shareTotal' => 3,
    'entries' => [{ 'node' => '27b-both', 'name' => '27B Atlassian · both Macs', 'rank' => 1, 'weight' => 2 },
                  { 'node' => 'sonnet', 'name' => 'Claude Sonnet · OpenRouter', 'rank' => 2, 'weight' => 1 }] } },
  { 'strategy' => quick, 'role' => 'planning', 'names' => {}, 'expect' => {
    'role' => 'planning', 'sameAs' => 'chat', 'when' => 'overflow', 'ifNotLoaded' => 'load',
    'entries' => [{ 'node' => 'flash-here', 'name' => 'flash-here', 'rank' => 1, 'weight' => 1 }] } },
  { 'strategy' => { 'id' => 'p', 'name' => 'Planning only', 'roles' => {
      'planning' => { 'chain' => [{ 'node' => 'sonnet', 'weight' => 1 }], 'when' => 'failover', 'ifNotLoaded' => 'load' } } },
    'role' => 'chat', 'names' => names, 'expect' => nil }
]

fixture = {
  '_' => 'Shared by crates/goose/src/nodes (Rust) and ui/desktop/src/components/nodes (TS): both suites run every case. Generated by an independent encoding of design §6.3; edit nodes.fixture.gen.rb and rerun it (ruby nodes.fixture.gen.rb nodes.fixture.json), never the cases.',
  'configs' => configs,
  'modelIds' => model_ids,
  'effectiveRole' => effective,
  'resolve' => cases,
  'sentenceFacts' => sentences
}
File.write(OUT, JSON.pretty_generate(fixture) + "\n")
puts "#{cases.size} resolve cases, #{effective.size} inheritance cases, #{model_ids.size} model ids, #{configs.size} configs, #{sentences.size} sentence cases"
