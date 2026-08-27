# Justfile

# list all tasks
default:
  @just --list

# Stable macOS code-signing identity for LOCAL builds (self-signed, login keychain).
# A keychain item's trusted-application ACL binds to the code's DESIGNATED REQUIREMENT. An
# ad-hoc signature's requirement is that build's cdhash, so every rebuild is a brand-new
# identity and macOS re-prompts for the keychain password the first time the new binary reads
# a stored key. Signing with a certificate makes the requirement `certificate leaf = <hash>`,
# which does not move between builds, so one "Always Allow" then covers every later build.
# That is one of TWO gates: the item's PartitionID ACL is a separate list, and code with no
# Team Identifier (which a self-signed cert cannot supply) partitions as `cdhash:<build>`.
# Signing alone therefore does not stop the prompts -- run scripts/fix-keychain-prompts.sh
# once as well (`just fix-keychain-prompts`).
# Absent (CI, fresh clone) the recipes fall back to ad-hoc and behave as before.
# Create it once with: just setup-signing-identity
# The gate below deliberately omits `find-identity -v`: -v filters out self-signed certs as
# untrusted and reports "0 valid identities" even when the cert is installed and signs fine.
# Stable local code-signing identity name.
local_sign_identity := "Goose Local Dev"

# Create the stable self-signed code-signing certificate this repo's macOS recipes look for.
# Idempotent: refuses to add a second cert with the same name, because two would make
# `codesign --sign "Goose Local Dev"` ambiguous and fail every build.
# Prompts once for your LOGIN KEYCHAIN password (set-key-partition-list needs it to grant
# codesign non-interactive access to the new private key). No trust settings are installed --
# codesign does not require the certificate to be trusted.
# One-time setup: create the stable local code-signing certificate
[macos]
setup-signing-identity:
    #!/usr/bin/env bash
    set -euo pipefail
    if security find-identity -p codesigning | grep -q "{{local_sign_identity}}"; then
        echo "'{{local_sign_identity}}' already present - nothing to do."
        exit 0
    fi
    WORK=$(mktemp -d)
    trap 'rm -rf "$WORK"' EXIT
    openssl req -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes \
        -keyout "$WORK/key.pem" -out "$WORK/cert.pem" \
        -subj "/CN={{local_sign_identity}}" \
        -addext "basicConstraints=critical,CA:false" \
        -addext "keyUsage=critical,digitalSignature" \
        -addext "extendedKeyUsage=critical,codeSigning" >/dev/null 2>&1
    openssl pkcs12 -export -out "$WORK/id.p12" -inkey "$WORK/key.pem" -in "$WORK/cert.pem" \
        -passout pass:goose -macalg sha1 -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES
    security import "$WORK/id.p12" -k ~/Library/Keychains/login.keychain-db -P goose -T /usr/bin/codesign
    echo ">>> Enter your macOS LOGIN password when prompted (grants codesign access to the key):"
    security set-key-partition-list -S apple-tool:,apple:,codesign: -s -D "{{local_sign_identity}}" -t private ~/Library/Keychains/login.keychain-db
    security find-identity -p codesigning | grep "{{local_sign_identity}}"

# The second half of the fix, and the half signing cannot do. Proves the mechanism on a
# throwaway item before it touches a real one, and asks for your login password.
# One-time repair: clear per-build partition IDs off goose's keychain items
[macos]
fix-keychain-prompts:
    ./scripts/fix-keychain-prompts.sh

# Run all style checks and formatting (precommit validation)
check-everything:
    @echo "🔧 RUNNING ALL STYLE CHECKS..."
    @echo "  → Formatting Rust code..."
    cargo fmt --all
    @echo "  → Running clippy linting..."
    cargo clippy --all-targets -- -D warnings
    @echo "  → Checking UI code formatting..."
    cd ui/desktop && pnpm run lint:check
    @echo ""
    @echo "✅ All style checks passed!"

# Default release command
release-binary:
    @echo "Building release version..."
    cargo build --release -p goose-cli --bin goose
    @just copy-binary
    @echo "Generating OpenAPI schema..."
    cargo run -p goose-server --bin generate_schema

# Build Windows executable on a Windows host
[unix]
release-windows:
    @echo "just release-windows requires a Windows host because Goose Windows releases build the MSVC target. Use .github/workflows/bundle-desktop-windows.yml for CI builds."
    @exit 1

[windows]
release-windows:
    @powershell.exe -NoProfile -ExecutionPolicy Bypass -Command 'rustup target add x86_64-pc-windows-msvc; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; cargo build --release --target x86_64-pc-windows-msvc -p goose-cli --bin goose; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; Write-Host "Windows executable created at ./target/x86_64-pc-windows-msvc/release/goose.exe"'

# Build for Intel Mac
release-intel:
    @echo "Building release version for Intel Mac..."
    cargo build --release --target x86_64-apple-darwin
    @just copy-binary-intel

copy-binary BUILD_MODE="release":
    @rm -f ./ui/desktop/src/bin/goosed
    @if [ -f ./target/{{BUILD_MODE}}/goose ]; then \
        echo "Copying goose CLI binary from target/{{BUILD_MODE}}..."; \
        if [ "$(uname)" = "Darwin" ]; then \
            if security find-identity -p codesigning | grep -q "{{local_sign_identity}}"; then \
                echo "Signing with the stable '{{local_sign_identity}}' identity..."; \
                codesign --force -s "{{local_sign_identity}}" ./target/{{BUILD_MODE}}/goose; \
            else \
                codesign --force -s - ./target/{{BUILD_MODE}}/goose; \
            fi; \
        fi; \
        rm -f ./ui/desktop/src/bin/goose; \
        cp -p ./target/{{BUILD_MODE}}/goose ./ui/desktop/src/bin/; \
        ./ui/desktop/src/bin/goose --version >/dev/null || { echo "copied binary does not EXECUTE (broken code signature = silent SIGKILL on Apple Silicon)"; exit 1; }; \
    else \
        echo "goose CLI binary not found in target/{{BUILD_MODE}}"; \
        exit 1; \
    fi

# Copy binary command for Intel build
copy-binary-intel:
    @rm -f ./ui/desktop/src/bin/goosed
    @if [ -f ./target/x86_64-apple-darwin/release/goose ]; then \
        echo "Copying Intel goose CLI binary to ui/desktop/src/bin..."; \
        rm -f ./ui/desktop/src/bin/goose; \
        cp -p ./target/x86_64-apple-darwin/release/goose ./ui/desktop/src/bin/; \
    else \
        echo "Intel goose CLI binary not found."; \
        exit 1; \
    fi

# Copy Windows binary command on a Windows host
[unix]
copy-binary-windows:
    @echo "just copy-binary-windows requires a Windows host because it copies the MSVC build output."
    @exit 1

[windows]
copy-binary-windows:
    @powershell.exe -NoProfile -ExecutionPolicy Bypass -Command 'if (Test-Path ./target/x86_64-pc-windows-msvc/release/goose.exe) { \
        Write-Host "Copying Windows binary to ui/desktop/src/bin..."; \
        New-Item -ItemType Directory -Force "./ui/desktop/src/bin" | Out-Null; \
        Remove-Item -Path "./ui/desktop/src/bin/goosed.exe" -Force -ErrorAction SilentlyContinue; \
        Copy-Item -Path "./target/x86_64-pc-windows-msvc/release/goose.exe" -Destination "./ui/desktop/src/bin/" -Force; \
    } else { \
        Write-Host "Windows binary not found." -ForegroundColor Red; \
        exit 1; \
    }'

# Run UI with latest
run-ui:
    @just release-binary
    @echo "Running UI..."
    cd ui/desktop && pnpm install && pnpm run start-gui

run-ui-playwright:
    #!/usr/bin/env sh
    just release-binary
    echo "Running UI with Playwright debugging..."
    RUN_DIR="$HOME/goose-runs/$(date +%Y%m%d-%H%M%S)"
    mkdir -p "$RUN_DIR"
    echo "Using isolated directory: $RUN_DIR"
    cd ui/desktop && ENABLE_PLAYWRIGHT=true GOOSE_PATH_ROOT="$RUN_DIR" pnpm run start-gui

run-ui-only:
    @echo "Running UI..."
    cd ui/desktop && pnpm install && pnpm run start-gui

debug-ui:
    @echo "🚀 Starting goose frontend in external ACP backend mode"
    cd ui/desktop && \
    export GOOSE_EXTERNAL_BACKEND=true && \
    export GOOSE_SERVER__SECRET_KEY="${GOOSE_SERVER__SECRET_KEY:-test}" && \
    pnpm install && \
    pnpm run start-gui

# Run UI with main process debugging enabled
# To debug main process:
# 1. Run: just debug-ui-main-process
# 2. Open Chrome → chrome://inspect
# 3. Click "Open dedicated DevTools for Node"
# 4. If not auto-detected, click "Configure" and add: localhost:9229

debug-ui-main-process:
	@echo "🔍 Starting goose UI with main process debugging enabled"
	@just release-binary
	cd ui/desktop && \
	pnpm install && \
	pnpm run start-gui-debug

# Signs with entitlements (needed for mic access, etc.), preferring the stable
# "Goose Local Dev" identity so the packaged app keeps one code identity across rebuilds
# and stops re-triggering the keychain password prompt; ad-hoc otherwise.
# The cert branch uses entitlements.local.plist: a self-signed cert has no Team Identifier,
# and that file is the same entitlement set plus disable-library-validation, which is what
# lets the nested Electron Framework load once anything turns the hardened runtime on.
# `--deep` reaches the frameworks and helper .apps but NOT Contents/Resources/bin/goose --
# a bare Mach-O under Resources is sealed as a resource, not re-signed. That binary is the
# one that reads the keychain, and it carries whatever signature copy-binary gave it.
# Package the desktop app locally for testing (macOS)
package-ui:
    @just release-binary
    @echo "Packaging desktop app..."
    cd ui/desktop && pnpm install && pnpm run package
    @if security find-identity -p codesigning | grep -q "{{local_sign_identity}}"; then \
        echo "Signing with the stable '{{local_sign_identity}}' identity + entitlements.local.plist..."; \
        codesign --force --deep --sign "{{local_sign_identity}}" --entitlements ui/desktop/entitlements.local.plist ui/desktop/out/Goose-darwin-arm64/Goose.app; \
    else \
        echo "Signing ad-hoc with entitlements..."; \
        codesign --force --deep --sign - --entitlements ui/desktop/entitlements.plist ui/desktop/out/Goose-darwin-arm64/Goose.app; \
    fi
    @echo "Done! Launch with: open ui/desktop/out/Goose-darwin-arm64/Goose.app"

# local-edition: build a SIGNED desktop release for our own fork's auto-update feed
# (leanzero-srl/goose-local-edition), and stage the update artifacts.
# One-time prereq: the stable self-signed cert -- run `just setup-signing-identity`.
# Unlike an ad-hoc `--sign -` build, which Squirrel rejects across versions, this signs
# every build with the SAME stable cert so auto-update validates build-to-build.
# Usage: just release-fork 1.41.1
release-fork version:
    # STAMP THE BUILD SO A RUN CAN SAY WHICH BINARY PRODUCED IT.
    # The engine's levers_resolved event reports these. Without them it fell back to CARGO_PKG_VERSION,
    # which is the workspace crate version (1.41.0) and has not moved in 54 desktop releases — so every
    # run from every build emitted the same string and attributing a result to a build meant trusting a
    # hand-typed note. A number the engine did not emit is not evidence.
    GOOSE_BUILD_VERSION={{version}} GOOSE_BUILD_SHA=$(git rev-parse --short HEAD) just release-binary
    @echo "Bumping ui/desktop version to {{version}}..."
    cd ui/desktop && node -e 'const fs=require("fs");const p=JSON.parse(fs.readFileSync("package.json","utf8"));p.version="{{version}}";fs.writeFileSync("package.json",JSON.stringify(p,null,2)+"\n")'
    @echo "Building (electron-forge make)..."
    cd ui/desktop && pnpm run make
    @echo "Signing the WHOLE bundle with the stable self-signed cert + local entitlements..."
    @echo "  (@electron/osx-sign ignores the entitlements path and applies defaults WITHOUT"
    @echo "   disable-library-validation, so the self-signed no-Team-ID build crashes on launch;"
    @echo "   this explicit re-sign forces entitlements.local.plist onto the app, the frameworks"
    @echo "   and the helper .apps. It does NOT reach Contents/Resources/bin/goose -- a bare Mach-O"
    @echo "   under Resources is sealed as a resource, and keeps the signature copy-binary gave it.)"
    cd ui/desktop && codesign --force --deep --options runtime --entitlements entitlements.local.plist --sign "{{local_sign_identity}}" out/Goose-darwin-arm64/Goose.app
    @echo "Re-zipping the signed app (auto-update artifact) + rebuilding the DMG FROM the signed app..."
    cd ui/desktop && rm -f out/Goose-darwin-arm64/Goose.zip && ditto -c -k --sequesterRsrc --keepParent out/Goose-darwin-arm64/Goose.app out/Goose-darwin-arm64/Goose.zip
    cd ui/desktop && rm -rf out/dmgstage && mkdir -p out/dmgstage && ditto out/Goose-darwin-arm64/Goose.app out/dmgstage/Goose.app && ln -s /Applications out/dmgstage/Applications && rm -f out/make/Goose-{{version}}.dmg && hdiutil create -volname Goose -srcfolder out/dmgstage -ov -format UDZO out/make/Goose-{{version}}.dmg
    @echo "Generating update manifest (latest-mac.yml)..."
    cd ui/desktop && node scripts/generate-mac-update-manifest.js --version {{version}} --directory out/Goose-darwin-arm64
    @echo ""
    @echo ">>> LAUNCH-CHECK BEFORE PUBLISHING (a valid signature is NOT the same as a launchable app):"
    @echo "    open ui/desktop/out/make/Goose-{{version}}.dmg   # drag to /Applications, then launch it"
    @echo "Staged: ui/desktop/out/Goose-darwin-arm64/{Goose-darwin-arm64.zip,latest-mac.yml} + out/make/Goose-{{version}}.dmg"
    @echo "Publish to the fork release (needs 'gh' + a GITHUB_TOKEN with repo scope):"
    @echo "  gh release create v{{version}} --repo leanzero-srl/goose-local-edition --title v{{version}} --notes 'local build' \\"
    @echo "    ui/desktop/out/Goose-darwin-arm64/Goose-darwin-arm64.zip \\"
    @echo "    ui/desktop/out/Goose-darwin-arm64/latest-mac.yml \\"
    @echo "    ui/desktop/out/make/Goose-{{version}}.dmg"

# Run UI with latest (Windows version)
run-ui-windows:
    @just release-windows
    @powershell.exe -Command "Write-Host 'Copying Windows binary...'"
    @just copy-binary-windows
    @powershell.exe -Command "Write-Host 'Running UI...'; Set-Location ui/desktop; pnpm install; pnpm run start-gui"

# Run Docusaurus server for documentation
run-docs:
    @echo "Running docs server..."
    cd documentation && yarn && yarn start

# Run server
run-server:
    @echo "Running external ACP backend..."
    GOOSE_SERVER__SECRET_KEY="${GOOSE_SERVER__SECRET_KEY:-test}" cargo run -p goose-cli --bin goose -- serve --platform desktop --host 127.0.0.1 --port 3000

# Generate OpenAPI specification without starting the UI
generate-openapi:
    @echo "Generating OpenAPI schema..."
    cargo run -p goose-server --bin generate_schema

# Check if generated ACP schema and TypeScript types are up-to-date
check-acp-schema: generate-acp-types
    #!/usr/bin/env bash
    set -e
    echo "🔍 Checking ACP schema and generated types are up-to-date..."
    if ! git diff --exit-code crates/goose/acp-schema.json crates/goose/acp-meta.json ui/sdk/src/generated/; then
      echo ""
      echo "❌ ACP generated files are out of date!"
      echo ""
      echo "Run 'just generate-acp-types' locally, then commit the changes."
      exit 1
    fi
    echo "✅ ACP schema and generated types are up-to-date"

# Generate ACP JSON schema from Rust types
generate-acp-schema:
    @echo "Generating ACP schema..."
    cd crates/goose && cargo run --features code-mode,local-inference,aws-providers,telemetry,otel,rustls-tls,system-keyring --bin generate-acp-schema
    @echo "ACP schema generated: crates/goose/acp-schema.json, crates/goose/acp-meta.json"

# Generate ACP TypeScript types from JSON schema (requires generate-acp-schema first)
generate-acp-types: generate-acp-schema
    @echo "Generating ACP TypeScript types..."
    cd ui/sdk && npx tsx generate-schema.ts
    @echo "ACP TypeScript types generated in ui/sdk/src/generated/"

# Build SDK TypeScript package (schema + types + compile)
build-sdk: generate-acp-types
    @echo "Compiling ACP TypeScript..."
    cd ui/sdk && pnpm run build:ts
    @echo "ACP package built."

# Generate manpages for the CLI
generate-manpages:
    @echo "Generating manpages..."
    cargo run -p goose-cli --bin generate_manpages
    @echo "Manpages generated at target/man/"

# make GUI with latest binary
lint-ui:
    cd ui/desktop && pnpm run lint:check

# make GUI with latest binary
make-ui:
    @just release-binary
    cd ui/desktop && pnpm run bundle:default
    # electron-forge signs the bundle its own way (ad-hoc), so after bundling the app and the engine
    # inside it disagree: Resources/bin/goose carries the stable "Goose Local Dev" leaf-hash requirement
    # from copy-binary while the .app is still Signature=adhoc. The engine is the one that reads the
    # keychain so prompts are already fixed by copy-binary, but a bundle whose identity changes every
    # build is what Squirrel rejects across versions. Re-sign it here when the cert exists; ad-hoc
    # otherwise, so CI and a fresh clone are unaffected.
    @if security find-identity -p codesigning | grep -q "{{local_sign_identity}}"; then \
        echo "Re-signing the bundle with '{{local_sign_identity}}'..."; \
        codesign --force --deep --sign "{{local_sign_identity}}" --entitlements ui/desktop/entitlements.local.plist ui/desktop/out/Goose-darwin-arm64/Goose.app; \
        ./ui/desktop/out/Goose-darwin-arm64/Goose.app/Contents/Resources/bin/goose --version >/dev/null || { echo "bundle does not EXECUTE after signing"; exit 1; }; \
    fi

# make GUI with latest Windows binary on a Windows host
[unix]
make-ui-windows:
    @echo "just make-ui-windows requires a Windows host because Goose Windows releases build the MSVC target. Use .github/workflows/bundle-desktop-windows.yml for CI builds."
    @exit 1

[windows]
make-ui-windows:
    @just release-windows
    @just copy-binary-windows
    @powershell.exe -NoProfile -ExecutionPolicy Bypass -Command 'Set-Location ui/desktop; $env:ELECTRON_PLATFORM="win32"; node scripts/prepare-platform-binaries.js; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; pnpm run make --platform=win32 --arch=x64; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; Write-Host "Windows package build complete!"'

# make GUI with latest binary
make-ui-intel:
    @just release-intel
    cd ui/desktop && pnpm run bundle:intel



# Run UI with debug build
run-dev:
    @echo "Building development version..."
    cargo build
    @just copy-binary debug
    @echo "Running UI..."
    cd ui/desktop && pnpm run start-gui

# Install all dependencies (run once after fresh clone)
install-deps:
    cd ui/desktop && pnpm install
    cd documentation && yarn

ensure-release-branch:
    #!/usr/bin/env bash
    branch=$(git rev-parse --abbrev-ref HEAD); \
    if [[ ! "$branch" == release/* ]]; then \
        echo "Error: You are not on a release branch (current: $branch)"; \
        exit 1; \
    fi

    # check that main is up to date with upstream main
    git fetch
    # @{u} refers to upstream branch of current branch
    if [ "$(git rev-parse HEAD)" != "$(git rev-parse @{u})" ]; then \
        echo "Error: Your branch is not up to date with the upstream branch"; \
        echo "  ensure your branch is up to date (git pull)"; \
        exit 1; \
    fi

# validate the version is semver, and not the current version
validate version:
    #!/usr/bin/env bash
    if [[ ! "{{ version }}" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-.*)?$ ]]; then
      echo "[error]: invalid version '{{ version }}'."
      echo "  expected: semver format major.minor.patch or major.minor.patch-<suffix>"
      exit 1
    fi

    current_version=$(just get-tag-version)
    if [[ "{{ version }}" == "$current_version" ]]; then
      echo "[error]: current_version '$current_version' is the same as target version '{{ version }}'"
      echo "  expected: new version in semver format"
      exit 1
    fi

get-next-minor-version:
    @python -c "import sys; v=sys.argv[1].split('.'); print(f'{v[0]}.{int(v[1])+1}.0')" $(just get-tag-version)

get-next-patch-version:
    @python -c "import sys; v=sys.argv[1].split('.'); print(f'{v[0]}.{v[1]}.{int(v[2])+1}')" $(just get-tag-version)

# derive the prior release tag from a version
# patch bump (e.g. 1.25.1): prior is v1.25.0 (deterministic)
# minor bump (e.g. 1.26.0): prior is highest v1.25.* GitHub release
get-prior-version version:
    #!/usr/bin/env bash
    IFS='.' read -r major minor patch <<< "{{ version }}"
    if [[ "$patch" -gt 0 ]]; then
      echo "v${major}.${minor}.$((patch - 1))"
    elif [[ "$minor" -gt 0 ]]; then
      prev_minor=$((minor - 1))
      prefix="v${major}.${prev_minor}."
      best=$(gh release list --limit 100 --exclude-drafts --exclude-pre-releases \
        --json tagName --jq "[.[] | select(.tagName | startswith(\"${prefix}\"))][0].tagName")
      if [[ -n "$best" && "$best" != "null" ]]; then
        echo "$best"
      fi
    fi

# update version numbers in all manifests
bump-version version:
    @just validate {{ version }} || exit 1
    @uvx --from=toml-cli toml set --toml-path=Cargo.toml "workspace.package.version" {{ version }}
    @cd ui/desktop && npm pkg set "version={{ version }}"
    # update Cargo.lock after bumping versions in Cargo.toml
    @cargo update --workspace
    @just set-openapi-version {{ version }}

# rebuild canonical model registry and mapping report from models.dev
build-canonical-models:
    @cargo run --bin build_canonical_models

# bump version, rebuild canonical models, and commit
prepare-release version:
    @just bump-version {{ version }}
    @just build-canonical-models
    @git add \
        Cargo.toml \
        Cargo.lock \
        ui/desktop/package.json \
        ui/pnpm-lock.yaml \
        ui/desktop/openapi.json \
        crates/goose-provider-types/src/canonical/data/canonical_models.json \
        crates/goose-provider-types/src/canonical/data/provider_metadata.json
    @git commit --message "chore(release): release version {{ version }}"

set-openapi-version version:
    @jq '.info.version |= "{{ version }}"' ui/desktop/openapi.json > ui/desktop/openapi.json.tmp && mv ui/desktop/openapi.json.tmp ui/desktop/openapi.json

# extract version from Cargo.toml
get-tag-version:
    @uvx --from=toml-cli toml get --toml-path=Cargo.toml "workspace.package.version"

# create the git tag from Cargo.toml, checking we're on a release branch
tag: ensure-release-branch
    git tag v$(just get-tag-version)

# create tag and push to origin (use this when release branch is merged to main)
tag-push: tag
    # this will kick of ci for release
    git push origin tag v$(just get-tag-version)

# generate release notes from git commits
release-notes old:
    #!/usr/bin/env bash
    git log --pretty=format:"- %s" {{ old }}..v$(just get-tag-version)

### s = file separator based on OS
s := if os() == "windows" { "\\" } else { "/" }
linux_vulkan_features := if os() == "linux" { "--features vulkan" } else { "" }

### testing/debugging
os:
  echo "{{os()}}"
  echo "{{s}}"

# Make just work on Window
set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

### Build the core code
### profile = --release or "" for debug
### allparam = OR/AND/ANY/NONE --workspace --all-features --all-targets
win-bld profile allparam:
  cargo run {{profile}} -p goose-server --bin  generate_schema
  cargo build {{profile}} {{allparam}}

### Build just debug
win-bld-dbg:
  just win-bld " " " "

### Build debug and test, examples,...
win-bld-dbg-all:
  just win-bld " " "--workspace --all-targets --all-features"

### Build just release
win-bld-rls:
  just win-bld "--release" " "

### Build release and test, examples, ...
win-bld-rls-all:
  just win-bld "--release" "--workspace --all-targets --all-features"

### Install pnpm stuff
win-app-deps:
  cd ui{{s}}desktop ; pnpm install

### Windows copy {release|debug} files to ui\desktop\src\bin
### s = os dependent file separator
### profile = release or debug
win-copy-win profile:
  copy target{{s}}{{profile}}{{s}}*.exe ui{{s}}desktop{{s}}src{{s}}bin
  copy target{{s}}{{profile}}{{s}}*.dll ui{{s}}desktop{{s}}src{{s}}bin
  if exist ui{{s}}desktop{{s}}src{{s}}bin{{s}}goosed.exe del /f /q ui{{s}}desktop{{s}}src{{s}}bin{{s}}goosed.exe

### "Other" copy {release|debug} files to ui/desktop/src/bin
### s = os dependent file separator
### profile = release or debug
win-copy-oth profile:
  find target{{s}}{{profile}}{{s}} -maxdepth 1 -type f -executable -print -exec cp {} ui{{s}}desktop{{s}}src{{s}}bin \;

### copy files depending on OS
### profile = release or debug
win-app-copy profile="release":
  just win-copy-{{ if os() == "windows" { "win" } else { "oth" } }} {{profile}}

### Only copy binaries, pnpm install, start-gui
### profile = release or debug
### s = os dependent file separator
win-app-run profile:
  just win-app-copy {{profile}}
  just win-app-deps
  cd ui{{s}}desktop ; pnpm run start-gui

### Only run debug desktop, no build
win-run-dbg:
  just win-app-run "debug"

### Only run release desktop, nu build
win-run-rls:
  just win-app-run "release"

### Build and run debug desktop. tot = cli and desktop
### allparam = nothing or -all passed on command line
### -all = build with --workspace --all-targets --all-features
win-total-dbg *allparam:
  just win-bld-dbg{{allparam}}
  just win-run-dbg

### Build and run release desktop
### allparam = nothing or -all passed on command line
### -all = build with --workspace --all-targets --all-features
win-total-rls *allparam:
  just win-bld-rls{{allparam}}
  just win-run-rls

build-test-tools:
  cargo build -p goose-test

record-mcp-tests: build-test-tools
  GOOSE_RECORD_MCP=1 cargo test --package goose --test mcp_integration_test
  git add crates/goose/tests/mcp_replays/
