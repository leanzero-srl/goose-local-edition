#!/bin/zsh
cd ${0:a:h}
for c in 2 3; do
  echo "MEM c${c} start MB $(top -l 1 -n 0 | grep PhysMem) | ST $(ssh workhorse 'top -l 1 -n 0 | grep PhysMem')" >> mem.log
  node turn.mjs c${c}t1 "In one sentence: what is the capital of Spain?" >/dev/null 2>&1
  if [ $c = 2 ]; then f1=when-rules.md; f2=load-rule.md; else f1=roles.md; f2=when-rules.md; fi
  node turn.mjs c${c}t2 "Please run two delegates (subagents) at the same time: one reads work/$f1 and summarises it in two sentences, the other reads work/$f2 and summarises it in two sentences. Then give me both summaries, labelled by file." >/dev/null 2>&1
  echo "MEM c${c} after-t2 MB $(top -l 1 -n 0 | grep PhysMem) | ST $(ssh workhorse 'top -l 1 -n 0 | grep PhysMem')" >> mem.log
  node turn.mjs c${c}t3 "Thanks. In one short sentence: which file mentions a Mac-wide lock?" >/dev/null 2>&1
  echo "MEM c${c} end MB $(top -l 1 -n 0 | grep PhysMem) | ST $(ssh workhorse 'top -l 1 -n 0 | grep PhysMem')" >> mem.log
done
echo ALLDONE >> mem.log
