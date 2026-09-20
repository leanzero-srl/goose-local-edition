# Optional benchmark tools

SB7.1 requires macOS on Apple Silicon. Open Benchmark and choose **Install benchmark tools** before the first run. This is optional for Goose users who do not run benchmarks. The app downloads about 49 MB (46.8 MiB), verifies each pinned archive, installs Python and the video tools in its own application-data directory, and runs a preflight. No Homebrew, system Python, or terminal setup is required. Chromium, Playwright and Node already travel with Goose.

Installation reports download, extraction and verification progress. Run remains unavailable until the tools are ready. Interrupted or corrupt downloads can be retried; the installer does not replace an existing installation until all new tools pass verification. Settings/provider credentials are separate: configure the selected provider in the normal app settings.

The installed runtime lives at `<Electron userData>/benchmark/runtime`. Results and install identity follow the configured Goose profile. A clean-profile acceptance test must isolate both `GOOSE_PATH_ROOT` and Electron `--user-data-dir`; changing only one can borrow the other profile's data.

Runtime pins and download sizes live in `ui/desktop/scripts/benchmark-runtimes.json`. The installer checks SHA-256/SHA-512 before extraction. `sources.json` and upstream package notices travel with the installed tools. The developer-only `bundle-benchmark-runtimes.mjs` prepares a local cache for runtime tests; it is not a packaging hook and the runtime is not included in the app download.

## Validation, 2026-09-20

`BENCH_RUNTIME_INTEGRATION=1 npx vitest run src/benchRuntimeInstaller.test.ts` passed all four tests, including real downloads and execution of Python 3.12.14, sqlite3, SSL, zoneinfo, FFmpeg and FFprobe with `PATH=/usr/bin:/bin`. Other tests prove that inspection performs no download, checksum failures never extract an archive, interrupted transfers never become ready, concurrent install requests share one operation, and failed replacement preserves prior files.

Installed-app UI acceptance and the full run/restart/publish path are still required before the stable release is declared. These tool-level checks do not establish that release outcome.
