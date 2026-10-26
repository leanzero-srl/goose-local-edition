//! Owner destinations come only from the authenticated authority, never from ACP.

use super::{ClientError, ClientResult};
use crate::{controller_wire::*, peer_dial::MeshProxy};
use reqwest::{Client, Method};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
    server::ParsedCertificate,
    DigitallySignedStruct, SignatureScheme,
};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use url::Url;

#[derive(Debug)]
struct OwnerVerifier {
    owner: uuid::Uuid,
    pin: String,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for OwnerVerifier {
    fn verify_server_cert(
        &self,
        leaf: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let cert = ParsedCertificate::try_from(leaf)?;
        let pin = Sha256::digest(cert.subject_public_key_info().as_ref())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if pin != self.pin {
            return Err(rustls::Error::General("owner SPKI pin changed".into()));
        }
        // The worker pin establishes the trust anchor; webpki still enforces leaf
        // validity, serverAuth and the immutable owner SAN, including self-signed keys.
        // https://docs.rs/rustls/0.23.43/rustls/client/fn.verify_server_cert_signed_by_trust_anchor.html
        // https://docs.rs/rustls/0.23.43/rustls/client/fn.verify_server_name.html
        let mut roots = rustls::RootCertStore::empty();
        roots.add(leaf.clone())?;
        rustls::client::verify_server_cert_signed_by_trust_anchor(
            &cert,
            &roots,
            intermediates,
            now,
            self.provider.signature_verification_algorithms.all,
        )?;
        let name = ServerName::try_from(format!("owner-{}.link", self.owner))
            .map_err(|_| rustls::Error::General("invalid owner name".into()))?;
        rustls::client::verify_server_name(&cert, &name)?;
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub(super) struct OwnerTransport {
    client: Client,
    base: Url,
}

pub(super) fn validate_endpoint(
    host: &HostDescriptor,
    endpoint: &MeshEndpoint,
) -> ClientResult<Url> {
    if host.device_role != DeviceRole::Host
        || host.protocol_version != 1
        || host.availability != HostAvailability::Online
        || !host.endpoints.contains(endpoint)
    {
        return Err(ClientError::unavailable(
            "The selected owner is offline or needs an update",
        ));
    }
    let url = Url::parse(&endpoint.url)
        .map_err(|_| ClientError::invalid("Invalid registered owner endpoint"))?;
    let mesh_ip = match url.host() {
        Some(url::Host::Ipv4(ip)) => u32::from(ip) & 0xffc00000 == 0x64400000,
        Some(url::Host::Ipv6(ip)) => ip.octets()[..6] == [0xfd, 0x7a, 0x11, 0x5c, 0xa1, 0xe0],
        _ => false,
    };
    if url.scheme() != "https"
        || !mesh_ip
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || endpoint.tls_spki_sha256.len() != 64
        || !endpoint
            .tls_spki_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ClientError::invalid(
            "Owner endpoint is not a pinned mesh destination",
        ));
    }
    Ok(url)
}

impl OwnerTransport {
    pub(super) fn new(host: &HostDescriptor, proxy: MeshProxy) -> ClientResult<Self> {
        let endpoint = host
            .endpoints
            .first()
            .ok_or_else(|| ClientError::unavailable("Owner has no registered mesh endpoint"))?;
        let base = validate_endpoint(host, endpoint)?;
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let verifier = Arc::new(OwnerVerifier {
            owner: host.owner_node_id,
            pin: endpoint.tls_spki_sha256.clone(),
            provider: provider.clone(),
        });
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|_| ClientError::unavailable("TLS configuration failed"))?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        let client = Client::builder()
            .proxy(
                reqwest::Proxy::all(proxy.proxy_url())
                    .map_err(|_| ClientError::invalid("Invalid mesh proxy"))?,
            )
            .tls_backend_preconfigured(config)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| ClientError::unavailable("Cannot construct pinned owner transport"))?;
        Ok(Self { client, base })
    }

    pub(super) async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        token: &str,
        body: Option<&serde_json::Value>,
    ) -> ClientResult<T> {
        let mut url = self.base.clone();
        url.set_path(&format!("/v1/controller/{path}"));
        if !query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(k, v)| (*k, v)));
        }
        let mut request = self.client.request(method, url).bearer_auth(token);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .map_err(|_| ClientError::unavailable("The pinned owner connection failed"))?;
        let status = response.status();
        if !status.is_success() {
            let envelope = response.json::<serde_json::Value>().await.ok();
            let code = envelope
                .as_ref()
                .and_then(|e| e.pointer("/error/code"))
                .and_then(|v| v.as_str())
                .unwrap_or("owner_request_failed");
            return Err(ClientError {
                code: code.into(),
                message: format!("The owner refused the request ({})", status.as_u16()),
                retryable: status.is_server_error(),
                status: status.as_u16(),
            });
        }
        response
            .json()
            .await
            .map_err(|_| ClientError::invalid("The owner returned an invalid controller response"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller_tls::ControllerTlsIdentity;
    use std::net::IpAddr;

    #[test]
    fn owner_destination_rejects_plaintext_userinfo_dns_nonmesh_and_redirect_paths() {
        let endpoint = MeshEndpoint {
            url: "https://100.64.0.1:41227".into(),
            tls_spki_sha256: "a".repeat(64),
        };
        let mut host = HostDescriptor {
            owner_node_id: uuid::Uuid::new_v4(),
            name: "Owner".into(),
            device_role: DeviceRole::Host,
            protocol_version: 1,
            availability: HostAvailability::Online,
            last_seen_at: None,
            endpoints: vec![endpoint.clone()],
            capabilities: vec![ControllerScope::CatalogRead],
        };
        assert!(validate_endpoint(&host, &endpoint).is_ok());
        for url in [
            "http://100.64.0.1:41227",
            "https://127.0.0.1:41227",
            "https://example.com:41227",
            "https://user:secret@100.64.0.1:41227",
            "https://100.64.0.1:41227/redirect",
            "https://100.64.0.1:41227?token=secret",
            "https://100.64.0.1:41227#other",
        ] {
            let bad = MeshEndpoint {
                url: url.into(),
                ..endpoint.clone()
            };
            host.endpoints = vec![bad.clone()];
            assert!(validate_endpoint(&host, &bad).is_err(), "accepted {url}");
        }
        host.endpoints = vec![endpoint.clone()];
        host.availability = HostAvailability::Offline;
        assert!(validate_endpoint(&host, &endpoint).is_err());
        host.availability = HostAvailability::Online;
        host.device_role = DeviceRole::Controller;
        assert!(validate_endpoint(&host, &endpoint).is_err());
    }
    #[test]
    fn pin_san_and_expiry_are_all_required() {
        let directory = tempfile::tempdir().unwrap();
        let owner = uuid::Uuid::new_v4();
        let identity =
            ControllerTlsIdentity::load_or_create(directory.path().join("key"), owner).unwrap();
        let mut verifier = OwnerVerifier {
            owner,
            pin: identity.spki_sha256(),
            provider: Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
        };
        let leaf = CertificateDer::from(identity.certificate_der());
        let name = ServerName::IpAddress(IpAddr::from([100, 64, 0, 1]).into());
        assert!(verifier
            .verify_server_cert(&leaf, &[], &name, &[], UnixTime::now())
            .is_ok());
        verifier.owner = uuid::Uuid::new_v4();
        assert!(verifier
            .verify_server_cert(&leaf, &[], &name, &[], UnixTime::now())
            .is_err());
        verifier.owner = owner;
        verifier.pin = "0".repeat(64);
        assert!(verifier
            .verify_server_cert(&leaf, &[], &name, &[], UnixTime::now())
            .is_err());
        verifier.pin = identity.spki_sha256();
        assert!(verifier
            .verify_server_cert(
                &leaf,
                &[],
                &name,
                &[],
                UnixTime::since_unix_epoch(Duration::ZERO)
            )
            .is_err());
        assert!(verifier
            .verify_server_cert(
                &leaf,
                &[],
                &name,
                &[],
                UnixTime::since_unix_epoch(Duration::from_secs(8_000_000_000))
            )
            .is_err());
    }
}
