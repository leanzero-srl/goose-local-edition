# Cloud SB7 incident: smoke lane transition rejected before admission

Campaign `cloud-sb7-20260823-ac376` launched all five strict provider-contract smoke supervisors at `2026-08-23T01:05:35Z`. Each supervisor transitioned its durable state from `PLANNED` to `WAITING_PROVIDER_LANE`, acquired its distinct provider lane, and then called `prepare_smoke_attempt`. That function accepted only `PLANNED` and `PRE_ADMISSION_FAILURE`, so it rejected the state its own caller had just established. All five failures were exactly `<entrant> smoke cannot launch from WAITING_PROVIDER_LANE`.

This is a coordinator infrastructure defect, not model output or provider behavior. Every smoke state retained `launch_attempts=0` and `admitted_episodes=0`. The budget ledger retained `spent_upper_bound=0.0`, no outstanding reservations, and no settled requests. No attempt directory, provider lifecycle event, or paid request was created.

The correction distinguishes retry eligibility from attempt preparation. `SMOKE_RETRYABLE_STATES` remains unchanged for supervisor admission and recovery; `prepare_smoke_attempt` additionally accepts the already-admitted `WAITING_PROVIDER_LANE` transition. The regression test reproduces that exact durable transition and proves it reaches `PREPARING` with one launch attempt and zero admitted provider episodes.

Frozen predecessor identity:

- Binary SHA-256: `5d9de4ac8d9222458b3b6c80d2fdb261df99e9f7047669f1b992add33cc7db7a`
- Instrument SHA-256: `ce8a8272f2a3b7e4159da5ec9ae676ec22f1678ae217bfe374df5b37c29603f9`
- Smoke contract SHA-256: `b193b62f459e4f6e1472d8f9557a78100612a932da182815e0df8b08b45a8ca9`
- Entrants affected: `glm-5.3`, `gemini-3.7-flash`, `gemini-3.1-pro-preview`, `deepseek-v4-flash`, `deepseek-v4-pro`
