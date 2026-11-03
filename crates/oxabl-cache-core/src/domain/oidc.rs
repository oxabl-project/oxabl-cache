//! OIDC issuers and workload trust policies.

use crate::domain::scope::Grant;
use crate::ids::{PrincipalId, TenantId};

/// The claim names a registered issuer pins as immutable identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssuerShape {
    /// Tokens like those of GitHub Actions.
    GitHub,
    /// Tokens like those of GitLab CI.
    GitLab,
    /// Any other issuer; the registration declares its id claims.
    Custom,
}

impl IssuerShape {
    /// The protocol name of the shape.
    pub const fn name(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::GitLab => "gitlab",
            Self::Custom => "custom",
        }
    }

    /// Parses a protocol shape name.
    pub fn parse(name: &str) -> Option<Self> {
        [Self::GitHub, Self::GitLab, Self::Custom]
            .into_iter()
            .find(|shape| shape.name() == name)
    }
}

/// An identity provider a tenant trusts to issue ID tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issuer {
    /// The tenant that trusts it.
    pub tenant: TenantId,
    /// The issuer URL, exactly as tokens carry it in `iss`.
    pub url: String,
    /// Which immutable id claims it pins.
    pub shape: IssuerShape,
    /// The declared id claims of a custom issuer; empty for the other shapes.
    pub id_claims: Vec<String>,
    /// Registration time, unix seconds.
    pub created_at: i64,
}

impl Issuer {
    /// The alternatives of immutable id claims: a policy must match every claim of at
    /// least one alternative.
    pub fn id_claim_sets(&self) -> Vec<Vec<&str>> {
        match self.shape {
            IssuerShape::GitHub => vec![
                vec!["repository_id"],
                vec!["repository_owner_id", "repository"],
            ],
            IssuerShape::GitLab => vec![vec!["project_id"], vec!["namespace_id", "project_path"]],
            IssuerShape::Custom => self
                .id_claims
                .iter()
                .map(|claim| vec![claim.as_str()])
                .collect(),
        }
    }

    /// Whether a policy matching exactly `claims` pins an immutable id.
    pub fn pins_identity(&self, claims: &[&str]) -> bool {
        self.id_claim_sets()
            .iter()
            .any(|set| set.iter().all(|claim| claims.contains(claim)))
    }
}

/// An exact-match condition on one claim; only a keyed hash of the value is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimCondition {
    /// The claim name.
    pub claim: String,
    /// The keyed hash of the expected value.
    pub value_hash: [u8; 32],
}

/// A workload identity: the claims a CI job must present and what it then may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadPolicy {
    /// The principal sessions of this policy act as.
    pub id: PrincipalId,
    /// The tenant it belongs to.
    pub tenant: TenantId,
    /// The issuer URL the token must come from.
    pub issuer: String,
    /// The tenant's name for the policy; the natural key.
    pub name: String,
    /// Every condition must hold.
    pub conditions: Vec<ClaimCondition>,
    /// What a matching workload may do.
    pub grants: Vec<Grant>,
    /// Creation time, unix seconds.
    pub created_at: i64,
}
