//! Authoritative mesh-role admission shared by every compute consumer through PeerRegistry.

use crate::{
    controller_wire::{DeviceRole, RegisteredPeers},
    mesh::MeshPeer,
};
use std::collections::HashMap;
use tokio::time::Instant;

#[derive(Debug, Clone, Default)]
pub enum PeerAdmission {
    /// Compatibility mode is only valid before controller enrollment is enabled.
    #[default]
    LegacyFleet,
    /// Missing registry, missing key and unknown peers are all excluded before polling.
    RegisteredHosts {
        revision: u64,
        roles: HashMap<String, DeviceRole>,
        deadline: Instant,
    },
}

impl PeerAdmission {
    pub fn registered(
        snapshot: RegisteredPeers,
        request_started: Instant,
    ) -> Result<Self, &'static str> {
        if snapshot.protocol_version != 1 || snapshot.lease_seconds == 0 {
            return Err("registry returned an unsupported protocol or empty lease");
        }
        let deadline = request_started
            + std::time::Duration::from_secs(u64::from(snapshot.lease_seconds))
                .min(crate::controller_auth::MAX_AUTHORIZATION_LEASE);
        if deadline <= Instant::now() {
            return Err("registry lease expired during lookup");
        }
        let mut roles = HashMap::new();
        for peer in snapshot.peers {
            if peer.mesh_node_key.is_empty()
                || (peer.role == DeviceRole::Host && peer.owner_node_id.is_none())
            {
                return Err("registry peer has no authoritative identity");
            }
            if roles.insert(peer.mesh_node_key, peer.role).is_some() {
                return Err("registry contains duplicate mesh identity");
            }
        }
        Ok(Self::RegisteredHosts {
            revision: snapshot.revision,
            roles,
            deadline,
        })
    }

    pub fn admits(&self, mesh_node_key: Option<&str>) -> bool {
        match self {
            Self::LegacyFleet => true,
            Self::RegisteredHosts {
                roles, deadline, ..
            } => {
                Instant::now() < *deadline
                    && mesh_node_key.and_then(|key| roles.get(key)) == Some(&DeviceRole::Host)
            }
        }
    }

    pub fn admits_peer(&self, peer: &MeshPeer) -> bool {
        self.admits(peer.mesh_node_key.as_deref())
    }

    /// An accepted operation retains its host identity through lease expiry, but a
    /// later authoritative role change or removal revokes that continuation.
    pub(crate) fn continues(&self, mesh_node_key: Option<&str>) -> bool {
        match self {
            Self::LegacyFleet => true,
            Self::RegisteredHosts { roles, .. } => {
                mesh_node_key.and_then(|key| roles.get(key)) == Some(&DeviceRole::Host)
            }
        }
    }

    /// An authority outage cannot return the upgraded registry to legacy permissive mode.
    pub fn authority_unavailable(&mut self) {
        match self {
            Self::RegisteredHosts { deadline, .. } => *deadline = Instant::now(),
            Self::LegacyFleet => {
                *self = Self::RegisteredHosts {
                    revision: 0,
                    roles: HashMap::new(),
                    deadline: Instant::now(),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller_wire::RegisteredPeer;
    #[test]
    fn admission_uses_provider_key_and_excludes_controllers_and_unknowns() {
        let policy = PeerAdmission::registered(
            RegisteredPeers {
                protocol_version: 1,
                lease_seconds: 30,
                revision: 1,
                peers: vec![
                    RegisteredPeer {
                        mesh_node_key: "nodekey:host".into(),
                        role: DeviceRole::Host,
                        owner_node_id: Some(uuid::Uuid::new_v4()),
                    },
                    RegisteredPeer {
                        mesh_node_key: "nodekey:phone".into(),
                        role: DeviceRole::Controller,
                        owner_node_id: None,
                    },
                ],
            },
            Instant::now(),
        )
        .unwrap();
        assert!(policy.admits(Some("nodekey:host")));
        assert!(!policy.admits(Some("nodekey:phone")));
        assert!(!policy.admits(Some("nodekey:unknown")));
        assert!(!policy.admits(None));
        let mut lost = policy;
        lost.authority_unavailable();
        assert!(!lost.admits(Some("nodekey:host")));
    }
}
