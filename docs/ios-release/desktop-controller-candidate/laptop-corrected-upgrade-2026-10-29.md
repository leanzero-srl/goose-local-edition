# Original laptop: corrected controller upgrade

Verified 28 October 2026, 23:14 UTC. This receipt covers installation and reconnection, not the final visible active-steering test.

The complete signed 3.0.134 bundle from source `bf5966b19fdf4835b8feadcbe95b2951c7f3498b`, tree `67c316961e92ade0f6f10a156beeb0972f34993d`, was installed by node-dispatch job `laptop-20261029-010501-ff7d`. The job waited for resources, ran 23:07:07–23:07:27 UTC and exited 0. Signed CLI SHA256 is `349b242b64a4a48072fbbfcecda0c9a9a2c32fef5639ae6c045003b34d7bee6f`.

Before shutdown, the installer verified the complete staged bundle against the signed receipt and checked both the fresh owner catalog and session activity twice. Any active run ID prevents replacement, including an otherwise idle waiting agent or queued shell job. It terminated only the identified original application and waited for its descendants. The whole-app swap preserved profile files; normal LaunchServices launched the replacement. No security warning was bypassed or protection disabled.

New application PID58563/backend58858 reported connected, reconnected, online at100.64.0.1 with no last error. Original node identity remains mihai-macbook-2-local-faf207. The fresh catalog contains557 laptop entries and55 Gabriela entries, both error-free. Older owners without registered endpoints remain unavailable; that is not a claim that those machines were upgraded.

Read-only full-record preservation checks excluded only our known disposable session20261028_1. All73324 original session records and384284 original message records match the prior hashes exactly. Session digest:9dccd9687e6c46002fa2cddc1a3c0693354b3de70b221af0befa7f16ed12dc0e. Full message digest:7b6a5c8e63ad99a766b483ae2b22207537720a4b4d0058a2ea3e33bb0e4e38ec. The disposable session remains. Configuration retains all original keys and values; only the previously documented adjacent openai/openrouter map ordering differs. Evidence: `/tmp/goose-laptop-corrected-preservation.json`.

The prior controller candidate is retained at `~/.local/share/goose-controller-rollbacks/88cdf89cde29c490/Goose Swarm.app`. The original released3.0.134 rollback remains separately at `~/.local/share/goose-upgrade-rollback-3.0.134-20261028/Goose Swarm.app`. Neither Studio was touched. No inference, Apple upload or main merge occurred during this upgrade.

The UI remains locked. The owner has already been asked to unlock it; final visible steering confirmation remains outstanding.
