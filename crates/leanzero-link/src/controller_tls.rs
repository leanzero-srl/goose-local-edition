//! TLS for controller traffic, authenticated by the worker-bound owner SPKI pin.

use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use rcgen::{CertificateParams, ExtendedKeyUsagePurpose, KeyPair, KeyUsagePurpose, PublicKeyData};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::owner_identity::{invalid, load_or_create_private, OwnerIdentityError};

// Certificate lifecycle policy, independent of sessions or model execution.
const CERTIFICATE_DAYS: i64 = 90;
const RENEWAL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

#[derive(Error, Debug)]
pub enum ControllerTlsError {
    #[error(transparent)]
    Identity(#[from] OwnerIdentityError),
    #[error("cannot issue controller TLS certificate: {0}")]
    Certificate(#[from] rcgen::Error),
    #[error("controller TLS configuration failed: {0}")]
    Configuration(#[from] rustls::Error),
    #[error("controller TLS listener failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("controller TLS task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredKey {
    version: u32,
    owner_node_id: Uuid,
    private_key_pem: String,
}

#[derive(Clone)]
pub struct ControllerTlsIdentity {
    owner_node_id: Uuid,
    key: Arc<KeyPair>,
    certificate: Vec<u8>,
}

impl ControllerTlsIdentity {
    pub fn load_or_create(
        path: impl AsRef<Path>,
        owner_node_id: Uuid,
    ) -> Result<Self, ControllerTlsError> {
        let path = path.as_ref();
        if owner_node_id.get_version_num() != 4 {
            return Err(invalid(path, "owner must be a version 4 UUID").into());
        }
        let bytes = load_or_create_private(path, || {
            let key = KeyPair::generate().map_err(|error| invalid(path, error.to_string()))?;
            serde_json::to_vec(&StoredKey {
                version: 1,
                owner_node_id,
                private_key_pem: key.serialize_pem(),
            })
            .map_err(|source| OwnerIdentityError::Json {
                path: path.to_path_buf(),
                source,
            })
        })?;
        let stored: StoredKey =
            serde_json::from_slice(&bytes).map_err(|source| OwnerIdentityError::Json {
                path: path.to_path_buf(),
                source,
            })?;
        if stored.version != 1 || stored.owner_node_id != owner_node_id {
            return Err(invalid(path, "TLS key belongs to a different owner or version").into());
        }
        let key = Arc::new(KeyPair::from_pem(&stored.private_key_pem)?);
        let certificate = issue_certificate(owner_node_id, &key)?;
        Ok(Self {
            owner_node_id,
            key,
            certificate,
        })
    }

    pub fn spki_sha256(&self) -> String {
        Sha256::digest(self.key.subject_public_key_info())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub fn server_name(&self) -> String {
        format!("owner-{}.link", self.owner_node_id)
    }

    pub fn certificate_der(&self) -> &[u8] {
        &self.certificate
    }

    fn server_config(
        &self,
        certificate: Vec<u8>,
    ) -> Result<Arc<rustls::ServerConfig>, ControllerTlsError> {
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificate)],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.serialize_der())),
        )?;
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}

fn issue_certificate(owner_node_id: Uuid, key: &KeyPair) -> Result<Vec<u8>, rcgen::Error> {
    let mut params = CertificateParams::new(vec![format!("owner-{owner_node_id}.link")])?;
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::minutes(5);
    params.not_after = now + time::Duration::days(CERTIFICATE_DAYS);
    params.serial_number = Some(rcgen::SerialNumber::from_slice(Uuid::new_v4().as_bytes()));
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    Ok(params.self_signed(key)?.der().to_vec())
}

pub struct ControllerTlsHandle {
    pub local_addr: SocketAddr,
    handle: axum_server::Handle<SocketAddr>,
    task: Option<tokio::task::JoinHandle<Result<(), ControllerTlsError>>>,
}

impl ControllerTlsHandle {
    pub fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(|task| task.is_finished())
    }

    pub async fn shutdown(mut self) -> Result<(), ControllerTlsError> {
        self.handle.shutdown();
        if let Some(task) = self.task.take() {
            task.await??;
        }
        Ok(())
    }
}

impl Drop for ControllerTlsHandle {
    fn drop(&mut self) {
        self.handle.shutdown();
    }
}

pub async fn start_controller_tls(
    address: SocketAddr,
    router: Router,
    identity: ControllerTlsIdentity,
) -> Result<ControllerTlsHandle, ControllerTlsError> {
    let listener = TcpListener::bind(address)?;
    listener.set_nonblocking(true)?;
    let config = RustlsConfig::from_config(identity.server_config(identity.certificate.clone())?);
    let handle = axum_server::Handle::new();
    let server = axum_server::from_tcp_rustls(listener, config.clone())?.handle(handle.clone());
    let task_handle = handle.clone();
    let task = tokio::spawn(async move {
        let renew = async {
            let mut interval = tokio::time::interval_at(
                tokio::time::Instant::now() + RENEWAL_INTERVAL,
                RENEWAL_INTERVAL,
            );
            loop {
                interval.tick().await;
                let certificate = issue_certificate(identity.owner_node_id, &identity.key)?;
                config.reload_from_config(identity.server_config(certificate)?);
            }
            #[allow(unreachable_code)]
            Ok::<(), ControllerTlsError>(())
        };
        let result = tokio::select! {
            result = server.serve(router.into_make_service()) => result.map_err(ControllerTlsError::Io),
            result = renew => result,
        };
        task_handle.shutdown();
        if let Err(error) = &result {
            tracing::error!(%error, "controller TLS listener stopped");
        }
        result
    });
    let Some(local_addr) = handle.listening().await else {
        task.await??;
        return Err(std::io::Error::other("controller TLS listener never became ready").into());
    };
    Ok(ControllerTlsHandle {
        local_addr,
        handle,
        task: Some(task),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;

    #[test]
    fn key_and_pin_survive_restart_but_cannot_change_owner() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tls.json");
        let owner = Uuid::new_v4();
        let first = ControllerTlsIdentity::load_or_create(&path, owner).unwrap();
        let second = ControllerTlsIdentity::load_or_create(&path, owner).unwrap();
        assert_eq!(first.spki_sha256(), second.spki_sha256());
        assert_eq!(first.spki_sha256().len(), 64);
        assert_ne!(first.certificate_der(), second.certificate_der());
        assert!(ControllerTlsIdentity::load_or_create(&path, Uuid::new_v4()).is_err());
        assert_eq!(
            ControllerTlsIdentity::load_or_create(&path, owner)
                .unwrap()
                .spki_sha256(),
            first.spki_sha256()
        );
    }

    #[tokio::test]
    async fn trusted_owner_serves_https_and_wrong_names_keys_and_plaintext_fail() {
        let directory = tempfile::tempdir().unwrap();
        let identity = ControllerTlsIdentity::load_or_create(
            directory.path().join("tls.json"),
            Uuid::new_v4(),
        )
        .unwrap();
        let router =
            Router::new().route("/proof", get(|| async { "authenticated owner transport" }));
        let server = start_controller_tls("127.0.0.1:0".parse().unwrap(), router, identity.clone())
            .await
            .unwrap();
        let name = identity.server_name();
        let client = reqwest::Client::builder()
            .no_proxy()
            .tls_certs_only([reqwest::Certificate::from_der(identity.certificate_der()).unwrap()])
            .resolve(&name, server.local_addr)
            .build()
            .unwrap();
        let url = format!("https://{name}:{}/proof", server.local_addr.port());
        assert_eq!(
            client.get(&url).send().await.unwrap().text().await.unwrap(),
            "authenticated owner transport"
        );
        assert!(client
            .get(format!("https://{}/proof", server.local_addr))
            .send()
            .await
            .is_err());
        let other = ControllerTlsIdentity::load_or_create(
            directory.path().join("other.json"),
            Uuid::new_v4(),
        )
        .unwrap();
        let wrong_key = reqwest::Client::builder()
            .no_proxy()
            .tls_certs_only([reqwest::Certificate::from_der(other.certificate_der()).unwrap()])
            .resolve(&name, server.local_addr)
            .build()
            .unwrap();
        assert!(wrong_key.get(&url).send().await.is_err());
        assert!(client
            .get(format!("http://{}/proof", server.local_addr))
            .send()
            .await
            .is_err());
        let address = server.local_addr;
        server.shutdown().await.unwrap();
        assert!(tokio::net::TcpStream::connect(address).await.is_err());
    }
}
