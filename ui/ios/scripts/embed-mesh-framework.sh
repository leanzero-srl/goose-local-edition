#!/bin/sh
set -eu
if [ -z "${GOOSE_TAILSCALE_FRAMEWORK_DIR:-}" ]; then exit 0; fi
source_framework="$GOOSE_TAILSCALE_FRAMEWORK_DIR/TailscaleKit.framework"
if [ ! -f "$source_framework/TailscaleKit" ]; then
  echo 'error: Explicit mesh framework path does not contain TailscaleKit.framework/TailscaleKit' >&2
  exit 1
fi
mkdir -p "$TARGET_BUILD_DIR/$FRAMEWORKS_FOLDER_PATH"
/usr/bin/ditto "$source_framework" "$TARGET_BUILD_DIR/$FRAMEWORKS_FOLDER_PATH/TailscaleKit.framework"
if [ "${CODE_SIGNING_ALLOWED:-NO}" = YES ] && [ -n "${EXPANDED_CODE_SIGN_IDENTITY:-}" ]; then
  /usr/bin/codesign --force --sign "$EXPANDED_CODE_SIGN_IDENTITY" --preserve-metadata=identifier,entitlements "$TARGET_BUILD_DIR/$FRAMEWORKS_FOLDER_PATH/TailscaleKit.framework"
fi
