//! The capability token (PL-4.5, contracts section 5): a Biscuit minted by
//! the orchestrator with the facts `job`, `agent`, `cap`, `h`, `lineage`,
//! `plan_runs`, `grid_points` and `expires`, verified here offline with the
//! public key. The gateway adds `time(now)` and requires `cap("<route>")`;
//! what the token does not say, the gateway refuses.

use std::collections::BTreeSet;
use std::path::Path;

use biscuit_auth::builder::{fact, Term};
use biscuit_auth::{AuthorizerBuilder, Biscuit, KeyPair, PublicKey};

/// What a verified token grants: the job's identity and budgets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub job: String,
    pub agent: String,
    pub caps: BTreeSet<String>,
    /// The information horizon, nanoseconds since the epoch (Definition PL-4.1).
    pub h: i64,
    /// The lineage's id (the root program's hash), or empty when the job
    /// opens one.
    pub lineage: String,
    pub plan_runs: u32,
    pub grid_points: u32,
    /// Nanoseconds since the epoch.
    pub expires: i64,
}

impl Grant {
    pub fn has(&self, cap: &str) -> bool {
        self.caps.contains(cap)
    }

    /// The facts of the token, as the orchestrator writes them (contracts
    /// section 5): `time` and `expires` are nanoseconds, integers, so the
    /// token's own check compares like with like.
    pub fn facts(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("job({});\n", quoted(&self.job)));
        s.push_str(&format!("agent({});\n", quoted(&self.agent)));
        for c in &self.caps {
            s.push_str(&format!("cap({});\n", quoted(c)));
        }
        s.push_str(&format!("h({});\n", self.h));
        s.push_str(&format!("lineage({});\n", quoted(&self.lineage)));
        s.push_str(&format!("plan_runs({});\n", self.plan_runs));
        s.push_str(&format!("grid_points({});\n", self.grid_points));
        s.push_str(&format!("expires({});\n", self.expires));
        s.push_str("check if time($t), expires($e), $t < $e;\n");
        s
    }
}

fn quoted(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Why a token was not accepted: a 403 either way, the message says which.
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("the token does not verify: {0}")]
    Invalid(String),
    #[error("the token lacks {0}")]
    Lacks(String),
}

/// Verifies tokens with the orchestrator's public key.
#[derive(Clone)]
pub struct Verifier {
    key: PublicKey,
}

impl Verifier {
    pub fn new(key: PublicKey) -> Verifier {
        Verifier { key }
    }

    /// The public key as `token_public_key` holds it (contracts section 5):
    /// PEM, or the 32 bytes of an ed25519 key as 64 hex digits (with or
    /// without the `ed25519/` prefix Biscuit prints), or raw.
    pub fn from_file(path: &Path) -> Result<Verifier, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        Verifier::from_bytes(&bytes).map_err(|e| format!("{}: {}", path.display(), e))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Verifier, String> {
        let text = String::from_utf8_lossy(bytes);
        let text = text.trim();
        let ed = |hex: &str| -> Result<PublicKey, String> {
            if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("not an ed25519 public key: 64 hex digits expected".into());
            }
            let raw: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap_or(0)).collect();
            PublicKey::from_bytes(&raw, biscuit_auth::Algorithm::Ed25519).map_err(|e| format!("not an ed25519 public key: {}", e))
        };
        let key = if text.contains("-----BEGIN") {
            PublicKey::from_pem(text).map_err(|e| format!("not a PEM public key: {}", e))?
        } else if let Some(hex) = text.strip_prefix("ed25519/") {
            ed(hex)?
        } else if text.len() == 64 {
            ed(text)?
        } else if bytes.len() == 32 {
            PublicKey::from_bytes(bytes, biscuit_auth::Algorithm::Ed25519).map_err(|e| format!("not an ed25519 public key: {}", e))?
        } else {
            return Err("not a public key: PEM, 64 hex digits or 32 raw bytes".into());
        };
        Ok(Verifier { key })
    }

    pub fn key(&self) -> PublicKey {
        self.key
    }

    /// The grant of a token, verified at `now` (nanoseconds) and holding
    /// `cap` when one is required: the signature, the token's own checks
    /// (its expiry against `time(now)`), then its facts.
    pub fn verify(&self, token_base64: &str, now: i64, cap: Option<&str>) -> Result<Grant, TokenError> {
        let invalid = |e: biscuit_auth::error::Token| TokenError::Invalid(e.to_string());
        let token = Biscuit::from_base64(token_base64.trim(), self.key).map_err(invalid)?;
        let mut builder = AuthorizerBuilder::new().fact(fact("time", &[Term::Integer(now)])).map_err(invalid)?;
        if let Some(c) = cap {
            builder = builder.check(format!("check if cap({})", quoted(c)).as_str()).map_err(invalid)?;
        }
        let mut authorizer = builder.policy("allow if true").map_err(invalid)?.build(&token).map_err(invalid)?;
        if let Err(e) = authorizer.authorize() {
            let msg = e.to_string();
            return Err(match cap {
                Some(c) if msg.contains("cap(") => TokenError::Lacks(format!("the capability {}", c)),
                _ => TokenError::Lacks(format!("a passing check: {}", msg)),
            });
        }
        let texts = |a: &mut biscuit_auth::Authorizer, name: &str| -> Result<Vec<String>, TokenError> {
            let v: Vec<(String,)> = a.query(format!("data($x) <- {}($x)", name).as_str()).map_err(|e| TokenError::Invalid(e.to_string()))?;
            Ok(v.into_iter().map(|(s,)| s).collect())
        };
        let ints = |a: &mut biscuit_auth::Authorizer, name: &str| -> Result<Vec<i64>, TokenError> {
            let v: Vec<(i64,)> = a.query(format!("data($x) <- {}($x)", name).as_str()).map_err(|e| TokenError::Invalid(e.to_string()))?;
            Ok(v.into_iter().map(|(n,)| n).collect())
        };
        let lacks = |name: &str| TokenError::Lacks(format!("the fact {}", name));
        let job = texts(&mut authorizer, "job")?.into_iter().next().ok_or_else(|| lacks("job"))?;
        let agent = texts(&mut authorizer, "agent")?.into_iter().next().ok_or_else(|| lacks("agent"))?;
        let caps = texts(&mut authorizer, "cap")?;
        let h = ints(&mut authorizer, "h")?.into_iter().next().ok_or_else(|| lacks("h"))?;
        let lineage = texts(&mut authorizer, "lineage")?.into_iter().next().unwrap_or_default();
        let plan_runs = ints(&mut authorizer, "plan_runs")?.into_iter().next().unwrap_or(0);
        let grid_points = ints(&mut authorizer, "grid_points")?.into_iter().next().unwrap_or(0);
        let expires = ints(&mut authorizer, "expires")?.into_iter().next().ok_or_else(|| lacks("expires"))?;
        if now >= expires {
            return Err(TokenError::Lacks("time before its expiry".into()));
        }
        Ok(Grant {
            job,
            agent,
            caps: caps.into_iter().collect(),
            h,
            lineage,
            plan_runs: plan_runs.clamp(0, u32::MAX as i64) as u32,
            grid_points: grid_points.clamp(0, u32::MAX as i64) as u32,
            expires,
        })
    }
}

/// A token with the grant's facts, signed by `root` (what the orchestrator
/// does; here for the tests and `abt-gateway token`).
pub fn mint(root: &KeyPair, grant: &Grant) -> Result<String, String> {
    Biscuit::builder()
        .code(grant.facts())
        .map_err(|e| e.to_string())?
        .build(root)
        .map_err(|e| e.to_string())?
        .to_base64()
        .map_err(|e| e.to_string())
}

/// The token's base64 from an `Authorization: Biscuit <base64>` header.
pub fn from_header(value: &str) -> Option<&str> {
    let (scheme, rest) = value.trim().split_once(' ')?;
    scheme.eq_ignore_ascii_case("biscuit").then(|| rest.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant() -> Grant {
        Grant {
            job: "job-1".into(),
            agent: "quant".into(),
            caps: ["abt.query", "abt.catalog"].iter().map(|s| s.to_string()).collect(),
            h: 1_700_000_000_000_000_000,
            lineage: String::new(),
            plan_runs: 4,
            grid_points: 25,
            expires: 2_000_000_000_000_000_000,
        }
    }

    #[test]
    fn a_minted_token_verifies_with_its_facts_and_its_capability() {
        let root = KeyPair::new();
        let t = mint(&root, &grant()).unwrap();
        let v = Verifier::new(root.public());
        let g = v.verify(&t, 1_800_000_000_000_000_000, Some("abt.query")).unwrap();
        assert_eq!(g, grant());
        assert!(matches!(v.verify(&t, 1_800_000_000_000_000_000, Some("abt.plan")), Err(TokenError::Lacks(_))));
        assert!(matches!(v.verify(&t, 2_100_000_000_000_000_000, Some("abt.query")), Err(TokenError::Lacks(_))));
        let other = KeyPair::new();
        assert!(matches!(Verifier::new(other.public()).verify(&t, 1_800_000_000_000_000_000, None), Err(TokenError::Invalid(_))));
        assert!(matches!(v.verify("not a token", 1, None), Err(TokenError::Invalid(_))));
        assert_eq!(from_header("Biscuit abc"), Some("abc"));
        assert_eq!(from_header("Bearer abc"), None);
    }

    #[test]
    fn the_public_key_reads_as_pem_or_hex() {
        let root = KeyPair::new();
        let pem = root.public().to_pem().unwrap();
        assert_eq!(Verifier::from_bytes(pem.as_bytes()).unwrap().key(), root.public());
        let hex: String = root.public().to_bytes().iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(Verifier::from_bytes(hex.as_bytes()).unwrap().key(), root.public());
        assert_eq!(Verifier::from_bytes(format!("ed25519/{}", hex).as_bytes()).unwrap().key(), root.public());
        assert!(Verifier::from_bytes(b"nonsense").is_err());
    }
}
