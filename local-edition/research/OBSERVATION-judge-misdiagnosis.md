SYMPTOM    integrate-verify spent 80 minutes and 94 tool calls "fixing" static file serving, applied the
           judge's dictated code twice, and GET / still returned 404.

EVIDENCE   run swarm-3node-r0, 18:27-19:47Z. api.py modified 22:26 local, now contains
           `abs_path = Path(__file__).parent.parent / path` — exactly what judge_nudge dictated. That
           path RESOLVES CORRECTLY: <rundir>/vendorsync/web/index.html, verified to exist.
           The actual defect: the `/`, `/index.html`, `/styles.css`, `/app.js` branches are inside
           `do_POST`, not `do_GET`. A browser GET falls through do_GET's chain to its 404.
           Judge nudges, in escalation order:
             1. "fix the static file serving path resolution in vendorsync/api.py"
             2. "Read vendorsync/api.py lines 135-160, identify the bug, and fix it"
             3. "...stop re-reading lines 135-160"
             4. "use Path(__file__).parent.parent / \"web\" for the static directory path"
             5. "Run python3 -m vendorsync to verify the app boots and serves index.html at root"
           Escalation worked mechanically — each nudge more concrete than the last. The DIAGNOSIS was
           wrong from nudge 1 and never re-examined.

PHASE      integrate

HYPOTHESIS The judge's evidence is the call's own reasoning tail, its tool calls, and its answer. It
           never reads the file. So when a worker misframes its own bug ("the path must be wrong"), the
           judge inherits that frame, and because the judge is MORE confident and MORE concrete, it
           converts a hypothesis into an instruction. Escalation then amplifies a wrong diagnosis
           instead of correcting it: nudge 4 handed the worker literal code for the wrong line. This is
           the same failure the persona research found independently — corrective turns on small models
           are accepted verbally ("The supervisor is right") without changing behaviour, and the
           delivery mechanism degrades with conversation depth.

CHANGE     The judge must escalate toward OBSERVATION, not toward a patch. When its previous direction
           was followed and the symptom persists, the next nudge must be a MEASUREMENT that can refute
           the current theory — "run `curl -s localhost:PORT/` and print which handler receives it" —
           rather than a more detailed version of the same fix. A judge that has been obeyed twice and
           is still looking at the same symptom has evidence that its DIAGNOSIS is wrong, and that is
           the one inference it never draws today.

DETECTOR   Count nudges whose NEXT text names the same file as the previous nudge AND where the worker
           did act in between (tool_calls advanced). That pair — obeyed, and still wrong — is the
           signature of a misdiagnosis being amplified, and it is computable from the judge_nudge
           events already in the log. Emit it as `nudge_repeat_same_target` so the next run reports it
           without anyone reading a transcript.
