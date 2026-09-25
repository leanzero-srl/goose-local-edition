#!/bin/bash
# Sourced by the node and npx shims: bootstraps hermit and Node.js. Every line of setup output goes
# to goose's log (see goose-shim-common.sh), never to the command the caller is running.

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/goose-shim-common.sh" "$(basename "${0}")"

goose_node_setup() {
    # macOS GUI apps otherwise never get the hermit-managed node/npx onto PATH (#9482).
    goose_hermit_bootstrap always

    log "Installing Node.js with hermit."
    hermit install node

    log "node: $(which node)"
    log "npx: $(which npx)"
    goose_hermit_release_lock

    if [ -n "${GOOSE_NPM_REGISTRY:-}" ] && curl -s --head --fail "${GOOSE_NPM_REGISTRY}" > /dev/null; then
        log "${GOOSE_NPM_REGISTRY} is accessible. Using it for npm registry."
        export NPM_CONFIG_REGISTRY="${GOOSE_NPM_REGISTRY}"

        if [ -n "${GOOSE_NPM_CERT:-}" ] && curl -s --head --fail "${GOOSE_NPM_CERT}" > /dev/null; then
            log "Downloading certificate from: ${GOOSE_NPM_CERT}"
            if curl -sSL -o "${MCP_HERMIT_DIR}/cert.pem" "${GOOSE_NPM_CERT}"; then
                log "Certificate downloaded successfully."
                export NODE_EXTRA_CA_CERTS="${MCP_HERMIT_DIR}/cert.pem"
            else
                log "Unable to download the certificate. Skipping certificate setup."
            fi
        else
            log "GOOSE_NPM_CERT is either not set or not accessible. Skipping certificate setup."
        fi
    else
        log "GOOSE_NPM_REGISTRY is either not set or not accessible. Using the default npm registry."
        export NPM_CONFIG_REGISTRY="https://registry.npmjs.org/"
    fi

    log "Node setup completed."
}

goose_node_setup < /dev/null >> "${LOG_FILE}" 2>&1
