# Local Edition backlog

## Model weights in the swarm (requested 2026-07-11, and previously)
The user wants to assign WEIGHTS to each model/device in the swarm fleet, so a slower machine can be given
LESS work (fewer/lighter tasks) while a faster one does more. Currently the scheduler treats nodes roughly
equally (`planner_weight: 1` in config.yaml; devices listed without per-node weights).

Two concrete parts:
1. **Per-node weights in the fleet config + scheduler** — add a `weight` per device in config.yaml (and a
   UI to set it in the swarm settings), and have the task dispatcher bias assignment by weight (a lower-weight
   node picks up fewer tasks / is skipped more often when idle-stealing). Slower Mac => lower weight => less load.
2. **Model choice for single-model uses (recipe chat, etc.)** — right now the recipe chat (and any single-model
   path) always grabs the FIRST available model (`fleet.models.find(coder) ?? fleet.models[0]` in
   RecipeChatWizard). The user should be able to pick which model these use — tie this to the same
   weights/preference so a build/recipe doesn't always land on the first node.

Where: crates/goose-swarm scheduler (task assignment), crates/goose-cli/commands/swarm.rs (fleet reconcile
from config), ~/.config/goose/config.yaml (per-device weight), ui/desktop swarm settings (weight sliders) +
ui/desktop/src/components/swarm/RecipeChatWizard.tsx + useFleet.ts (model picker).
