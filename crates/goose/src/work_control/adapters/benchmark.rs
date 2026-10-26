use super::{error, WorkAdapters, LOG_PAGE_BYTES};
use crate::session::work_catalog::{CatalogRecord, WorkCatalog};
use leanzero_link::{controller::ControllerError, controller_wire::CommandReceipt};
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Hash, Eq, PartialEq)]
pub(super) struct ReceiptScanKey {
    device: Uuid,
    command: Uuid,
    work: Uuid,
    run: Uuid,
}

impl WorkAdapters {
    pub(super) async fn benchmark_note_delivered(
        &self,
        catalog: &WorkCatalog,
        record: &CatalogRecord,
        receipt: &CommandReceipt,
    ) -> Result<bool, ControllerError> {
        let desktop = self
            .desktop
            .as_ref()
            .ok_or_else(|| error("benchmark desktop owner is unavailable"))?;
        let run = receipt
            .run_id
            .ok_or_else(|| error("benchmark receipt has no run"))?;
        let key: Value = serde_json::from_str(
            &catalog
                .external_run_key(&record.work_id, &run.to_string())
                .await
                .map_err(error)?,
        )
        .map_err(error)?;
        let mut params: Value = serde_json::from_str(&record.local_locator).map_err(error)?;
        params["run_id"] = key["run_id"].clone();
        params["activity_started_at"] = key["activity_started_at"].clone();
        params["command_id"] = json!(receipt.command_id);
        params["checkpoint_id"] = json!(receipt
            .checkpoint_id
            .as_deref()
            .ok_or_else(|| error("benchmark receipt has no checkpoint"))?);
        let scan_key = ReceiptScanKey {
            device: receipt.authenticated_device_id,
            command: receipt.command_id,
            work: receipt.work.work_id,
            run,
        };
        let mut offsets = self.benchmark_receipt_offsets.lock().await;
        let from = offsets.get(&scan_key).copied().unwrap_or(0);
        params["from"] = json!(from);
        let result = desktop.call("benchmark.receipt", params).await?;
        let next = result["next"]
            .as_u64()
            .ok_or_else(|| error("desktop returned no receipt scan cursor"))?;
        if next < from || next - from > LOG_PAGE_BYTES {
            return Err(error("desktop returned an invalid receipt scan interval"));
        }
        match result["state"].as_str() {
            Some("delivered") => {
                offsets.remove(&scan_key);
                Ok(true)
            }
            Some("waiting") => {
                offsets.insert(scan_key, next);
                Ok(false)
            }
            _ => Err(error("desktop returned no receipt delivery state")),
        }
    }
}
