use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

// Replace with PUBLIC_KEY_B64 printed by: cargo run --bin civitas-license -- keygen --out <DIR>
pub const CIVITAS_LICENSE_PUBKEY_B64: &str = "IS9OAzRToPjHkCfCWkSUBqEn4ANo78N/Ug9ecr+tMHY=";
pub const ENTITLEMENTS_SCHEMA_VERSION: &str = "civitas.entitlements.v1";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EntitlementsStatusKind {
    Active,
    Expired,
    Invalid,
    Missing,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementsStatusResponse {
    pub status: EntitlementsStatusKind,
    pub plan: Option<String>,
    pub enabled_libraries: Vec<String>,
    pub expires_at: Option<String>,
    pub read_only: bool,
    pub source_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SignedEntitlements {
    schema_version: String,
    customer_id: String,
    order_id: String,
    plan: String,
    entitled_libraries: Vec<String>,
    bonus_libraries: Vec<String>,
    issued_at: String,
    expires_at: Option<String>,
    byol_enabled: bool,
    signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct EntitlementsPayload {
    schema_version: String,
    customer_id: String,
    order_id: String,
    plan: String,
    entitled_libraries: Vec<String>,
    bonus_libraries: Vec<String>,
    issued_at: String,
    expires_at: Option<String>,
    byol_enabled: bool,
}

impl From<&SignedEntitlements> for EntitlementsPayload {
    fn from(value: &SignedEntitlements) -> Self {
        Self {
            schema_version: value.schema_version.clone(),
            customer_id: value.customer_id.clone(),
            order_id: value.order_id.clone(),
            plan: value.plan.clone(),
            entitled_libraries: value.entitled_libraries.clone(),
            bonus_libraries: value.bonus_libraries.clone(),
            issued_at: value.issued_at.clone(),
            expires_at: value.expires_at.clone(),
            byol_enabled: value.byol_enabled,
        }
    }
}

#[derive(Debug)]
struct VerifiedEntitlements {
    payload: EntitlementsPayload,
    expires_at_utc: Option<DateTime<Utc>>,
    enabled_libraries: Vec<String>,
}

pub fn get_entitlements_status(now: DateTime<Utc>) -> EntitlementsStatusResponse {
    let source_path = resolve_entitlements_path();
    get_entitlements_status_from_path(&source_path, now, CIVITAS_LICENSE_PUBKEY_B64)
}

fn resolve_entitlements_path() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            return exe_dir.join("config").join("entitlements.json");
        }
    }
    PathBuf::from("config").join("entitlements.json")
}

fn get_entitlements_status_from_path(
    source_path: &Path,
    now: DateTime<Utc>,
    pubkey_b64: &str,
) -> EntitlementsStatusResponse {
    let source_path_text = source_path.to_string_lossy().to_string();
    if !source_path.is_file() {
        return EntitlementsStatusResponse {
            status: EntitlementsStatusKind::Missing,
            plan: None,
            enabled_libraries: Vec::new(),
            expires_at: None,
            read_only: true,
            source_path: source_path_text,
            message: "Unlicensed: config/entitlements.json not found.".to_string(),
        };
    }

    let signed_json = match fs::read_to_string(source_path) {
        Ok(value) => value,
        Err(err) => {
            return EntitlementsStatusResponse {
                status: EntitlementsStatusKind::Invalid,
                plan: None,
                enabled_libraries: Vec::new(),
                expires_at: None,
                read_only: true,
                source_path: source_path_text,
                message: format!("License invalid: failed to read entitlements.json ({err})."),
            };
        }
    };

    let verified = match parse_and_verify_entitlements(&signed_json, pubkey_b64) {
        Ok(value) => value,
        Err(err) => {
            return EntitlementsStatusResponse {
                status: EntitlementsStatusKind::Invalid,
                plan: None,
                enabled_libraries: Vec::new(),
                expires_at: None,
                read_only: true,
                source_path: source_path_text,
                message: format!("License invalid: {err}"),
            };
        }
    };

    let is_expired = verified
        .expires_at_utc
        .is_some_and(|expires_at| now > expires_at);
    let status = if is_expired {
        EntitlementsStatusKind::Expired
    } else {
        EntitlementsStatusKind::Active
    };
    let message = if is_expired {
        "License expired: viewer is in read-only mode.".to_string()
    } else {
        "License active.".to_string()
    };

    EntitlementsStatusResponse {
        status,
        plan: Some(verified.payload.plan),
        enabled_libraries: verified.enabled_libraries,
        expires_at: verified.expires_at_utc.map(format_rfc3339_utc),
        read_only: is_expired,
        source_path: source_path_text,
        message,
    }
}

fn parse_and_verify_entitlements(
    signed_json: &str,
    pubkey_b64: &str,
) -> Result<VerifiedEntitlements, String> {
    let signed: SignedEntitlements =
        serde_json::from_str(signed_json).map_err(|err| format!("invalid JSON payload ({err})"))?;
    let payload = EntitlementsPayload::from(&signed);
    validate_payload_shape(&payload)?;

    let payload_string = serde_json::to_string(&payload)
        .map_err(|err| format!("failed to serialize payload for verification ({err})"))?;
    verify_signature(
        payload_string.as_bytes(),
        signed.signature.trim(),
        pubkey_b64.trim(),
    )?;

    let expires_at_utc = payload
        .expires_at
        .as_ref()
        .map(|value| parse_rfc3339_utc(value, "expires_at"))
        .transpose()?;

    Ok(VerifiedEntitlements {
        enabled_libraries: merge_enabled_libraries(&payload),
        payload,
        expires_at_utc,
    })
}

fn validate_payload_shape(payload: &EntitlementsPayload) -> Result<(), String> {
    if payload.schema_version != ENTITLEMENTS_SCHEMA_VERSION {
        return Err(format!(
            "schema_version mismatch (expected {ENTITLEMENTS_SCHEMA_VERSION}, got {})",
            payload.schema_version
        ));
    }
    if payload.customer_id.trim().is_empty() {
        return Err("customer_id is empty".to_string());
    }
    if payload.order_id.trim().is_empty() {
        return Err("order_id is empty".to_string());
    }
    if payload.plan.trim().is_empty() {
        return Err("plan is empty".to_string());
    }
    parse_rfc3339_utc(&payload.issued_at, "issued_at")?;
    Ok(())
}

fn verify_signature(
    payload_bytes: &[u8],
    signature_b64: &str,
    pubkey_b64: &str,
) -> Result<(), String> {
    let verifying_key = decode_verifying_key(pubkey_b64)?;
    let signature = decode_signature(signature_b64)?;
    verifying_key
        .verify(payload_bytes, &signature)
        .map_err(|_| "signature verification failed".to_string())
}

fn decode_verifying_key(pubkey_b64: &str) -> Result<VerifyingKey, String> {
    let key_bytes = BASE64_STANDARD
        .decode(pubkey_b64)
        .map_err(|err| format!("invalid base64 public key ({err})"))?;
    let key_array: [u8; 32] = key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "public key must decode to exactly 32 bytes".to_string())?;
    VerifyingKey::from_bytes(&key_array).map_err(|err| format!("invalid public key bytes ({err})"))
}

fn decode_signature(signature_b64: &str) -> Result<Signature, String> {
    let signature_bytes = BASE64_STANDARD
        .decode(signature_b64)
        .map_err(|err| format!("invalid base64 signature ({err})"))?;
    Signature::from_slice(&signature_bytes)
        .map_err(|err| format!("signature must decode to 64 bytes ({err})"))
}

fn parse_rfc3339_utc(value: &str, field_name: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map_err(|err| format!("{field_name} must be RFC3339 ({err})"))
        .map(|value| value.with_timezone(&Utc))
}

fn format_rfc3339_utc(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn merge_enabled_libraries(payload: &EntitlementsPayload) -> Vec<String> {
    let mut merged = BTreeSet::new();
    for value in payload
        .entitled_libraries
        .iter()
        .chain(payload.bonus_libraries.iter())
    {
        let normalized = value.trim();
        if !normalized.is_empty() {
            merged.insert(normalized.to_string());
        }
    }
    merged.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn sample_payload() -> EntitlementsPayload {
        EntitlementsPayload {
            schema_version: ENTITLEMENTS_SCHEMA_VERSION.to_string(),
            customer_id: "cust_001".to_string(),
            order_id: "ord_001".to_string(),
            plan: "EU_DUO".to_string(),
            entitled_libraries: vec![
                "vendorsecurity/v1".to_string(),
                "iso27001-lite/v1".to_string(),
            ],
            bonus_libraries: vec!["soc_2".to_string()],
            issued_at: "2026-02-25T00:00:00Z".to_string(),
            expires_at: Some("2026-12-31T23:59:59Z".to_string()),
            byol_enabled: false,
        }
    }

    fn signed_json_from_payload(payload: &EntitlementsPayload, signing_key: &SigningKey) -> String {
        let payload_json = serde_json::to_string(payload).expect("serialize payload");
        let signature = signing_key.sign(payload_json.as_bytes());
        let signed = serde_json::json!({
            "schema_version": &payload.schema_version,
            "customer_id": &payload.customer_id,
            "order_id": &payload.order_id,
            "plan": &payload.plan,
            "entitled_libraries": &payload.entitled_libraries,
            "bonus_libraries": &payload.bonus_libraries,
            "issued_at": &payload.issued_at,
            "expires_at": &payload.expires_at,
            "byol_enabled": payload.byol_enabled,
            "signature": BASE64_STANDARD.encode(signature.to_bytes())
        });
        serde_json::to_string(&signed).expect("serialize signed payload")
    }

    #[test]
    fn payload_serialization_is_stable() {
        let payload = sample_payload();
        let serialized = serde_json::to_string(&payload).expect("serialize");
        assert_eq!(
            serialized,
            r#"{"schema_version":"civitas.entitlements.v1","customer_id":"cust_001","order_id":"ord_001","plan":"EU_DUO","entitled_libraries":["vendorsecurity/v1","iso27001-lite/v1"],"bonus_libraries":["soc_2"],"issued_at":"2026-02-25T00:00:00Z","expires_at":"2026-12-31T23:59:59Z","byol_enabled":false}"#
        );
    }

    #[test]
    fn valid_signature_passes() {
        let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
        let verifying_key_b64 = BASE64_STANDARD.encode(signing_key.verifying_key().as_bytes());
        let payload = sample_payload();
        let signed_json = signed_json_from_payload(&payload, &signing_key);

        let verified =
            parse_and_verify_entitlements(&signed_json, &verifying_key_b64).expect("must verify");
        assert_eq!(verified.payload.plan, "EU_DUO");
        assert!(verified
            .enabled_libraries
            .contains(&"vendorsecurity/v1".to_string()));
    }

    #[test]
    fn invalid_signature_fails() {
        let signing_key = SigningKey::from_bytes(&[9_u8; 32]);
        let verifying_key_b64 = BASE64_STANDARD.encode(signing_key.verifying_key().as_bytes());
        let payload = sample_payload();
        let mut signed: serde_json::Value =
            serde_json::from_str(&signed_json_from_payload(&payload, &signing_key))
                .expect("signed json");
        signed["plan"] = serde_json::Value::String("IR_DUO".to_string());
        let tampered = serde_json::to_string(&signed).expect("tampered json");

        let error =
            parse_and_verify_entitlements(&tampered, &verifying_key_b64).expect_err("must fail");
        assert!(
            error.contains("signature verification failed"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn expired_license_is_read_only() {
        let signing_key = SigningKey::from_bytes(&[11_u8; 32]);
        let verifying_key_b64 = BASE64_STANDARD.encode(signing_key.verifying_key().as_bytes());
        let mut payload = sample_payload();
        payload.expires_at = Some("2026-01-01T00:00:00Z".to_string());
        let signed_json = signed_json_from_payload(&payload, &signing_key);

        let temp_dir = tempfile::Builder::new()
            .prefix("epi-viewer-entitlements-")
            .tempdir()
            .expect("temp dir");
        let path = temp_dir.path().join("entitlements.json");
        fs::write(&path, signed_json).expect("write entitlements");

        let status = get_entitlements_status_from_path(
            &path,
            DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
                .expect("valid now")
                .with_timezone(&Utc),
            &verifying_key_b64,
        );
        assert_eq!(status.status, EntitlementsStatusKind::Expired);
        assert!(status.read_only);
    }
}
