//! Capability request/grant semantics (Stage 3R R6).
//!
//! A domain declares the capabilities it wants before activation. The
//! Host policy decides the effective grant: the grant can only be equal
//! to or narrower than Host authority, and a domain can never grant
//! itself capability. Enablement never implies authority: the authority
//! set is separate Host state and activation still applies policy.

use crate::domain::failure::DomainFailure;

use std::collections::BTreeSet;

/// Maximum number of capability ids in one request or authority set.
pub const MAX_CAPABILITIES: usize = 32;

/// Maximum length of one capability identifier in bytes.
pub const MAX_CAPABILITY_ID_BYTES: usize = 64;

fn valid_capability_id(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_CAPABILITY_ID_BYTES {
        return false;
    }
    let mut previous_separator = false;
    for (index, byte) in value.bytes().enumerate() {
        let separator = byte == b'-';
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || separator)
            || (separator && (index == 0 || previous_separator))
        {
            return false;
        }
        previous_separator = separator;
    }
    !previous_separator
}

/// A small validated capability identifier (`workspace-read`, for
/// example). The R6 vocabulary is deliberately tiny; future
/// capabilities extend the request vocabulary without widening this
/// boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CapabilityId(String);

impl CapabilityId {
    /// Parse a canonical capability identifier.
    pub fn parse(value: &str) -> Result<Self, DomainFailure> {
        if !valid_capability_id(value) {
            return Err(DomainFailure::InvalidInput {
                reason: "invalid capability id".to_owned(),
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// The canonical identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Ordered, deduplicated set of requested capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRequest {
    ids: BTreeSet<CapabilityId>,
}

impl CapabilityRequest {
    /// Parse and canonicalize a request (validation, dedup, sort).
    pub fn parse(values: &[String]) -> Result<Self, DomainFailure> {
        if values.len() > MAX_CAPABILITIES {
            return Err(DomainFailure::InvalidInput {
                reason: "too many requested capabilities".to_owned(),
            });
        }
        let mut ids = BTreeSet::new();
        for value in values {
            ids.insert(CapabilityId::parse(value)?);
        }
        Ok(Self { ids })
    }

    /// Whether this request asks for the capability.
    pub fn contains(&self, id: &CapabilityId) -> bool {
        self.ids.contains(id)
    }

    /// Ordered iteration over the requested capability ids.
    pub fn iter(&self) -> impl Iterator<Item = &CapabilityId> {
        self.ids.iter()
    }

    /// The number of requested capabilities.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether no capabilities are requested.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// The effective grant: the capability set the Host actually gives an
/// active domain. Always equal to or narrower than Host authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityGrant {
    ids: BTreeSet<CapabilityId>,
}

impl CapabilityGrant {
    /// Whether this grant includes the capability.
    pub fn contains(&self, id: &CapabilityId) -> bool {
        self.ids.contains(id)
    }

    /// Ordered iteration over the granted capability ids.
    pub fn iter(&self) -> impl Iterator<Item = &CapabilityId> {
        self.ids.iter()
    }

    /// Whether nothing was granted.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// The capability set the Host may grant. This is separate Host state:
/// enablement, installation, and activation never widen it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostAuthority {
    ids: BTreeSet<CapabilityId>,
}

impl HostAuthority {
    /// Parse and canonicalize a Host authority set.
    pub fn parse(values: &[String]) -> Result<Self, DomainFailure> {
        if values.len() > MAX_CAPABILITIES {
            return Err(DomainFailure::InvalidInput {
                reason: "too many authority capabilities".to_owned(),
            });
        }
        let mut ids = BTreeSet::new();
        for value in values {
            ids.insert(CapabilityId::parse(value)?);
        }
        Ok(Self { ids })
    }

    /// Whether this authority includes the capability.
    pub fn contains(&self, id: &CapabilityId) -> bool {
        self.ids.contains(id)
    }

    /// Ordered iteration over the authority capability ids.
    pub fn iter(&self) -> impl Iterator<Item = &CapabilityId> {
        self.ids.iter()
    }
}

/// The Host policy decision for one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantDecision {
    /// Every requested capability is within Host authority; the
    /// effective grant equals the request.
    Granted(CapabilityGrant),
    /// Some requested capabilities are outside Host authority, in
    /// canonical order.
    Denied {
        /// The requested capabilities the Host cannot grant.
        missing: Vec<CapabilityId>,
    },
}

/// The deterministic Host policy: fail closed unless every requested
/// capability is within Host authority. The effective grant equals the
/// request and can never be wider than the authority. A denial is
/// typed and never triggers automatic permission escalation.
pub fn decide_grant(
    request: &CapabilityRequest,
    authority: &HostAuthority,
) -> GrantDecision {
    let missing: Vec<CapabilityId> =
        request.iter().filter(|id| !authority.contains(id)).cloned().collect();
    if missing.is_empty() {
        GrantDecision::Granted(CapabilityGrant { ids: request.ids.clone() })
    } else {
        GrantDecision::Denied { missing }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CapabilityId, CapabilityRequest, GrantDecision, HostAuthority,
        decide_grant,
    };

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn request_is_deduped_and_ordered() {
        let request = CapabilityRequest::parse(&ids(&[
            "process-exec",
            "workspace-read",
            "workspace-read",
        ]))
        .unwrap();
        let order: Vec<&str> =
            request.iter().map(CapabilityId::as_str).collect();
        assert_eq!(order, vec!["process-exec", "workspace-read"]);
        assert_eq!(request.len(), 2);
    }

    #[test]
    fn request_rejects_malformed_ids() {
        assert!(CapabilityRequest::parse(&ids(&["workspace read"])).is_err());
        assert!(CapabilityRequest::parse(&ids(&["Workspace-Read"])).is_err());
        assert!(CapabilityRequest::parse(&ids(&[""])).is_err());
    }

    #[test]
    fn grant_equals_request_within_authority() {
        let authority =
            HostAuthority::parse(&ids(&["workspace-read", "process-exec"]))
                .unwrap();
        let request =
            CapabilityRequest::parse(&ids(&["workspace-read"])).unwrap();
        match decide_grant(&request, &authority) {
            GrantDecision::Granted(grant) => {
                let granted: Vec<&str> =
                    grant.iter().map(CapabilityId::as_str).collect();
                assert_eq!(granted, vec!["workspace-read"]);
            }
            GrantDecision::Denied { .. } => panic!("must be granted"),
        }
    }

    #[test]
    fn denial_reports_ordered_missing_capabilities() {
        let authority =
            HostAuthority::parse(&ids(&["workspace-read"])).unwrap();
        let request = CapabilityRequest::parse(&ids(&[
            "workspace-read",
            "process-exec",
            "network-access",
        ]))
        .unwrap();
        match decide_grant(&request, &authority) {
            GrantDecision::Denied { missing } => {
                let missing: Vec<&str> =
                    missing.iter().map(CapabilityId::as_str).collect();
                assert_eq!(missing, vec!["network-access", "process-exec"]);
            }
            GrantDecision::Granted(_) => panic!("must be denied"),
        }
    }

    #[test]
    fn denial_never_widens_authority() {
        let authority =
            HostAuthority::parse(&ids(&["workspace-read"])).unwrap();
        let request =
            CapabilityRequest::parse(&ids(&["process-exec"])).unwrap();
        assert!(matches!(
            decide_grant(&request, &authority),
            GrantDecision::Denied { .. }
        ));
        let authority_ids: Vec<&str> =
            authority.iter().map(CapabilityId::as_str).collect();
        assert_eq!(authority_ids, vec!["workspace-read"]);
    }
}
