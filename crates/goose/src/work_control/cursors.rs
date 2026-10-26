//! Authenticated positions over retained journal versions, never raw caller-selected IDs.

use axum::http::StatusCode;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use leanzero_link::controller::ControllerError;
use leanzero_link::controller_auth::AuthenticatedDevice;
use leanzero_link::owner_identity::load_or_create_private;
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};
use subtle::ConstantTimeEq;
use uuid::Uuid;

/// Transport policy only. Journal versions are currently retained without pruning.
/// A future pruning policy must retain every unexpired snapshot or reject it explicitly.
#[derive(Clone, Debug)]
pub struct CursorPolicy {
    pub lifetime: Duration,
    pub page_size: usize,
    pub stream_buffer: usize,
    pub journal_poll_interval: Duration,
}

impl Default for CursorPolicy {
    fn default() -> Self {
        Self {
            lifetime: Duration::from_secs(15 * 60),
            page_size: 128,
            stream_buffer: 256,
            journal_poll_interval: Duration::from_secs(1),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct BenchmarkLogSource {
    pub activity_started_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "purpose", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Position {
    CatalogPage {
        after_work_id: String,
    },
    CatalogResume,
    HistoryPage {
        work_id: Uuid,
        after_timestamp: i64,
        after_row_id: i64,
    },
    ExternalLog {
        work_id: Uuid,
        offset: u64,
        source_run: Option<Uuid>,
        benchmark_source: Option<BenchmarkLogSource>,
    },
    WorkResume {
        work_id: Uuid,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    version: u32,
    owner: Uuid,
    account: Uuid,
    device: Uuid,
    pub generation: Uuid,
    pub revision: i64,
    pub expires_at: i64,
    pub position: Position,
}

pub(crate) struct CursorSigner {
    key: [u8; 32],
    owner: Uuid,
    account: Uuid,
    lifetime: Duration,
}

impl CursorSigner {
    pub fn open(
        path: &Path,
        owner: Uuid,
        account: Uuid,
        policy: &CursorPolicy,
    ) -> Result<Self, ControllerError> {
        if policy.lifetime.as_secs() == 0
            || policy.lifetime.as_secs() > i64::MAX as u64
            || policy.page_size == 0
            || policy.stream_buffer == 0
            || policy.journal_poll_interval.is_zero()
        {
            return Err(ControllerError::unavailable(
                "invalid controller transport policy",
            ));
        }
        let bytes = load_or_create_private(path, || Ok(rand::random::<[u8; 32]>().to_vec()))
            .map_err(|_| ControllerError::unavailable("controller cursor key unavailable"))?;
        let key = bytes.try_into().map_err(|_| {
            ControllerError::unavailable("controller cursor key has invalid length")
        })?;
        Ok(Self {
            key,
            owner,
            account,
            lifetime: policy.lifetime,
        })
    }

    pub fn new_cursor(
        &self,
        device: &AuthenticatedDevice,
        generation: Uuid,
        revision: i64,
        position: Position,
    ) -> Result<Cursor, ControllerError> {
        let expires_at = Utc::now()
            .timestamp()
            .checked_add(self.lifetime.as_secs() as i64)
            .ok_or_else(|| ControllerError::unavailable("cursor expiry overflow"))?;
        Ok(Cursor {
            version: 1,
            owner: self.owner,
            account: self.account,
            device: device.device_id(),
            generation,
            revision,
            expires_at,
            position,
        })
    }

    pub fn encode(&self, cursor: &Cursor) -> Result<String, ControllerError> {
        let bytes = serde_json::to_vec(cursor)
            .map_err(|_| ControllerError::unavailable("cursor serialization failed"))?;
        let mac = blake3::keyed_hash(&self.key, &bytes);
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(&bytes),
            URL_SAFE_NO_PAD.encode(mac.as_bytes())
        ))
    }

    pub fn decode(
        &self,
        device: &AuthenticatedDevice,
        token: &str,
    ) -> Result<Cursor, ControllerError> {
        // Cursor JSON contains only fixed identity/position fields; larger input is not a cursor.
        if token.len() > 4096 {
            return Err(invalid_cursor());
        }
        let (body, signature) = token.split_once('.').ok_or_else(invalid_cursor)?;
        let bytes = URL_SAFE_NO_PAD.decode(body).map_err(|_| invalid_cursor())?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| invalid_cursor())?;
        let expected = blake3::keyed_hash(&self.key, &bytes);
        if !bool::from(expected.as_bytes().as_slice().ct_eq(&signature)) {
            return Err(invalid_cursor());
        }
        let cursor: Cursor = serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())?;
        if cursor.version != 1 || cursor.revision < 0 {
            return Err(invalid_cursor());
        }
        if cursor.owner != self.owner
            || cursor.account != self.account
            || cursor.account != device.account_id()
            || cursor.device != device.device_id()
            || cursor.owner != device.owner_node_id()
        {
            return Err(ControllerError::new(
                StatusCode::FORBIDDEN,
                "cursor_binding_mismatch",
                "cursor belongs to another owner or principal",
                false,
            ));
        }
        if Utc::now().timestamp() >= cursor.expires_at {
            return Err(ControllerError::new(
                StatusCode::GONE,
                "snapshot_required",
                "cursor expired; request a new snapshot",
                false,
            ));
        }
        Ok(cursor)
    }
}

pub(crate) fn invalid_cursor() -> ControllerError {
    ControllerError::new(
        StatusCode::BAD_REQUEST,
        "invalid_cursor",
        "cursor is invalid for this request",
        false,
    )
}
