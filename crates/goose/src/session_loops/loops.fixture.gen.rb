#!/usr/bin/env ruby
# Generates crates/goose/src/session_loops/loops.fixture.json. Every expectation is computed by an
# encoding of DESIGN-SESSION-LOOPS.md (§4.3-§4.7, §7.2, §7.4) written in plain Ruby, independently
# of the Rust rules (session_loops/rules.rs, prompt.rs) and the desktop mirror
# (components/loops/model.ts), so the fixture is a third opinion both suites must agree with.
# Run: ruby loops.fixture.gen.rb loops.fixture.json
require 'json'
require 'time'

OUT = ARGV[0] or abort 'usage: loops.fixture.gen.rb <out.json>'

# chrono's TimeDelta holds at most i64::MAX / 1000 seconds.
MAX_DELTA_SECS = 9_223_372_036_854_775
I64_MAX = 9_223_372_036_854_775_807

def t(hm, sec = 0)
  h, m = hm.split(':').map(&:to_i)
  Time.utc(2026, 9, 27, h, m, sec)
end

def iso(time)
  time.utc.strftime('%Y-%m-%dT%H:%M:%SZ')
end

def ptime(text)
  Time.iso8601(text).utc
end

# ---------------------------------------------------------------------------------------------
# The cadence grammar: `<n>s|m|h`, n a (signed) integer > 0 that chrono can hold.
# ---------------------------------------------------------------------------------------------

def cadence_secs(text)
  s = text.strip
  return nil if s.empty?
  unit = s[-1]
  num = s[0...-1].strip
  return nil unless num =~ /\A[+-]?\d+\z/
  n = num.to_i
  return nil if n <= 0 || n > I64_MAX
  mult = { 's' => 1, 'm' => 60, 'h' => 3600 }[unit]
  return nil unless mult
  secs = n * mult
  return nil if secs > MAX_DELTA_SECS
  secs
end

def cadence_label(cadence)
  case cadence['kind']
  when 'self_paced' then 'goose decides when'
  when 'back_to_back' then 'back to back'
  else
    every = cadence['every']
    if cadence_secs(every)
      s = every.strip
      n = s[0...-1].strip.to_i
      { 's' => "every #{n} s", 'm' => "every #{n} min", 'h' => "every #{n} h" }[s[-1]]
    else
      "every #{every.strip}"
    end
  end
end

def duration_words(seconds)
  s = [seconds, 0].max
  h = s / 3600
  m = (s % 3600) / 60
  sec = s % 60
  return(m > 0 ? "#{h}h #{m}m" : "#{h}h") if h > 0
  return(sec > 0 ? "#{m}m #{sec}s" : "#{m}m") if m > 0
  "#{sec}s"
end

def clock(time, offset_minutes)
  (time.utc + offset_minutes * 60).strftime('%H:%M')
end

# ---------------------------------------------------------------------------------------------
# Records
# ---------------------------------------------------------------------------------------------

LOOP_ID = 'lp_0a1b2c3d'
UUIDS = %w[5f0c6a2e9d3b4c1a8e7f6d5c4b3a2918 0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e 9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d
           11112222333344445555666677778888 aaaabbbbccccddddeeeeffff00001111 12341234123412341234123412341234].freeze
WD = '/Users/mihai/work/users-gen'
STATE = '.goose/loops/users-csv/NOW.md'
GOAL = "Make scripts/generate_users.js produce every problem class in notes/kickoff.md\nKeep the seed at 42."

QUALITY_STEPS = "1. Discover: open {state_file}, then run or read what {goal_first_line} names in {working_dir}. List what is broken, missing or confusing, each with the evidence you saw (command output, file:line).\n" \
  "2. Critique: rank what you found by how much it blocks the goal; pick the ONE item that matters most (the last tick named: {last_next_step}).\n" \
  "3. Fix: make that change, and only that change.\n" \
  "4. Prove it with the check — {check} — and quote the result. A fix without a quoted result is not done.\n" \
  '5. Rewrite {state_file}: what is now true, what is next, what you found but did not fix.'
UNTIL_STEPS = "1. Run the check — {check} — and read why it fails.\n2. Fix the first cause it names.\n" \
  "3. Run the check again — {check} — and quote the result.\n4. Rewrite {state_file}."
WATCH_STEPS = "1. Look at what {goal_first_line} names (a build, a deploy, a folder, a URL) and compare it with {state_file}.\n" \
  "2. If nothing changed, say so in one line and report progress.\n" \
  "3. If something changed, do what the goal asks and quote the evidence.\n4. Rewrite {state_file}."

def every(e)
  { 'kind' => 'every', 'every' => e }
end
SELF = { 'kind' => 'self_paced' }.freeze
B2B = { 'kind' => 'back_to_back' }.freeze

def rec(opts = {})
  r = {
    'id' => LOOP_ID, 'goal' => GOAL, 'template' => 'quality', 'steps' => QUALITY_STEPS,
    'cadence' => every('10m'), 'stateFile' => STATE, 'status' => 'waiting',
    'createdAt' => iso(t('21:59')), 'startedAt' => iso(t('21:59')), 'ticks' => []
  }
  opts.each { |k, v| v.nil? ? r.delete(k) : r[k] = v }
  r
end

def tick_id(n)
  "looptick_#{LOOP_ID}_#{n}_#{UUIDS[(n - 1) % UUIDS.size]}"
end

def report(verdict, summary, next_step, extra = {})
  { 'verdict' => verdict, 'summary' => summary, 'nextStep' => next_step }.merge(extra)
end

def tick(n, started, ended, opts = {})
  tk = { 'n' => n, 'origin' => n == 1 ? 'first' : 'cadence', 'startedAt' => iso(started),
         'firstMessageId' => tick_id(n), 'wrote' => [] }
  tk['endedAt'] = iso(ended) if ended
  opts.each { |k, v| v.nil? ? tk.delete(k) : tk[k] = v }
  tk
end

def check_run(command, started, ran, exit_code, tail, opts = {})
  c = { 'command' => command, 'startedAt' => iso(started), 'ran' => ran, 'outputTail' => tail }
  c['exit'] = exit_code unless exit_code.nil?
  c.merge(opts)
end

CHECK = 'node scripts/validate_users.js'

# ---------------------------------------------------------------------------------------------
# The next tick (§4.3)
# ---------------------------------------------------------------------------------------------

def next_tick(record, now, reviewers)
  last = record['ticks'].last
  return { 'kind' => 'at', 'next' => { 'at' => iso(now), 'reason' => { 'kind' => 'first' } } } unless last
  raise 'not ended' unless last['endedAt']
  ended = ptime(last['endedAt'])
  due = lambda do |at, reason|
    if at <= now && reviewers
      { 'kind' => 'after_reviewers', 'n' => last['n'] }
    else
      { 'kind' => 'at', 'next' => { 'at' => iso(at), 'reason' => reason } }
    end
  end
  c = record['cadence']
  case c['kind']
  when 'every'
    secs = cadence_secs(c['every']) or raise 'bad cadence'
    cand = ptime(last['startedAt']) + secs
    if cand > now
      due.call(cand, { 'kind' => 'cadence' })
    else
      due.call(now, { 'kind' => 'overdue' })
    end
  when 'self_paced'
    given = last.dig('report', 'nextIn')
    given = given.strip if given
    if given.nil? || given.empty?
      return { 'kind' => 'waiting_you', 'reason' => { 'kind' => 'no_delay', 'n' => last['n'] } }
    end
    secs = cadence_secs(given)
    unless secs
      return { 'kind' => 'waiting_you', 'reason' => { 'kind' => 'bad_delay', 'n' => last['n'], 'given' => given } }
    end
    at = [ended + secs, now].max
    reason = { 'kind' => 'self_paced', 'interval' => given }
    reason['reason'] = last['report']['nextReason'] if last['report']['nextReason']
    due.call(at, reason)
  else
    due.call(now, { 'kind' => 'back_to_back' })
  end
end

# ---------------------------------------------------------------------------------------------
# After a tick (§4.6)
# ---------------------------------------------------------------------------------------------

def norm_step(s)
  s.split.join(' ').downcase
end

def stalled?(prev, cur)
  return false unless prev['report'] && cur['report']
  cur['report']['verdict'] == 'progress' && cur['wrote'].empty? &&
    norm_step(prev['report']['nextStep']) == norm_step(cur['report']['nextStep'])
end

def decide(record, facts)
  now = ptime(facts['now'])
  cur = record['ticks'].last
  n = cur['n']
  prev = record['ticks'].size >= 2 ? record['ticks'][-2] : nil
  finish = facts['end']
  cause = finish['cause']
  outcome =
    if finish['kind'] == 'cancelled' && cause && cause['kind'] == 'yield'
      o = { 'kind' => 'yielded', 'toSession' => cause['toSession'], 'toChat' => cause['toChat'] }
      o['way'] = cause['way'] if cause['way']
      o
    elsif finish['kind'] == 'cancelled'
      { 'kind' => 'stopped_by_you' }
    elsif facts['asked']
      { 'kind' => 'asked', 'itemId' => facts['asked']['itemId'], 'question' => facts['asked']['question'] }
    elsif finish['kind'] == 'errored'
      { 'kind' => 'failed', 'errorClass' => finish['errorClass'], 'error' => finish['error'] }
    elsif cur['report'].nil?
      { 'kind' => 'no_report' }
    else
      { 'kind' => cur['report']['verdict'] }
    end
  out = ->(status, reason) { { 'outcome' => outcome, 'status' => status, 'reason' => reason } }

  return out.call('ended', { 'kind' => 'stopped_by_you', 'n' => n }) if finish['kind'] == 'cancelled' && cause && cause['kind'] == 'loop_stopped'

  checked = %w[progress done].include?(outcome['kind'])
  run = nil
  if record['check'] && checked
    run = facts['check'] or return { 'error' => true }
    return out.call('ended', { 'kind' => 'goal_met', 'n' => n, 'check' => record['check'] }) if run['ran'] && run['exit'] == 0
  end
  return out.call('ended', { 'kind' => 'reported_done', 'n' => n }) if record['check'].nil? && outcome['kind'] == 'done'
  k = record['stopAfterTicks']
  return out.call('ended', { 'kind' => 'reached_count', 'k' => k }) if k && n >= k

  case outcome['kind']
  when 'stopped_by_you' then return out.call('paused', { 'kind' => 'you_stopped_tick', 'n' => n })
  when 'yielded' then return out.call('waiting_turn', { 'kind' => 'user_turn', 'sessionId' => outcome['toSession'], 'chat' => outcome['toChat'] })
  when 'asked' then return out.call('needs_you', { 'kind' => 'asked', 'n' => n, 'itemId' => outcome['itemId'], 'question' => outcome['question'] })
  when 'blocked'
    on = cur['report']['blockedOn']
    on = on.strip if on
    on = "tick #{n} did not say what it is blocked on" if on.nil? || on.empty?
    return out.call('paused', { 'kind' => 'blocked', 'n' => n, 'blockedOn' => on })
  end
  if run && !run['ran']
    return out.call('paused', { 'kind' => 'check_could_not_run', 'n' => n, 'error' => run['error'] || 'the check did not start and named no error' })
  end
  if outcome['kind'] == 'failed' && prev && prev['outcome'] && prev['outcome']['kind'] == 'failed' && prev['outcome']['errorClass'] == outcome['errorClass']
    return out.call('paused', { 'kind' => 'same_failure_twice', 'prev' => prev['n'], 'n' => n, 'error' => outcome['error'] })
  end
  if outcome['kind'] == 'no_report' && prev && prev['outcome'] && prev['outcome']['kind'] == 'no_report'
    return out.call('paused', { 'kind' => 'no_report_twice', 'prev' => prev['n'], 'n' => n })
  end
  if outcome['kind'] == 'progress' && prev && stalled?(prev, cur)
    return out.call('paused', { 'kind' => 'stalled', 'prev' => prev['n'], 'n' => n })
  end
  nt = next_tick(record, now, facts['reviewersPending'])
  case nt['kind']
  when 'at' then { 'outcome' => outcome, 'status' => 'waiting', 'nextTick' => nt['next'] }
  when 'after_reviewers' then out.call('waiting_turn', { 'kind' => 'reviewers', 'n' => nt['n'] })
  else out.call('waiting_you', nt['reason'])
  end
end

# ---------------------------------------------------------------------------------------------
# Effective status (§5.1) and the status sentence (§8.4)
# ---------------------------------------------------------------------------------------------

def effective(record, proof)
  return { 'status' => record['status'], 'reason' => record['statusReason'] } if %w[paused ended].include?(record['status'])
  closed = { 'status' => 'paused', 'reason' => { 'kind' => 'closed' } }
  return closed if proof.nil? || proof['kind'] == 'gone'
  return { 'status' => 'elsewhere' } if proof['kind'] == 'live'
  { 'status' => record['status'], 'reason' => record['statusReason'] }
end

def ticks_due(record, now)
  nt = record['nextTick'] or return 0
  at = ptime(nt['at'])
  return 0 if at > now
  if record['cadence']['kind'] == 'every'
    ((now - at).to_i / cadence_secs(record['cadence']['every'])) + 1
  else
    1
  end
end

def sent(key, facts, text)
  { 'key' => key, 'facts' => facts, 'text' => text }
end

def sentence(record, status, reason, now, off)
  hm = ->(s) { clock(ptime(s), off) }
  since = ->(s) { duration_words((now - ptime(s)).to_i) }
  last = record['ticks'].last
  nxt = ((last && last['n']) || 0) + 1
  r = reason || {}
  case status
  when 'running'
    n = last['n'].to_s
    time = hm.call(last['startedAt'])
    el = since.call(last['startedAt'])
    if last['served']
      node = last['served']['node']
      sent('loops.now.runningOn', { 'n' => n, 'time' => time, 'elapsed' => el, 'node' => node }, "Tick #{n} · started #{time} · #{el} · on #{node}")
    else
      sent('loops.now.running', { 'n' => n, 'time' => time, 'elapsed' => el }, "Tick #{n} · started #{time} · #{el}")
    end
  when 'checking'
    c = last['check']
    el = since.call(c['startedAt'])
    sent('loops.now.checking', { 'check' => c['command'], 'elapsed' => el }, "Checking `#{c['command']}` · #{el}")
  when 'waiting'
    nt = record['nextTick']
    at = ptime(nt['at'])
    time = clock(at, off)
    rs = nt['reason']
    if rs['kind'] == 'self_paced' && rs['reason']
      sent('loops.now.selfPaced', { 'time' => time, 'interval' => rs['interval'], 'reason' => rs['reason'] }, "Next tick #{time} — goose chose #{rs['interval']}: \"#{rs['reason']}\"")
    elsif rs['kind'] == 'self_paced'
      sent('loops.now.selfPacedNoReason', { 'time' => time, 'interval' => rs['interval'] }, "Next tick #{time} — goose chose #{rs['interval']} and gave no reason")
    elsif at <= now
      sent('loops.now.startsNow', {}, 'Next tick starts now')
    else
      rel = duration_words((at - now).to_i)
      sent('loops.now.nextAt', { 'time' => time, 'rel' => rel }, "Next tick #{time} · in #{rel}")
    end
  when 'waiting_turn'
    nx = nxt.to_s
    case r['kind']
    when 'user_turn' then sent('loops.now.dueAfterYourTurn', { 'next' => nx, 'chat' => r['chat'] }, "Tick #{nx} is due — it starts when your turn in \"#{r['chat']}\" ends")
    when 'refused'
      rf = r['refused']
      case rf['kind']
      when 'turn_running', 'queued_message' then sent('loops.now.dueAfterYourMessage', { 'next' => nx }, "Tick #{nx} is due — it starts after your message here")
      when 'pending_cancel' then sent('loops.now.dueAfterStop', { 'next' => nx }, "Tick #{nx} is due — it starts when the answer you stopped here has settled")
      when 'load_failed' then sent('loops.now.dueLoadFailed', { 'next' => nx, 'error' => rf['error'] }, "Tick #{nx} is due — this chat could not be opened in the window: #{rf['error']}")
      else sent('loops.now.dueSubmitFailed', { 'next' => nx, 'error' => rf['error'] }, "Tick #{nx} is due — goose refused its message: #{rf['error']}")
      end
    when 'reviewers' then sent('loops.now.dueAfterReviewers', { 'next' => nx, 'n' => r['n'].to_s }, "Tick #{nx} is due — it starts when goose's check of tick #{r['n']} ends")
    when 'way_held' then sent('loops.now.dueWayHeld', { 'next' => nx, 'node' => r['node'], 'chat' => r['chat'], 'target' => r['target'] }, "Tick #{nx} is due — #{r['node']} is answering you in \"#{r['chat']}\"; the tick loads #{r['target']} after")
    end
  when 'waiting_you'
    if r['kind'] == 'no_delay'
      sent('loops.now.noDelay', { 'n' => r['n'].to_s }, "Tick #{r['n']} didn't say when to come back.")
    else
      sent('loops.now.badDelay', { 'n' => r['n'].to_s, 'given' => r['given'] }, "Tick #{r['n']} named a delay goose can't read: \"#{r['given']}\".")
    end
  when 'needs_you'
    if r['kind'] == 'asked'
      sent('loops.now.asked', { 'n' => r['n'].to_s, 'question' => r['question'] }, "Tick #{r['n']} asked you: \"#{r['question']}\"")
    else
      sent('loops.now.answerRunning', { 'n' => r['n'].to_s }, "Your answer to tick #{r['n']} is running — the next tick starts after it")
    end
  when 'paused'
    case r['kind']
    when 'by_you'
      if r['afterTick'] == 0
        sent('loops.paused.byYouBeforeFirst', {}, 'Paused by you before the first tick.')
      else
        sent('loops.paused.byYou', { 'n' => r['afterTick'].to_s }, "Paused by you after tick #{r['afterTick']}.")
      end
    when 'you_stopped_tick' then sent('loops.paused.youStoppedTick', { 'n' => r['n'].to_s }, "You stopped tick #{r['n']}.")
    when 'blocked' then sent('loops.paused.blocked', { 'blockedOn' => r['blockedOn'] }, "Blocked — #{r['blockedOn']}")
    when 'check_could_not_run' then sent('loops.paused.checkCouldNotRun', { 'error' => r['error'] }, "The check could not run: #{r['error']}")
    when 'same_failure_twice' then sent('loops.paused.sameFailureTwice', { 'prev' => r['prev'].to_s, 'n' => r['n'].to_s, 'error' => r['error'] }, "Ticks #{r['prev']} and #{r['n']} failed the same way: #{r['error']}")
    when 'no_report_twice' then sent('loops.paused.noReportTwice', { 'prev' => r['prev'].to_s, 'n' => r['n'].to_s }, "Ticks #{r['prev']} and #{r['n']} ended without a loop report")
    when 'stalled' then sent('loops.paused.stalled', { 'prev' => r['prev'].to_s, 'n' => r['n'].to_s }, "Stalled — tick #{r['n']} named the same next step as tick #{r['prev']} and made no write or edit outside the state file")
    when 'closed'
      due = ticks_due(record, now)
      were = due == 1 ? '1 tick was due' : "#{due} ticks were due"
      if r['closedAt']
        time = hm.call(r['closedAt'])
        sent('loops.paused.closedAt', { 'time' => time, 'due' => due.to_s }, "goose was closed at #{time}; #{were}.")
      else
        sent('loops.paused.closed', { 'due' => due.to_s }, "goose was closed; #{were}.")
      end
    when 'finishing_elsewhere' then sent('loops.paused.finishingElsewhere', { 'n' => r['n'].to_s }, "Paused — tick #{r['n']} is finishing in the other window")
    end
  when 'ended'
    case r['kind']
    when 'goal_met' then sent('loops.ended.goalMet', { 'n' => r['n'].to_s, 'check' => r['check'] }, "Goal met — `#{r['check']}` passed after tick #{r['n']}")
    when 'reported_done' then sent('loops.ended.reportedDone', { 'n' => r['n'].to_s }, "goose reported the goal done after tick #{r['n']} — no check was set")
    when 'reached_count' then sent('loops.ended.reachedCount', { 'k' => r['k'].to_s }, "Reached #{r['k']} ticks, as you set")
    when 'stopped_by_you' then sent('loops.ended.stoppedByYou', { 'n' => r['n'].to_s }, "Stopped by you after tick #{r['n']}")
    end
  when 'elsewhere' then sent('loops.now.elsewhere', {}, 'This loop runs in another goose window.')
  end
end

# ---------------------------------------------------------------------------------------------
# Tick ids, ranges, steps, the state file, validation, `/loop`, the output tail
# ---------------------------------------------------------------------------------------------

def parse_tick_id(id)
  m = /\Alooptick_(lp_[0-9a-f]{8})_([1-9][0-9]*)_([0-9a-f-]+)\z/.match(id) or return nil
  { 'loopId' => m[1], 'n' => m[2].to_i, 'uuid' => m[3] }
end

def tick_ranges(ids, ticks)
  starts = ticks.map { |tk| ids.index(tk['firstMessageId']) }
  ticks.each_with_index.map do |tk, i|
    s = starts[i]
    if s.nil?
      { 'n' => tk['n'], 'range' => nil }
    else
      later = starts.compact.select { |o| o > s }
      { 'n' => tk['n'], 'range' => [s, later.empty? ? ids.size : later.min] }
    end
  end
end

KNOWN = %w[state_file check goal_first_line last_next_step working_dir].freeze
NO_CHECK = 'no check command is set; run the command that shows the change works and quote it'

def step_slots(steps)
  steps.scan(/\{([a-z][a-z0-9_]*)\}/).flatten.uniq
end

def render_steps(steps, f)
  unknown = []
  text = steps.gsub(/\{([a-z][a-z0-9_]*)\}/) do
    name = Regexp.last_match(1)
    case name
    when 'state_file' then "`#{f['stateFile']}`"
    when 'working_dir' then "`#{f['workingDir']}`"
    when 'goal_first_line' then "\"#{f['goalFirstLine']}\""
    when 'check' then f['check'] ? "`#{f['check']}`" : NO_CHECK
    when 'last_next_step'
      l = f['lastNextStep']
      case l['kind']
      when 'first' then 'this is the first tick'
      when 'named_none' then "tick #{l['prev']} named no next step"
      else "\"#{l['text']}\""
      end
    else
      unknown << name unless unknown.include?(name)
      "{#{name}}"
    end
  end
  { 'text' => text, 'unknown' => unknown }
end

def first_line(goal)
  (goal.strip.lines.first || '').strip
end

def default_state_file(goal)
  words = first_line(goal).downcase.split(/[^[:alnum:]]+/).reject(&:empty?).first(4)
  ".goose/loops/#{words.empty? ? 'loop' : words.join('-')}/NOW.md"
end

def rel_state_file(path, wd)
  p = path.strip
  if p.start_with?('/')
    dir = wd.sub(%r{/+\z}, '')
    return :outside if dir.empty?
    return :outside unless p == dir || p.start_with?(dir + '/')
    p = p[dir.size..-1]
  end
  parts = []
  p.split('/').each do |seg|
    next if seg.empty? || seg == '.'
    if seg == '..'
      return :outside if parts.empty?
      parts.pop
    else
      parts << seg
    end
  end
  return :empty if parts.empty?
  parts.join('/')
end

def refusal(code, reason)
  { 'refusal' => { 'code' => code, 'reason' => reason } }
end

def validate(edit, chat)
  return refusal('swarm_build', 'Loops run chat turns. This chat builds with the swarm, so every tick would start a full build. Use Agent Work for recurring builds.') if chat['swarmBuild']
  return refusal('empty_goal', 'Say what the loop should do.') if edit['goal'].strip.empty?
  check = edit['check'] && !edit['check'].strip.empty? ? edit['check'].strip : nil
  return refusal('check_required', 'This template needs a command to check.') if edit['template'] == 'until_check' && check.nil?
  if edit['cadence']['kind'] == 'every' && cadence_secs(edit['cadence']['every']).nil?
    return refusal('bad_cadence', 'Use a number and s, m or h — 90m, 2h')
  end
  sf = rel_state_file(edit['stateFile'], chat['workingDir'])
  return refusal('state_file_outside', "Keep the state file inside #{chat['workingDir']}.") if sf == :outside
  return refusal('empty_state_file', 'Name the state file — goose reads it first and rewrites it last.') if sf == :empty
  unknown = step_slots(edit['steps'] || '').find { |s| !KNOWN.include?(s) }
  return refusal('unknown_slot', "{#{unknown}} is not a fact goose knows") if unknown
  if edit['stopAfterTicks'] == 0
    return refusal('bad_stop_after', 'Stop after needs at least one tick — leave it empty to run until the goal is met or you stop it.')
  end
  ok = { 'goal' => edit['goal'], 'template' => edit['template'], 'steps' => edit['steps'] || '',
         'cadence' => edit['cadence']['kind'] == 'every' ? every(edit['cadence']['every'].strip) : edit['cadence'],
         'stateFile' => sf }
  ok['check'] = check if check
  ok['stopAfterTicks'] = edit['stopAfterTicks'] if edit['stopAfterTicks']
  { 'ok' => ok }
end

def parse_loop_command(line)
  l = line.strip
  return nil unless l.start_with?('/loop')
  rest = l[5..-1]
  return nil unless rest.empty? || rest =~ /\A\s/
  rest = rest.strip
  return { 'kind' => 'status' } if rest.empty?
  word, after = rest.split(/\s+/, 2)
  after = (after || '').strip
  lower = word.downcase
  if %w[now pause resume stop].include?(lower)
    return { 'kind' => lower } if after.empty?
    return { 'kind' => 'refused', 'code' => 'control_takes_no_words',
             'reason' => "/loop #{lower} takes nothing after it — to loop on a goal that starts with \"#{word}\", use the Loop button." }
  end
  if lower == 'every'
    ev, goal = after.split(/\s+/, 2)
    goal = (goal || '').strip
    return { 'kind' => 'refused', 'code' => 'missing_cadence', 'reason' => 'Say how often and what: /loop every 10m <goal>.' } if ev.nil? || ev.empty?
    return { 'kind' => 'refused', 'code' => 'bad_cadence', 'reason' => 'Use a number and s, m or h — 90m, 2h' } if cadence_secs(ev).nil?
    return { 'kind' => 'refused', 'code' => 'missing_goal', 'reason' => "Say what the loop should do: /loop every #{ev} <goal>." } if goal.empty?
    return { 'kind' => 'start', 'goal' => goal, 'cadence' => every(ev) }
  end
  { 'kind' => 'start', 'goal' => rest, 'cadence' => SELF }
end

def words(s)
  s.split.size
end

def output_tail(output, window)
  budget = window / 64
  chars = output.chars
  (0..chars.size).each do |i|
    tail = chars[i..-1].join
    return tail if words(tail) <= budget
  end
end

# ---------------------------------------------------------------------------------------------
# The tick prompt (§4.4)
# ---------------------------------------------------------------------------------------------

OUTCOME_WORDS = { 'progress' => 'progress', 'done' => 'done', 'blocked' => 'blocked', 'asked' => 'asked the user',
                  'failed' => 'failed', 'no_report' => 'no report', 'yielded' => 'yielded',
                  'stopped_by_you' => 'stopped by the user' }.freeze

def prompt(record, n, facts)
  off = facts['utcOffsetMinutes']
  hm = ->(s) { clock(ptime(s), off) }
  prev = record['ticks'].last
  lines = []
  head = "Loop tick #{n} — \"#{first_line(record['goal'])}\" · #{cadence_label(record['cadence'])}"
  head += " · stop after #{record['stopAfterTicks']} ticks" if record['stopAfterTicks']
  lines << head
  lines << "Your goal (the user's words):"
  lines << record['goal'].strip
  lines << "State file: #{record['stateFile']} — read it before anything else; rewrite it before you call loop_report"
  lines << '(Now · Next · Found · Done; keep it short enough to read in one go).'
  last_step =
    if prev.nil? then { 'kind' => 'first' }
    elsif prev['report'] then { 'kind' => 'named', 'text' => prev['report']['nextStep'] }
    else { 'kind' => 'named_none', 'prev' => prev['n'] }
    end
  sf = { 'stateFile' => record['stateFile'], 'goalFirstLine' => first_line(record['goal']),
         'lastNextStep' => last_step, 'workingDir' => facts['workingDir'] }
  sf['check'] = record['check'] if record['check']
  steps = render_steps(record['steps'], sf)['text']
  unless steps.strip.empty?
    lines << "What each tick does (the user's steps, as they left them in the dialog):"
    lines << steps.strip
  end
  if prev
    p = prev['n']
    oc = prev['outcome']
    last = "Last tick (#{p}, #{hm.call(prev['startedAt'])}, #{oc ? OUTCOME_WORDS[oc['kind']] : 'not ended'})"
    if prev['report']
      last += ": \"#{prev['report']['summary'].strip}\" — next step it named: \"#{prev['report']['nextStep'].strip}\""
    elsif oc && oc['kind'] == 'failed'
      last += ": #{oc['error'].strip}"
    elsif oc && oc['kind'] == 'no_report'
      last += ': it ended without calling loop_report.'
    else
      last += '.'
    end
    lines << last
    if prev['report'] && prev['report']['verdict'] == 'blocked' && prev['report']['blockedOn']
      lines << "Tick #{p} was blocked on: \"#{prev['report']['blockedOn'].strip}\""
    end
    c = prev['check']
    if c
      if c['ran']
        if prev['report'] && prev['report']['verdict'] == 'done' && c['exit'] != 0
          lines << (c['exit'].nil? ? "You reported the goal done in tick #{p}; `#{c['command']}` ended without an exit status." : "You reported the goal done in tick #{p}; `#{c['command']}` exited #{c['exit']}.")
        end
        result = c['exit'] == 0 ? 'passed' : (c['exit'].nil? ? 'ended without an exit status' : "exited #{c['exit']}")
        tail = c['outputTail'].rstrip
        if tail.empty?
          lines << "Check `#{c['command']}` after tick #{p}: #{result}. Its output was empty."
        else
          lines << "Check `#{c['command']}` after tick #{p}: #{result}. Its output ended with:"
          lines << tail
        end
      else
        lines << "Check `#{c['command']}` could not run after tick #{p}: #{c['error'] || 'it did not start and named no error'}."
      end
    end
    if oc && oc['kind'] == 'yielded'
      lines << "Tick #{p} was stopped at #{hm.call(prev['endedAt'])} for the user's turn in \"#{oc['toChat']}\"; its partial work is above."
    elsif oc && oc['kind'] == 'asked'
      res = { 'answered' => 'their answer is above', 'dismissed' => 'they dismissed it' }[facts['askedResolution']] || 'it is still open'
      lines << "Tick #{p} asked the user \"#{oc['question']}\"; #{res}."
    end
  end
  lines << 'Say when to come back: next_in ("10m", "2h") and why.' if record['cadence']['kind'] == 'self_paced'
  lines << 'Finish by calling loop_report; calling it ends this tick.'
  lines.join("\n")
end

# ---------------------------------------------------------------------------------------------
# The cases
# ---------------------------------------------------------------------------------------------

fixture = {}

fixture['cadences'] = ['10m', '30m', '1h', '90s', ' 10m ', '10 m', '+5m', '0m', '-5m', 'soon', 'm', '', '1.5h', '10d',
                       '10é', 'é', '999999999999999m', '99999999999999999999s', '9223372036854775s',
                       '9223372036854776s'].map do |text|
  { 'text' => text, 'seconds' => cadence_secs(text), 'label' => cadence_label(every(text)) }
end
fixture['cadenceLabels'] = [SELF, B2B, every('2h'), every('45s')].map { |c| { 'cadence' => c, 'label' => cadence_label(c) } }
fixture['durationWords'] = [0, 5, 59, 60, 72, 372, 2460, 3600, 3900, 7322, -3].map { |s| { 'seconds' => s, 'words' => duration_words(s) } }

served = { 'node' => '27B · both Macs', 'rank' => 1, 'tried' => [], 'atMs' => 1_790_000_000_000 }
full = rec(
  'check' => CHECK, 'stopAfterTicks' => 10, 'status' => 'running',
  'offer' => { 'n' => 3, 'messageId' => tick_id(3), 'offeredAt' => iso(t('22:20')) },
  'owner' => { 'goosedPid' => 4321, 'goosedStartedAt' => 1_790_000_000, 'appPid' => 4300 },
  'ticks' => [
    tick(1, t('22:00'), t('22:07', 30), 'report' => report('progress', 'Read kickoff.md; listed 6 problem classes.', 'scaffold the generator'),
                                          'outcome' => { 'kind' => 'progress' }, 'wrote' => ['scripts/generate_users.js'],
                                          'check' => check_run(CHECK, t('22:07', 31), true, 1, 'missing svc- accounts', 'endedAt' => iso(t('22:07', 40)), 'logPath' => '/Users/mihai/.local/share/goose/loops/lp_0a1b2c3d/check-1.log'),
                                          'served' => served, 'tokens' => { 'input' => 41_000, 'output' => 3_100, 'total' => 44_100 }),
    tick(2, t('22:10'), t('22:11'), 'outcome' => { 'kind' => 'failed', 'errorClass' => 'provider', 'error' => 'stream ended early' }),
    tick(3, t('22:20'), nil, 'origin' => 'after_your_turn')
  ]
)
fixture['records'] = [
  { 'name' => 'a fresh loop, no ticks', 'record' => rec },
  { 'name' => 'every field set', 'record' => full },
  { 'name' => 'ended by the check', 'record' => rec('status' => 'ended', 'statusReason' => { 'kind' => 'goal_met', 'n' => 4, 'check' => CHECK }, 'endedAt' => iso(t('23:00'))) },
  { 'name' => 'waiting, self-paced', 'record' => rec('cadence' => SELF, 'template' => 'blank', 'steps' => '', 'nextTick' => { 'at' => iso(t('22:30')), 'reason' => { 'kind' => 'self_paced', 'interval' => '15m', 'reason' => 'CI takes 12 minutes' } }) },
  { 'name' => 'every outcome kind', 'record' => rec('ticks' => [
    tick(1, t('20:00'), t('20:01'), 'outcome' => { 'kind' => 'asked', 'itemId' => 'ny_1', 'question' => 'Comma or semicolon?' }),
    tick(2, t('20:02'), t('20:03'), 'outcome' => { 'kind' => 'yielded', 'toSession' => 's2', 'toChat' => 'Kickoff notes', 'way' => 'tensor:local+work' }),
    tick(3, t('20:04'), t('20:05'), 'outcome' => { 'kind' => 'stopped_by_you' }),
    tick(4, t('20:06'), t('20:07'), 'outcome' => { 'kind' => 'no_report' }),
    tick(5, t('20:08'), t('20:09'), 'report' => report('blocked', 'Need the delimiter.', 'ask', 'blockedOn' => 'the CSV delimiter'), 'outcome' => { 'kind' => 'blocked' }),
    tick(6, t('20:10'), t('20:11'), 'report' => report('done', 'All classes present.', 'none', 'nextIn' => '5m', 'nextReason' => 'confirm'), 'outcome' => { 'kind' => 'done' }),
    tick(7, t('20:12'), t('20:13'), 'check' => check_run(CHECK, t('20:13'), false, nil, '', 'error' => 'sh: node: command not found', 'endedAt' => iso(t('20:13'))))
  ]) }
]

# nextTick
nt_cases = []
nt_cases << ['no tick yet: the first tick, now', rec, t('22:00'), false]
nt_cases << ['every: previous start plus the cadence', rec('ticks' => [tick(1, t('22:00'), t('22:03'))]), t('22:03'), false]
nt_cases << ['every: reviewers pending does not matter while the time is ahead', rec('ticks' => [tick(1, t('22:00'), t('22:03'))]), t('22:03'), true]
nt_cases << ['every: overdue, the tick overran the cadence', rec('ticks' => [tick(1, t('22:00'), t('22:12'))]), t('22:12'), false]
nt_cases << ['every: overdue but the reviewers of tick 1 still run', rec('ticks' => [tick(1, t('22:00'), t('22:12'))]), t('22:12'), true]
nt_cases << ['every: exactly on the cadence is due now', rec('ticks' => [tick(1, t('22:00'), t('22:04'))]), t('22:10'), false]
sp = ->(rep) { rec('cadence' => SELF, 'ticks' => [tick(1, t('22:00'), t('22:05'), 'report' => rep)]) }
nt_cases << ['self-paced: the delay the tick named, from its end', sp.call(report('progress', 's', 'n', 'nextIn' => '15m', 'nextReason' => 'CI takes 12 minutes')), t('22:05'), false]
nt_cases << ['self-paced: a delay with no reason', sp.call(report('progress', 's', 'n', 'nextIn' => ' 2h ')), t('22:05'), false]
nt_cases << ['self-paced: no delay named', sp.call(report('progress', 's', 'n')), t('22:05'), false]
nt_cases << ['self-paced: a blank delay', sp.call(report('progress', 's', 'n', 'nextIn' => '  ')), t('22:05'), false]
nt_cases << ['self-paced: a delay outside the grammar', sp.call(report('progress', 's', 'n', 'nextIn' => 'soon')), t('22:05'), false]
nt_cases << ['self-paced: no report at all', rec('cadence' => SELF, 'ticks' => [tick(1, t('22:00'), t('22:05'))]), t('22:05'), false]
nt_cases << ['self-paced: the delay already passed, due now', sp.call(report('progress', 's', 'n', 'nextIn' => '1m')), t('22:30'), false]
nt_cases << ['self-paced: due now with reviewers pending', sp.call(report('progress', 's', 'n', 'nextIn' => '1m')), t('22:30'), true]
nt_cases << ['back to back: now', rec('cadence' => B2B, 'ticks' => [tick(1, t('22:00'), t('22:05'))]), t('22:05'), false]
nt_cases << ['back to back: after the reviewers', rec('cadence' => B2B, 'ticks' => [tick(1, t('22:00'), t('22:05'))]), t('22:05'), true]
fixture['nextTick'] = nt_cases.map do |name, r, now, rv|
  { 'name' => name, 'record' => r, 'now' => iso(now), 'reviewersPending' => rv, 'expect' => next_tick(r, now, rv) }
end
fixture['nextTickErrors'] = [
  { 'name' => 'the last tick has not ended', 'record' => rec('ticks' => [tick(1, t('22:00'), nil)]), 'now' => iso(t('22:05')), 'reviewersPending' => false },
  { 'name' => 'a cadence outside the grammar', 'record' => rec('cadence' => every('soon'), 'ticks' => [tick(1, t('22:00'), t('22:01'))]), 'now' => iso(t('22:05')), 'reviewersPending' => false },
  { 'name' => 'a cadence past the last date goose can hold', 'record' => rec('cadence' => every('9223372036854775s'), 'ticks' => [tick(1, t('22:00'), t('22:01'))]), 'now' => iso(t('22:05')), 'reviewersPending' => false }
]

# decide
completed = { 'kind' => 'completed' }
dc = []
t3 = ->(opts) { tick(3, t('22:20'), t('22:26'), opts) }
prog = ->(step = 'add svc- accounts', extra = {}) { report('progress', 'Added case-only duplicate emails.', step, extra) }
two = ->(r2, r3, recopts = {}) { rec({ 'ticks' => [tick(2, t('22:10'), t('22:15'), r2), t3.call(r3)] }.merge(recopts)) }
passed = check_run(CHECK, t('22:26'), true, 0, 'all 7 classes present', 'endedAt' => iso(t('22:26', 20)))
failed1 = check_run(CHECK, t('22:26'), true, 1, 'missing svc- accounts', 'endedAt' => iso(t('22:26', 20)))
now = iso(t('22:26', 30))
f = ->(extra = {}) { { 'end' => completed, 'reviewersPending' => false, 'now' => now }.merge(extra) }

dc << ['goal met: the check passed after a progress tick', rec('check' => CHECK, 'ticks' => [t3.call('report' => prog.call, 'wrote' => ['scripts/generate_users.js'])]), f.call('check' => passed)]
dc << ['goal met: the check passed after a done tick', rec('check' => CHECK, 'ticks' => [t3.call('report' => report('done', 'All classes.', 'none'))]), f.call('check' => passed)]
dc << ['reported done, no check set: the model\'s own claim ends it', rec('ticks' => [t3.call('report' => report('done', 'All classes.', 'none'))]), f.call]
dc << ['reported done, the check fails: the loop continues', rec('check' => CHECK, 'ticks' => [t3.call('report' => report('done', 'All classes.', 'none'), 'wrote' => ['a.js'])]), f.call('check' => failed1)]
dc << ['the check could not run: paused, never read as failed', rec('check' => CHECK, 'ticks' => [t3.call('report' => prog.call, 'wrote' => ['a.js'])]), f.call('check' => check_run(CHECK, t('22:26'), false, nil, '', 'error' => 'sh: node: command not found'))]
dc << ['the check was stopped by the user: paused with that reason', rec('check' => CHECK, 'ticks' => [t3.call('report' => prog.call, 'wrote' => ['a.js'])]), f.call('check' => check_run(CHECK, t('22:26'), false, nil, 'partial', 'error' => 'stopped by you'))]
dc << ['blocked: paused on what only the user can decide', rec('check' => CHECK, 'ticks' => [t3.call('report' => report('blocked', 'Two formats fit.', 'decide', 'blockedOn' => 'which CSV delimiter the owner wants'))]), f.call]
dc << ['blocked without saying on what: the absence is named', rec('ticks' => [t3.call('report' => report('blocked', 'Stuck.', 'decide', 'blockedOn' => '  '))]), f.call]
dc << ['the tick asked you: needs you, the report is kept, the check does not run', rec('check' => CHECK, 'ticks' => [t3.call('report' => prog.call)]), f.call('asked' => { 'itemId' => 'ny_7f', 'question' => 'Comma or semicolon?' })]
dc << ['the tick asked you without a report', rec('ticks' => [t3.call({})]), f.call('asked' => { 'itemId' => 'ny_7f', 'question' => 'Comma or semicolon?' })]
dc << ['the user\'s count reached', rec('stopAfterTicks' => 3, 'ticks' => [t3.call('report' => prog.call, 'wrote' => ['a.js'])]), f.call]
dc << ['the user\'s count reached on a tick that asked', rec('stopAfterTicks' => 3, 'ticks' => [t3.call({})]), f.call('asked' => { 'itemId' => 'ny_1', 'question' => 'Which?' })]
dc << ['the count not reached yet', rec('stopAfterTicks' => 4, 'ticks' => [t3.call('report' => prog.call, 'wrote' => ['a.js'])]), f.call]
dc << ['Stop loop: ended, the running tick stopped by you', rec('ticks' => [t3.call({})]), f.call('end' => { 'kind' => 'cancelled', 'cause' => { 'kind' => 'loop_stopped' } })]
dc << ['the user stopped the tick: paused, never fires into a stopped chat', rec('ticks' => [t3.call({})]), f.call('end' => { 'kind' => 'cancelled' })]
dc << ['yielded to a user turn: waits for that turn, not a pause', rec('ticks' => [t3.call({})]), f.call('end' => { 'kind' => 'cancelled', 'cause' => { 'kind' => 'yield', 'toSession' => '20260927_8', 'toChat' => 'Kickoff notes' } })]
dc << ['yielded with the way known', rec('ticks' => [t3.call({})]), f.call('end' => { 'kind' => 'cancelled', 'cause' => { 'kind' => 'yield', 'toSession' => 's9', 'toChat' => 'Ops', 'way' => 'single:local' } })]
dc << ['the same failure twice: paused', two.call({ 'outcome' => { 'kind' => 'failed', 'errorClass' => 'provider', 'error' => 'stream ended early' } }, {}), f.call('end' => { 'kind' => 'errored', 'errorClass' => 'provider', 'error' => 'stream ended early again' })]
dc << ['two failures of different classes: continue', two.call({ 'outcome' => { 'kind' => 'failed', 'errorClass' => 'context', 'error' => 'too long' } }, {}), f.call('end' => { 'kind' => 'errored', 'errorClass' => 'provider', 'error' => 'stream ended early' })]
dc << ['one failure: continue, the next prompt carries it', rec('ticks' => [t3.call({})]), f.call('end' => { 'kind' => 'errored', 'errorClass' => 'provider', 'error' => 'stream ended early' })]
dc << ['no report twice: paused', two.call({ 'outcome' => { 'kind' => 'no_report' } }, {}), f.call]
dc << ['no report once: continue', rec('ticks' => [t3.call({})]), f.call]
dc << ['a yield then no report: a yield never counts toward no report twice', two.call({ 'outcome' => { 'kind' => 'yielded', 'toSession' => 's', 'toChat' => 'c' } }, {}), f.call]
dc << ['stalled: same next step, no write or edit outside the state file', two.call({ 'report' => prog.call('Add svc- accounts'), 'outcome' => { 'kind' => 'progress' }, 'wrote' => ['a.js'] }, { 'report' => prog.call(" add  SVC- accounts\n") }), f.call]
dc << ['not stalled: it wrote another file', two.call({ 'report' => prog.call('Add svc- accounts'), 'outcome' => { 'kind' => 'progress' } }, { 'report' => prog.call('add svc- accounts'), 'wrote' => ['scripts/generate_users.js'] }), f.call]
dc << ['not stalled: a different next step', two.call({ 'report' => prog.call('Add svc- accounts'), 'outcome' => { 'kind' => 'progress' } }, { 'report' => prog.call('seed no-email rows') }), f.call]
dc << ['not stalled: the previous tick had no report', two.call({ 'outcome' => { 'kind' => 'no_report' } }, { 'report' => prog.call }), f.call]
dc << ['stalled with a failing check still pauses', two.call({ 'report' => prog.call, 'outcome' => { 'kind' => 'progress' } }, { 'report' => prog.call }, 'check' => CHECK), f.call('check' => failed1)]
dc << ['self-paced: the next tick when goose said', rec('cadence' => SELF, 'ticks' => [t3.call('report' => prog.call('x', 'nextIn' => '20m', 'nextReason' => 'the deploy takes 15 minutes'), 'wrote' => ['a.js'])]), f.call]
dc << ['self-paced: no delay named, waiting for you', rec('cadence' => SELF, 'ticks' => [t3.call('report' => prog.call, 'wrote' => ['a.js'])]), f.call]
dc << ['self-paced: a delay goose cannot read', rec('cadence' => SELF, 'ticks' => [t3.call('report' => prog.call('x', 'nextIn' => 'after lunch'), 'wrote' => ['a.js'])]), f.call]
dc << ['self-paced: a failed tick named no delay', rec('cadence' => SELF, 'ticks' => [t3.call({})]), f.call('end' => { 'kind' => 'errored', 'errorClass' => 'provider', 'error' => 'boom' })]
dc << ['back to back: at once', rec('cadence' => B2B, 'ticks' => [t3.call('report' => prog.call, 'wrote' => ['a.js'])]), f.call]
dc << ['back to back: after the reviewers of this tick', rec('cadence' => B2B, 'ticks' => [t3.call('report' => prog.call, 'wrote' => ['a.js'])]), f.call('reviewersPending' => true)]
dc << ['every: the tick overran, due now', rec('ticks' => [tick(3, t('22:00'), t('22:14'), 'report' => prog.call, 'wrote' => ['a.js'])]), f.call('now' => iso(t('22:14')))]
dc << ['every: overran and the reviewers still run', rec('ticks' => [tick(3, t('22:00'), t('22:14'), 'report' => prog.call, 'wrote' => ['a.js'])]), f.call('now' => iso(t('22:14')), 'reviewersPending' => true)]
fixture['decide'] = dc.map { |name, r, fa| { 'name' => name, 'record' => r, 'facts' => fa, 'expect' => decide(r, fa) } }
fixture['decideErrors'] = [
  { 'name' => 'a check is set but no run was recorded after a progress tick', 'record' => rec('check' => CHECK, 'ticks' => [t3.call('report' => prog.call)]), 'facts' => f.call },
  { 'name' => 'no tick to decide on', 'record' => rec, 'facts' => f.call }
]
fixture['decide'].each { |c| raise "decide error in #{c['name']}" if c['expect']['error'] }

# stalled
fixture['stalled'] = [
  ['same step, nothing written', prog.call('Add svc- accounts'), prog.call('add svc- ACCOUNTS'), []],
  ['same step, a file written', prog.call('Add svc- accounts'), prog.call('Add svc- accounts'), ['x.js']],
  ['different step', prog.call('Add svc- accounts'), prog.call('Add no-email rows'), []],
  ['current verdict done', prog.call('a'), report('done', 's', 'a'), []],
  ['previous without a report', nil, prog.call('a'), []]
].map do |name, pr, cr, wrote|
  prev = tick(1, t('22:00'), t('22:01'))
  prev['report'] = pr if pr
  cur = tick(2, t('22:10'), t('22:11'), 'report' => cr, 'wrote' => wrote)
  { 'name' => name, 'prev' => prev, 'cur' => cur, 'expect' => stalled?(prev, cur) }
end

# effective status
owner = { 'goosedPid' => 4321, 'goosedStartedAt' => 1_790_000_000, 'appPid' => 4300 }
fixture['effective'] = [
  ['paused stays paused whatever the owner', rec('status' => 'paused', 'statusReason' => { 'kind' => 'by_you', 'afterTick' => 2 }, 'owner' => owner), { 'kind' => 'gone', 'why' => 'no process has pid 4321' }],
  ['ended stays ended', rec('status' => 'ended', 'statusReason' => { 'kind' => 'reached_count', 'k' => 5 }), nil],
  ['waiting with no owner reads closed', rec, nil],
  ['waiting, the owner proven gone', rec('owner' => owner), { 'kind' => 'gone', 'why' => 'pid 4321 was reparented to launchd' }],
  ['running, another live window owns it', rec('status' => 'running', 'owner' => owner), { 'kind' => 'live' }],
  ['waiting, this process owns it', rec('owner' => owner), { 'kind' => 'this_process' }],
  ['needs you, unproven: the written status stands', rec('status' => 'needs_you', 'statusReason' => { 'kind' => 'asked', 'n' => 2, 'itemId' => 'ny_1', 'question' => 'Q?' }, 'owner' => owner), { 'kind' => 'unproven', 'why' => 'The loop runner is not in this build' }]
].map { |name, r, proof| { 'name' => name, 'record' => r, 'proof' => proof, 'expect' => effective(r, proof).reject { |_, v| v.nil? } } }

# sentences
snow = t('22:45')
ended_tick = tick(3, t('22:20'), t('22:26'))
sc = []
sc << ['running', rec('status' => 'running', 'ticks' => [tick(3, t('22:40'), nil)]), 'running', nil, 0]
sc << ['running on a node', rec('status' => 'running', 'ticks' => [tick(3, t('22:43', 48), nil, 'served' => served)]), 'running', nil, 0]
sc << ['running, shown at UTC+3', rec('status' => 'running', 'ticks' => [tick(3, t('22:40'), nil)]), 'running', nil, 180]
sc << ['checking', rec('status' => 'checking', 'check' => CHECK, 'ticks' => [tick(3, t('22:30'), t('22:44'), 'check' => check_run(CHECK, t('22:44'), true, nil, ''))]), 'checking', nil, 0]
sc << ['waiting, next at a time', rec('nextTick' => { 'at' => iso(t('22:53')), 'reason' => { 'kind' => 'cadence' } }, 'ticks' => [ended_tick]), 'waiting', nil, 0]
sc << ['waiting, due now', rec('nextTick' => { 'at' => iso(t('22:40')), 'reason' => { 'kind' => 'overdue' } }, 'ticks' => [ended_tick]), 'waiting', nil, 0]
sc << ['waiting, self-paced with its reason', rec('cadence' => SELF, 'nextTick' => { 'at' => iso(t('23:05')), 'reason' => { 'kind' => 'self_paced', 'interval' => '20m', 'reason' => 'the deploy takes 15 minutes' } }), 'waiting', nil, 0]
sc << ['waiting, self-paced without a reason', rec('cadence' => SELF, 'nextTick' => { 'at' => iso(t('23:05')), 'reason' => { 'kind' => 'self_paced', 'interval' => '20m' } }), 'waiting', nil, 0]
sc << ['waiting for your turn', rec('status' => 'waiting_turn', 'ticks' => [ended_tick]), 'waiting_turn', { 'kind' => 'user_turn', 'sessionId' => 's2', 'chat' => 'Kickoff notes' }, 0]
%w[turn_running queued_message pending_cancel].each do |k|
  sc << ["waiting, the chat refused: #{k}", rec('status' => 'waiting_turn', 'ticks' => [ended_tick]), 'waiting_turn', { 'kind' => 'refused', 'refused' => { 'kind' => k } }, 0]
end
sc << ['waiting, the chat could not load', rec('status' => 'waiting_turn'), 'waiting_turn', { 'kind' => 'refused', 'refused' => { 'kind' => 'load_failed', 'error' => 'session not found' } }, 0]
sc << ['waiting, the prompt was refused', rec('status' => 'waiting_turn', 'ticks' => [ended_tick]), 'waiting_turn', { 'kind' => 'refused', 'refused' => { 'kind' => 'submit_failed', 'error' => 'session is busy in another run' } }, 0]
sc << ['waiting for the reviewers', rec('status' => 'waiting_turn', 'ticks' => [ended_tick]), 'waiting_turn', { 'kind' => 'reviewers', 'n' => 3 }, 0]
sc << ['waiting, the way is held', rec('status' => 'waiting_turn', 'ticks' => [ended_tick]), 'waiting_turn', { 'kind' => 'way_held', 'node' => '27B', 'chat' => 'Ops', 'target' => '8B' }, 0]
sc << ['waiting for you: no delay', rec('status' => 'waiting_you', 'cadence' => SELF), 'waiting_you', { 'kind' => 'no_delay', 'n' => 3 }, 0]
sc << ['waiting for you: a bad delay', rec('status' => 'waiting_you', 'cadence' => SELF), 'waiting_you', { 'kind' => 'bad_delay', 'n' => 3, 'given' => 'after lunch' }, 0]
sc << ['needs you: asked', rec('status' => 'needs_you'), 'needs_you', { 'kind' => 'asked', 'n' => 3, 'itemId' => 'ny_1', 'question' => 'Comma or semicolon?' }, 0]
sc << ['needs you: the answer runs', rec('status' => 'needs_you'), 'needs_you', { 'kind' => 'answer_running', 'n' => 3 }, 0]
sc << ['paused by you', rec('status' => 'paused'), 'paused', { 'kind' => 'by_you', 'afterTick' => 3 }, 0]
sc << ['paused by you before the first tick', rec('status' => 'paused'), 'paused', { 'kind' => 'by_you', 'afterTick' => 0 }, 0]
sc << ['you stopped a tick', rec('status' => 'paused'), 'paused', { 'kind' => 'you_stopped_tick', 'n' => 6 }, 0]
sc << ['blocked', rec('status' => 'paused'), 'paused', { 'kind' => 'blocked', 'n' => 3, 'blockedOn' => 'which CSV delimiter the owner wants' }, 0]
sc << ['the check could not run', rec('status' => 'paused'), 'paused', { 'kind' => 'check_could_not_run', 'n' => 3, 'error' => 'sh: node: command not found' }, 0]
sc << ['the same failure twice', rec('status' => 'paused'), 'paused', { 'kind' => 'same_failure_twice', 'prev' => 2, 'n' => 3, 'error' => 'stream ended early' }, 0]
sc << ['no report twice', rec('status' => 'paused'), 'paused', { 'kind' => 'no_report_twice', 'prev' => 2, 'n' => 3 }, 0]
sc << ['stalled', rec('status' => 'paused'), 'paused', { 'kind' => 'stalled', 'prev' => 6, 'n' => 7 }, 0]
sc << ['closed at a time, four ticks due', rec('status' => 'paused', 'nextTick' => { 'at' => iso(t('22:10')), 'reason' => { 'kind' => 'cadence' } }), 'paused', { 'kind' => 'closed', 'closedAt' => iso(t('22:05')) }, 0]
sc << ['closed, proven gone, one tick due', rec('status' => 'paused', 'cadence' => SELF, 'nextTick' => { 'at' => iso(t('22:10')), 'reason' => { 'kind' => 'self_paced', 'interval' => '5m' } }), 'paused', { 'kind' => 'closed' }, 0]
sc << ['closed before any tick came due', rec('status' => 'paused', 'nextTick' => { 'at' => iso(t('23:10')), 'reason' => { 'kind' => 'cadence' } }), 'paused', { 'kind' => 'closed' }, 0]
sc << ['closed with no next tick', rec('status' => 'paused'), 'paused', { 'kind' => 'closed' }, 0]
sc << ['finishing in the other window', rec('status' => 'paused'), 'paused', { 'kind' => 'finishing_elsewhere', 'n' => 4 }, 0]
sc << ['goal met', rec('status' => 'ended'), 'ended', { 'kind' => 'goal_met', 'n' => 5, 'check' => CHECK }, 0]
sc << ['reported done', rec('status' => 'ended'), 'ended', { 'kind' => 'reported_done', 'n' => 5 }, 0]
sc << ['reached the count', rec('status' => 'ended'), 'ended', { 'kind' => 'reached_count', 'k' => 5 }, 0]
sc << ['stopped by you', rec('status' => 'ended'), 'ended', { 'kind' => 'stopped_by_you', 'n' => 5 }, 0]
sc << ['in another window', rec('status' => 'waiting'), 'elsewhere', nil, 0]
fixture['sentences'] = sc.map do |name, r, st, rs, off|
  c = { 'name' => name, 'record' => r, 'status' => st, 'now' => iso(snow), 'utcOffsetMinutes' => off, 'expect' => sentence(r, st, rs, snow, off) }
  c['reason'] = rs if rs
  c
end
fixture['sentenceErrors'] = [
  { 'name' => 'paused with no reason', 'record' => rec('status' => 'paused'), 'status' => 'paused', 'now' => iso(snow), 'utcOffsetMinutes' => 0 },
  { 'name' => 'ended with a paused reason', 'record' => rec('status' => 'ended'), 'status' => 'ended', 'reason' => { 'kind' => 'by_you', 'afterTick' => 1 }, 'now' => iso(snow), 'utcOffsetMinutes' => 0 },
  { 'name' => 'waiting with no next tick', 'record' => rec, 'status' => 'waiting', 'now' => iso(snow), 'utcOffsetMinutes' => 0 }
]

# tick ids
fixture['tickIds'] = [
  tick_id(3), "looptick_#{LOOP_ID}_12_5f0c6a2e-9d3b-4c1a-8e7f-6d5c4b3a2918", "looptick_#{LOOP_ID}_0_abc",
  "looptick_#{LOOP_ID}_03_abc", 'looptick_lp_0A1B2C3D_3_abc', 'looptick_lp_0a1b2c3_3_abc', "looptick_#{LOOP_ID}_3_",
  "looptick_#{LOOP_ID}_3", "looptick_#{LOOP_ID}_x_abc", "looptick_#{LOOP_ID}_3_ABC", 'steer_5f0c6a2e', 'msg_123',
  "#{tick_id(3)} ", "looptick__3_abc", "looptick_#{LOOP_ID}_3_abc_def"
].map { |id| { 'id' => id, 'expect' => parse_tick_id(id) } }
fixture['tickIdMint'] = [[LOOP_ID, 1, UUIDS[0]], ['lp_ffffffff', 42, UUIDS[1]]].map do |l, n, u|
  { 'loopId' => l, 'n' => n, 'uuid' => u, 'id' => "looptick_#{l}_#{n}_#{u}" }
end

# tick ranges
rt = [tick(1, t('22:00'), t('22:05')), tick(2, t('22:10'), t('22:15')), tick(3, t('22:20'), nil)]
fixture['tickRanges'] = [
  ['markers in order, steers and a user turn inside', ['u0', tick_id(1), 'a1', 'steer_x', 'a2', 'u_user', 'a3', tick_id(2), 'a4', tick_id(3), 'a5'], rt],
  ['a marker removed by an edit', ['u0', tick_id(1), 'a1', 'a2', tick_id(3), 'a5'], rt],
  ['messages without ids', [nil, tick_id(1), nil, tick_id(2), nil], rt[0..1]],
  ['the last tick\'s marker is the last message', [tick_id(1), 'a', tick_id(2)], rt[0..1]],
  ['no ticks', ['u0', 'a0'], []]
].map { |name, ids, ticks| { 'name' => name, 'messageIds' => ids, 'ticks' => ticks, 'expect' => tick_ranges(ids, ticks) } }

# steps
facts_all = { 'stateFile' => STATE, 'check' => CHECK, 'goalFirstLine' => first_line(GOAL),
              'lastNextStep' => { 'kind' => 'named', 'text' => 'add svc- accounts' }, 'workingDir' => WD }
fixture['renderSteps'] = [
  ['quality, every fact present', QUALITY_STEPS, facts_all],
  ['quality, no check set, the first tick', QUALITY_STEPS, facts_all.reject { |k, _| k == 'check' }.merge('lastNextStep' => { 'kind' => 'first' })],
  ['quality, the last tick named no next step', QUALITY_STEPS, facts_all.merge('lastNextStep' => { 'kind' => 'named_none', 'prev' => 4 })],
  ['until a check passes', UNTIL_STEPS, facts_all],
  ['watch and act', WATCH_STEPS, facts_all],
  ['blank', '', facts_all],
  ['an unknown slot is left as typed and named', "1. Open {state_file} and {foo}.\n2. Then {foo} and {bar_2}.", facts_all],
  ['braces that are not slots', 'Keep {} and { a } and {Foo} and {1a} and {a-b} and {abc and json {"k": 1}.', facts_all]
].map { |name, steps, fa| { 'name' => name, 'steps' => steps, 'facts' => fa, 'expect' => render_steps(steps, fa) } }
fixture['stepSlots'] = [QUALITY_STEPS, UNTIL_STEPS, WATCH_STEPS, '', 'x {foo} {state_file} {foo}'].map { |s| { 'steps' => s, 'slots' => step_slots(s) } }

fixture['defaultStateFile'] = [
  'Make the users.csv generator produce every problem class Aoife listed', "  Fix\nthe second line", 'Fă testele să treacă',
  '🚀🚀', 'a-b_c d e f', 'UPPER Case', ''
].map { |g| { 'goal' => g, 'path' => default_state_file(g) } }

# validation
base_edit = { 'goal' => GOAL, 'template' => 'quality', 'steps' => QUALITY_STEPS, 'cadence' => every('10m'), 'stateFile' => STATE, 'check' => CHECK }
chat = { 'workingDir' => WD, 'swarmBuild' => false }
vc = []
vc << ['a valid start', base_edit, chat]
vc << ['swarm-build chat refused first, even with an empty goal', base_edit.merge('goal' => ''), chat.merge('swarmBuild' => true)]
vc << ['empty goal', base_edit.merge('goal' => " \n "), chat]
vc << ['until a check passes needs a check', base_edit.merge('template' => 'until_check', 'check' => '   '), chat]
vc << ['a blank check on another template is absent', base_edit.merge('check' => '  '), chat]
vc << ['a check is trimmed', base_edit.merge('check' => '  pnpm test  '), chat]
vc << ['a bad cadence', base_edit.merge('cadence' => every('soon')), chat]
vc << ['a cadence is trimmed', base_edit.merge('cadence' => every(' 90m ')), chat]
vc << ['self-paced needs no cadence string', base_edit.merge('cadence' => SELF), chat]
vc << ['an absolute state file inside the working dir becomes relative', base_edit.merge('stateFile' => "#{WD}/notes/NOW.md"), chat]
vc << ['a working dir with a trailing slash', base_edit.merge('stateFile' => "#{WD}/notes/NOW.md"), chat.merge('workingDir' => "#{WD}/")]
vc << ['dot segments are resolved', base_edit.merge('stateFile' => './notes/../loop/./NOW.md'), chat]
vc << ['a path that climbs out', base_edit.merge('stateFile' => '../elsewhere/NOW.md'), chat]
vc << ['a sibling folder sharing the prefix is outside', base_edit.merge('stateFile' => "#{WD}shop/NOW.md"), chat]
vc << ['an absolute path elsewhere', base_edit.merge('stateFile' => '/tmp/NOW.md'), chat]
vc << ['the working dir itself names no file', base_edit.merge('stateFile' => WD), chat]
vc << ['an empty state file', base_edit.merge('stateFile' => '  '), chat]
vc << ['an unknown slot', base_edit.merge('steps' => 'Open {state_file}, then {foo}.'), chat]
vc << ['stop after zero ticks', base_edit.merge('stopAfterTicks' => 0), chat]
vc << ['stop after five ticks', base_edit.merge('stopAfterTicks' => 5), chat]
fixture['validate'] = vc.map { |name, e, c| { 'name' => name, 'edit' => e, 'chat' => c, 'expect' => validate(e, c) } }

fixture['loopCommands'] = [
  '/loop', '/loop   ', '/loop now', '/loop NOW', '/loop pause', '/loop resume', '/loop stop', '/loop stop now',
  '/loop Stop the flaky test', '/loop fix the tests', '/loop every 10m fix the tests', '/loop Every 5m watch the build',
  '/loop  every   30m   watch the deploy  ', '/loop every 10m', '/loop every', '/loop every soon fix it',
  '/loop everything is broken', '/looping', 'hello /loop', '  /loop now  ', '/loop stop.', "/loop\tnow"
].map { |line| { 'line' => line, 'expect' => parse_loop_command(line) } }

big = (1..400).map { |i| "line#{i}: ok" }.join("\n") + "\nFAIL missing svc- accounts\n"
fixture['outputTail'] = [
  ['two words fit', 'a b c d e', 128],
  ['an output far larger than the window', big, 640],
  ['an output smaller than the budget', "3 passed\n", 64 * 10],
  ['an empty output', '', 262_144],
  ['a budget of zero keeps only trailing whitespace', "alpha beta \n", 63],
  ['multibyte words', 'ăîș țâ ßü 日本 語', 128]
].map { |name, out, w| { 'name' => name, 'output' => out, 'window' => w, 'expect' => output_tail(out, w) } }

# prompts
pf = ->(extra = {}) { { 'workingDir' => WD, 'utcOffsetMinutes' => 0 }.merge(extra) }
pr = []
pr << ['quality, tick 1', rec('check' => CHECK), 1, pf.call]
pr << ['quality, tick 4 after progress and a failing check, a tick count set',
       rec('check' => CHECK, 'stopAfterTicks' => 10, 'ticks' => [tick(3, t('22:31'), t('22:37', 12), 'report' => prog.call('add svc- service accounts with no last_login'),
                                                                        'outcome' => { 'kind' => 'progress' }, 'wrote' => ['scripts/generate_users.js'],
                                                                        'check' => check_run(CHECK, t('22:37', 13), true, 1, "row 17: ok\nFAIL missing svc- accounts\n", 'endedAt' => iso(t('22:37', 20))))]), 4, pf.call]
pr << ['until a check passes, back to back, reported done but the check failed',
       rec('template' => 'until_check', 'steps' => UNTIL_STEPS, 'cadence' => B2B, 'check' => 'pnpm test', 'ticks' => [tick(2, t('22:00'), t('22:04'), 'report' => report('done', 'All tests pass locally.', 'none'), 'outcome' => { 'kind' => 'done' },
                                                                                                                                  'check' => check_run('pnpm test', t('22:04'), true, 2, '2 failed, 40 passed'))]), 3, pf.call]
pr << ['blank, self-paced, the last tick made no report',
       rec('template' => 'blank', 'steps' => '', 'cadence' => SELF, 'ticks' => [tick(1, t('22:00'), t('22:06'), 'outcome' => { 'kind' => 'no_report' })]), 2, pf.call]
pr << ['watch, the last tick yielded to a user turn',
       rec('template' => 'watch', 'steps' => WATCH_STEPS, 'cadence' => every('30m'), 'ticks' => [tick(5, t('22:00'), t('22:03', 40), 'outcome' => { 'kind' => 'yielded', 'toSession' => 's2', 'toChat' => 'Kickoff notes' })]), 6, pf.call('utcOffsetMinutes' => 180)]
pr << ['the last tick asked and was answered',
       rec('check' => CHECK, 'ticks' => [tick(2, t('22:00'), t('22:02'), 'report' => prog.call('pick the delimiter'), 'outcome' => { 'kind' => 'asked', 'itemId' => 'ny_1', 'question' => 'Comma or semicolon?' })]), 3, pf.call('askedResolution' => 'answered')]
pr << ['the last tick asked and it was dismissed',
       rec('ticks' => [tick(2, t('22:00'), t('22:02'), 'outcome' => { 'kind' => 'asked', 'itemId' => 'ny_1', 'question' => 'Comma or semicolon?' })]), 3, pf.call('askedResolution' => 'dismissed')]
pr << ['the last tick failed',
       rec('ticks' => [tick(2, t('22:00'), t('22:01'), 'outcome' => { 'kind' => 'failed', 'errorClass' => 'provider', 'error' => 'Provider error: stream ended early' })]), 3, pf.call]
pr << ['the check could not run',
       rec('check' => CHECK, 'ticks' => [tick(2, t('22:00'), t('22:05'), 'report' => prog.call, 'outcome' => { 'kind' => 'progress' }, 'wrote' => ['a.js'],
                                              'check' => check_run(CHECK, t('22:05'), false, nil, '', 'error' => 'sh: node: command not found'))]), 3, pf.call]
pr << ['the check failed with no output',
       rec('check' => CHECK, 'ticks' => [tick(2, t('22:00'), t('22:05'), 'report' => prog.call, 'outcome' => { 'kind' => 'progress' }, 'wrote' => ['a.js'],
                                              'check' => check_run(CHECK, t('22:05'), true, 1, "  \n"))]), 3, pf.call]
pr << ['resumed after a blocked tick',
       rec('ticks' => [tick(4, t('22:00'), t('22:05'), 'report' => report('blocked', 'Two delimiters fit the notes.', 'ask the owner', 'blockedOn' => 'which CSV delimiter the owner wants'), 'outcome' => { 'kind' => 'blocked' })]), 5, pf.call]
fixture['prompts'] = pr.map { |name, r, n, fa| { 'name' => name, 'record' => r, 'n' => n, 'facts' => fa, 'expect' => prompt(r, n, fa) } }

File.write(OUT, JSON.pretty_generate(fixture) + "\n")
