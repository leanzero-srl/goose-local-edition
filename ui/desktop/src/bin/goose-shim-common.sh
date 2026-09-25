#!/bin/bash
# Sourced by goose's bundled tool shims (node, npx, uvx, jbang); never executed on its own.
#
# goose serve prepends this bin directory to PATH, so a shim runs for every MCP server launched as
# `npx`/`uvx`/`jbang` AND for every plain `node`/`npx`/`uvx` the model types into the shell tool.
# The contract that follows from that: the command's stdout, stderr, stdin, working directory and
# exit status belong to the command alone. Setup chatter goes to goose's log file; a setup that
# fails prints exactly one line to stderr naming that log and exits non-zero; a setup that
# succeeds execs the real tool in the caller's directory, so the tool's exit status is the shim's.

GOOSE_SHIM_NAME="${1:?goose-shim-common.sh needs the shim name}"
GOOSE_SHIM_CALLER_DIR="${PWD}"
GOOSE_SHIM_SETUP_DONE=0

if [ -n "${GOOSE_PATH_ROOT:-}" ]; then
    RESOLVED_GOOSE_CONFIG_DIR="${GOOSE_PATH_ROOT}/config"
    GOOSE_SHIM_LOG_DIR="${GOOSE_PATH_ROOT}/state/logs"
else
    if [ -n "${GOOSE_CONFIG_DIR:-}" ]; then
        GOOSE_SHIM_DEPRECATED_CONFIG_DIR=1
        RESOLVED_GOOSE_CONFIG_DIR="${GOOSE_CONFIG_DIR}"
    else
        RESOLVED_GOOSE_CONFIG_DIR="${HOME}/.config/goose"
    fi
    case "${XDG_STATE_HOME:-}" in
        /*) GOOSE_SHIM_LOG_DIR="${XDG_STATE_HOME}/goose/logs" ;;
        *) GOOSE_SHIM_LOG_DIR="${HOME}/.local/state/goose/logs" ;;
    esac
fi
MCP_HERMIT_DIR="${RESOLVED_GOOSE_CONFIG_DIR}/mcp-hermit"
LOG_FILE="${GOOSE_SHIM_LOG_DIR}/mcp-shims.log"

# The shims run on every shell `node`, so the log is appended (concurrent shims never clobber each
# other) and rotated once it passes 1 MiB rather than growing without bound.
if mkdir -p "${GOOSE_SHIM_LOG_DIR}" 2>/dev/null && touch "${LOG_FILE}" 2>/dev/null; then
    if [ "$(wc -c < "${LOG_FILE}")" -gt 1048576 ]; then
        mv -f "${LOG_FILE}" "${LOG_FILE}.1" 2>/dev/null || true
    fi
else
    GOOSE_SHIM_UNWRITABLE_LOG="${LOG_FILE}"
    LOG_FILE=/dev/null
fi

log() {
    printf '%s [%s %s] %s\n' "$(date +'%Y-%m-%d %H:%M:%S')" "${GOOSE_SHIM_NAME}" "$$" "${1}" >> "${LOG_FILE}"
}

# fd 9 keeps the caller's stderr while setup runs with stdout/stderr pointed at the log.
exec 9>&2

goose_shim_on_exit() {
    local status=$?
    if [ -n "${HERMIT_SETUP_LOCK_DIR:-}" ]; then
        rm -rf "${HERMIT_SETUP_LOCK_DIR}"
    fi
    if [ "${GOOSE_SHIM_SETUP_DONE}" != 1 ] && [ "${status}" != 0 ]; then
        log "Setup failed with status ${status}."
        if [ -n "${GOOSE_SHIM_UNWRITABLE_LOG:-}" ]; then
            echo "goose: ${GOOSE_SHIM_NAME} setup failed (status ${status}); its log could not be written to ${GOOSE_SHIM_UNWRITABLE_LOG}" >&9
        else
            echo "goose: ${GOOSE_SHIM_NAME} setup failed (status ${status}); see ${LOG_FILE}" >&9
        fi
    fi
}
trap goose_shim_on_exit EXIT

log "Starting ${GOOSE_SHIM_NAME} setup in ${GOOSE_SHIM_CALLER_DIR}."
if [ -n "${GOOSE_SHIM_DEPRECATED_CONFIG_DIR:-}" ]; then
    log "GOOSE_CONFIG_DIR is deprecated for desktop shims; prefer GOOSE_PATH_ROOT."
fi

# GUI-launched macOS apps inherit a minimal PATH that omits the system sbin directories, so Hermit's
# bootstrap cannot find tools like `chown` (/usr/sbin on macOS) and aborts with exit 127.
export PATH="/usr/sbin:/sbin:${PATH}"

# Bootstraps the shared hermit environment under ${MCP_HERMIT_DIR}. Callers run it with stdin from
# /dev/null and stdout/stderr on the log: an MCP server's stdin is its JSON-RPC channel, and a shell
# command's output is what the model reads. $1 = "always" or "linux": when to source activate-hermit
# (node needs it on macOS, see #9482; uvx and jbang have only ever activated on Linux).
goose_hermit_bootstrap() {
    local activate="${1}"

    mkdir -p "${RESOLVED_GOOSE_CONFIG_DIR}"
    HERMIT_SETUP_LOCK_DIR="${RESOLVED_GOOSE_CONFIG_DIR}/.mcp-hermit-setup.lock"
    local lock_timeout=300
    local lock_started_at
    lock_started_at=$(date +%s)
    while ! mkdir "${HERMIT_SETUP_LOCK_DIR}" 2>/dev/null; do
        if [ $(( $(date +%s) - lock_started_at )) -ge "${lock_timeout}" ]; then
            log "Timed out waiting for ${HERMIT_SETUP_LOCK_DIR}; removing stale lock."
            rm -rf "${HERMIT_SETUP_LOCK_DIR}"
            lock_started_at=$(date +%s)
        fi
        sleep 0.1
    done

    # One-time cleanup for existing Linux users to fix locking issues
    local cleanup_marker="${RESOLVED_GOOSE_CONFIG_DIR}/.mcp-hermit-cleanup-v1"
    if [[ "$(uname -s)" == "Linux" ]] && [ ! -f "${cleanup_marker}" ]; then
        log "Performing one-time cleanup of old mcp-hermit directory to fix locking issues."
        if [ -d "${MCP_HERMIT_DIR}" ]; then
            local stale_dir="${MCP_HERMIT_DIR}.stale.$$"
            mv "${MCP_HERMIT_DIR}" "${stale_dir}"
            rm -rf "${stale_dir}"
            log "Removed old mcp-hermit directory."
        fi
        touch "${cleanup_marker}"
        log "Cleanup completed. Marker file created."
    fi

    mkdir -p "${MCP_HERMIT_DIR}/bin"
    cd "${MCP_HERMIT_DIR}"

    if [ ! -f "${MCP_HERMIT_DIR}/bin/hermit" ]; then
        log "Hermit binary not found. Downloading hermit binary."
        curl -fsSL "https://github.com/cashapp/hermit/releases/download/stable/hermit-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m | sed 's/x86_64/amd64/' | sed 's/aarch64/arm64/').gz" \
            | gzip -dc > "${MCP_HERMIT_DIR}/bin/hermit" && chmod +x "${MCP_HERMIT_DIR}/bin/hermit"
        log "Hermit binary downloaded and made executable."
    fi

    mkdir -p "${MCP_HERMIT_DIR}/cache"
    export HERMIT_STATE_DIR="${MCP_HERMIT_DIR}/cache"
    export PATH="${MCP_HERMIT_DIR}/bin:${PATH}"
    which hermit

    if [ ! -f "bin/activate-hermit" ]; then
        log "Hermit environment not yet initialized. Setting up hermit."
        local hermit_cleanup_dir=""
        local hermit_original_path="${PATH}"
        # Fix hermit self-update lock issues on Linux by using temp binary for init only
        if [[ "$(uname -s)" == "Linux" ]]; then
            hermit_cleanup_dir="/tmp/hermit_tmp_$$"
            mkdir -p "${hermit_cleanup_dir}/bin"
            cp "${MCP_HERMIT_DIR}/bin/hermit" "${hermit_cleanup_dir}/bin/hermit"
            chmod +x "${hermit_cleanup_dir}/bin/hermit"
            export PATH="${hermit_cleanup_dir}/bin:${PATH}"
        fi
        hermit init
        if [ -n "${hermit_cleanup_dir}" ]; then
            export PATH="${hermit_original_path}"
            rm -rf "${hermit_cleanup_dir}"
        fi
    fi

    if [ "${activate}" = always ] || [[ "$(uname -s)" == "Linux" ]]; then
        log "Activating hermit environment."
        . "bin/activate-hermit"
    fi
}

# Forgetting the path matters: once released, another shim may hold the lock, and a later failure
# here must not remove it from under that shim.
goose_hermit_release_lock() {
    rm -rf "${HERMIT_SETUP_LOCK_DIR}"
    unset HERMIT_SETUP_LOCK_DIR
}

# Replaces the shim with the real tool: back in the caller's directory, fd 9 closed, no trap left to
# fire. The target is an explicit path under ${MCP_HERMIT_DIR}/bin, never a PATH lookup, because a
# lookup that missed the hermit tool would find this shim again and recurse.
goose_shim_exec() {
    local target="${1}"
    shift
    if [ ! -x "${target}" ]; then
        log "Expected ${target} after setup, but it is not executable."
        exit 127
    fi
    cd "${GOOSE_SHIM_CALLER_DIR}"
    log "Executing ${target} $*"
    GOOSE_SHIM_SETUP_DONE=1
    trap - EXIT
    exec 9>&-
    exec "${target}" "$@"
}
