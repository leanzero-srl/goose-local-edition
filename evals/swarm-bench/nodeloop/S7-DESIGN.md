# S7 — idle slots generate contract-derived tests (design, pre-implementation)

Replace the never-fired speculative-twin rung (Speculated: 0 in 75+ logs) with test GENERATION
on idle slots: an idle node writes 3-5 pytest functions against the FROZEN CONTRACTS + spec
table (never against the code — AgentCoder's load-bearing choice), into NEW files pytest
auto-collects. Zero merge surface; cancel-on-ready (the A3 yield rule already gates idle jobs);
converts the measured low-concurrency span into Q6's cross-execution currency (selection
accuracy scales with test count — a 27B with 100 tests selects like a 70B).

Guards: generated tests run once for collectability before landing (a SyntaxError test file is
worse than none — the engine's own doctrine); tests asserting values the spec does not document
are dropped at review (a passing test that cements a bug is the known failure mode — assertions
must trace to a spec sentence or a contract signature).

Registered checks: mechanism — new test files appear only from idle windows and never displace
a ready task (A3's event trail); quality — candidate selection in repair rounds cites
generated-test outcomes; safety — stable-24 never below spread on the arm.
