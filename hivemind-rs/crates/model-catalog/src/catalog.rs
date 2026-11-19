//! Catalog schema, validation, and signature handling.

use std::collections::HashSet;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::license::is_allowed_license;

/// Schema identifier every catalog must carry.
pub const CATALOG_SCHEMA: &str = "hivemind-model-catalog/v1";
/// Largest catalog accepted. The size is checked before the signature is verified.
pub const MAX_CATALOG_BYTES: usize = 1 << 20;
/// Largest number of models in one catalog.
pub const MAX_MODELS: usize = 64;
/// Largest GGUF file a catalog entry may describe.
pub const MAX_MODEL_FILE_BYTES: u64 = 16 << 30;
/// Largest context window a catalog entry may declare.
pub const MAX_CONTEXT_LENGTH: u32 = 131_072;
/// Smallest BitTorrent piece length. Piece lengths must be a multiple of it.
pub const MIN_PIECE_LENGTH: u32 = 16 << 10;
/// Largest BitTorrent piece length.
pub const MAX_PIECE_LENGTH: u32 = 16 << 20;
/// Largest price per million tokens, in whole CPT.
const MAX_PRICE_CPT_PER_MILLION: i64 = 1_000_000_000_000;

/// Reasons a catalog or key is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CatalogError {
    #[error("catalog is {0} bytes, which is over the size limit")]
    TooLarge(usize),
    #[error("signature is missing, malformed, or does not verify under the pinned key")]
    BadSignature,
    #[error("catalog does not parse against the schema: {0}")]
    Parse(String),
    #[error("invalid field `{field}`: {reason}")]
    Invalid { field: &'static str, reason: String },
    #[error("model `{0}` is not in the catalog")]
    UnknownModel(String),
    #[error("key must be 32 bytes encoded as 64 hex characters")]
    MalformedKey,
}

/// A signed list of models. The signature covers the exact catalog bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: String,
    pub generation: u64,
    pub issued_at: String,
    pub runtime: RuntimePin,
    pub models: Vec<ModelEntry>,
}

/// The llama.cpp package the catalog admits. `manifest_sha256` pins the file
/// listing that the runtime verifies file by file before it starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimePin {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub package_sha256: String,
    pub manifest_sha256: String,
}

/// One model the network may serve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEntry {
    pub model_id: String,
    pub display_name: String,
    pub quantization: String,
    pub context_length: u32,
    /// Memory the model needs on the host, in bytes. Admission uses this value.
    pub ram_bytes: u64,
    pub gguf: GgufPin,
    pub swarm: SwarmPin,
    pub license: LicensePin,
    pub prices: PricePin,
}

/// Identity of the GGUF file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GgufPin {
    pub file_name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// Identity of the private swarm metainfo for the GGUF file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmPin {
    /// SHA-256 of the metainfo file bytes.
    pub metainfo_sha256: String,
    /// BitTorrent v1 infohash, 40 hex characters.
    pub infohash: String,
    pub piece_length: u32,
}

/// The license that admits the model, with the text that was checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicensePin {
    pub spdx: String,
    pub text_sha256: String,
    pub text_source: String,
    pub model_card: String,
}

/// Prices in whole CPT per million tokens, one rate for prompt tokens and one for
/// completion tokens. Quota is bought in blocks of one million tokens of each kind
/// at these rates. See `price.rs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PricePin {
    pub input_cpt_per_million_tokens: i64,
    pub output_cpt_per_million_tokens: i64,
}

impl Catalog {
    /// Verifies the signature over the exact bytes, then parses and validates the catalog.
    /// Nothing in the bytes is trusted before the signature checks out.
    pub fn verify_and_parse(
        bytes: &[u8],
        signature_hex: &str,
        key: &VerifyingKey,
    ) -> Result<Self, CatalogError> {
        if bytes.len() > MAX_CATALOG_BYTES {
            return Err(CatalogError::TooLarge(bytes.len()));
        }
        let signature = parse_signature(signature_hex)?;
        key.verify(bytes, &signature)
            .map_err(|_| CatalogError::BadSignature)?;
        let catalog: Catalog =
            serde_json::from_slice(bytes).map_err(|err| CatalogError::Parse(err.to_string()))?;
        catalog.validate()?;
        Ok(catalog)
    }

    /// Checks every field. The bytes must already be verified when this is called from outside.
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.schema != CATALOG_SCHEMA {
            return Err(invalid("schema", format!("expected `{CATALOG_SCHEMA}`")));
        }
        if self.generation == 0 {
            return Err(invalid("generation", "must be at least 1"));
        }
        check_text("issued_at", &self.issued_at, 64)?;
        self.runtime.validate()?;
        if self.models.is_empty() || self.models.len() > MAX_MODELS {
            return Err(invalid(
                "models",
                format!("must contain 1 to {MAX_MODELS} entries"),
            ));
        }
        let mut seen = HashSet::new();
        for model in &self.models {
            model.validate()?;
            if !seen.insert(model.model_id.as_str()) {
                return Err(invalid(
                    "models[].model_id",
                    format!("duplicate id `{}`", model.model_id),
                ));
            }
        }
        Ok(())
    }

    /// Returns the entry for `model_id`.
    pub fn model(&self, model_id: &str) -> Result<&ModelEntry, CatalogError> {
        self.models
            .iter()
            .find(|model| model.model_id == model_id)
            .ok_or_else(|| CatalogError::UnknownModel(model_id.to_owned()))
    }
}

impl RuntimePin {
    fn validate(&self) -> Result<(), CatalogError> {
        check_text("runtime.name", &self.name, 64)?;
        check_text("runtime.version", &self.version, 64)?;
        check_text("runtime.platform", &self.platform, 64)?;
        check_hex("runtime.package_sha256", &self.package_sha256, 32)?;
        check_hex("runtime.manifest_sha256", &self.manifest_sha256, 32)
    }
}

impl ModelEntry {
    fn validate(&self) -> Result<(), CatalogError> {
        check_model_id(&self.model_id)?;
        check_text("models[].display_name", &self.display_name, 128)?;
        check_text("models[].quantization", &self.quantization, 32)?;
        if self.context_length == 0 || self.context_length > MAX_CONTEXT_LENGTH {
            return Err(invalid(
                "models[].context_length",
                format!("must be 1 to {MAX_CONTEXT_LENGTH}"),
            ));
        }
        if self.gguf.size_bytes == 0 || self.gguf.size_bytes > MAX_MODEL_FILE_BYTES {
            return Err(invalid(
                "models[].gguf.size_bytes",
                format!("must be 1 to {MAX_MODEL_FILE_BYTES}"),
            ));
        }
        if self.ram_bytes < self.gguf.size_bytes {
            return Err(invalid(
                "models[].ram_bytes",
                "must be at least the GGUF size",
            ));
        }
        check_file_name("models[].gguf.file_name", &self.gguf.file_name)?;
        if !self.gguf.file_name.ends_with(".gguf") {
            return Err(invalid("models[].gguf.file_name", "must end with .gguf"));
        }
        check_hex("models[].gguf.sha256", &self.gguf.sha256, 32)?;
        check_hex(
            "models[].swarm.metainfo_sha256",
            &self.swarm.metainfo_sha256,
            32,
        )?;
        check_hex("models[].swarm.infohash", &self.swarm.infohash, 20)?;
        let piece = self.swarm.piece_length;
        if !(MIN_PIECE_LENGTH..=MAX_PIECE_LENGTH).contains(&piece)
            || !piece.is_multiple_of(MIN_PIECE_LENGTH)
        {
            return Err(invalid(
                "models[].swarm.piece_length",
                "must be a multiple of 16 KiB between 16 KiB and 16 MiB",
            ));
        }
        if !is_allowed_license(&self.license.spdx) {
            return Err(invalid(
                "models[].license.spdx",
                format!(
                    "`{}` is not on the admitted license list",
                    self.license.spdx
                ),
            ));
        }
        check_hex(
            "models[].license.text_sha256",
            &self.license.text_sha256,
            32,
        )?;
        check_https("models[].license.text_source", &self.license.text_source)?;
        check_https("models[].license.model_card", &self.license.model_card)?;
        check_price(
            "models[].prices.input_cpt_per_million_tokens",
            self.prices.input_cpt_per_million_tokens,
        )?;
        check_price(
            "models[].prices.output_cpt_per_million_tokens",
            self.prices.output_cpt_per_million_tokens,
        )
    }
}

/// Signs catalog bytes. Returns the signature as 128 hex characters.
pub fn sign_catalog(bytes: &[u8], key: &SigningKey) -> String {
    hex::encode(key.sign(bytes).to_bytes())
}

/// Parses a 64-hex-character Ed25519 public key.
pub fn parse_public_key_hex(value: &str) -> Result<VerifyingKey, CatalogError> {
    let bytes = decode_32(value)?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| CatalogError::MalformedKey)
}

/// Parses a 64-hex-character Ed25519 seed. Used by development tooling only.
pub fn parse_secret_key_hex(value: &str) -> Result<SigningKey, CatalogError> {
    Ok(SigningKey::from_bytes(&decode_32(value)?))
}

/// SHA-256 of `bytes`, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn parse_signature(value: &str) -> Result<Signature, CatalogError> {
    let bytes = hex::decode(value.trim()).map_err(|_| CatalogError::BadSignature)?;
    let array: [u8; 64] = bytes.try_into().map_err(|_| CatalogError::BadSignature)?;
    Ok(Signature::from_bytes(&array))
}

fn decode_32(value: &str) -> Result<[u8; 32], CatalogError> {
    let bytes = hex::decode(value.trim()).map_err(|_| CatalogError::MalformedKey)?;
    bytes.try_into().map_err(|_| CatalogError::MalformedKey)
}

fn invalid(field: &'static str, reason: impl Into<String>) -> CatalogError {
    CatalogError::Invalid {
        field,
        reason: reason.into(),
    }
}

fn check_text(field: &'static str, value: &str, max_len: usize) -> Result<(), CatalogError> {
    if value.is_empty() || value.len() > max_len || value.chars().any(char::is_control) {
        return Err(invalid(
            field,
            format!("must be 1 to {max_len} characters with no control characters"),
        ));
    }
    Ok(())
}

fn check_hex(field: &'static str, value: &str, byte_len: usize) -> Result<(), CatalogError> {
    let ok = value.len() == byte_len * 2
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if ok {
        Ok(())
    } else {
        Err(invalid(
            field,
            format!("must be {} lowercase hex characters", byte_len * 2),
        ))
    }
}

fn check_model_id(value: &str) -> Result<(), CatalogError> {
    let bytes = value.as_bytes();
    let ok = !bytes.is_empty()
        && bytes.len() <= 64
        && matches!(bytes[0], b'a'..=b'z' | b'0'..=b'9')
        && bytes
            .iter()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(invalid(
            "models[].model_id",
            "use 1 to 64 characters from a-z, 0-9, '.', '_', '-', starting with a letter or digit",
        ))
    }
}

fn check_file_name(field: &'static str, value: &str) -> Result<(), CatalogError> {
    let forbidden = value.chars().any(|c| {
        c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    });
    if value.is_empty() || value.len() > 255 || value == "." || value == ".." || forbidden {
        return Err(invalid(field, "must be a plain file name"));
    }
    Ok(())
}

fn check_https(field: &'static str, value: &str) -> Result<(), CatalogError> {
    if value.starts_with("https://") && value.len() <= 512 && !value.chars().any(char::is_control) {
        Ok(())
    } else {
        Err(invalid(
            field,
            "must be an https URL of at most 512 characters",
        ))
    }
}

fn check_price(field: &'static str, value: i64) -> Result<(), CatalogError> {
    if (0..=MAX_PRICE_CPT_PER_MILLION).contains(&value) {
        Ok(())
    } else {
        Err(invalid(
            field,
            format!("must be 0 to {MAX_PRICE_CPT_PER_MILLION}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMOLLM2_SHA: &str = "48ab3034d0dd401fbc721eb1df3217902fee7dab9078992d66431f09b7750201";
    const LICENSE_SHA: &str = "c274f80372d90c012937370f0e1f15087d22e308ef98b27cea5dc0d2d088366c";
    const HEX64: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    fn fixture_json() -> String {
        format!(
            r#"{{
  "schema": "{CATALOG_SCHEMA}",
  "generation": 1,
  "issued_at": "2026-11-13T00:00:00Z",
  "runtime": {{
    "name": "llama.cpp",
    "version": "b12110",
    "platform": "win-x64-cpu",
    "package_sha256": "98901e327c252875accaf2d1bf364e10e868843b0b63468baad1aa945cdc5da2",
    "manifest_sha256": "{HEX64}"
  }},
  "models": [{{
    "model_id": "smollm2-360m-instruct-q8_0",
    "display_name": "SmolLM2 360M Instruct",
    "quantization": "Q8_0",
    "context_length": 8192,
    "ram_bytes": 1073741824,
    "gguf": {{
      "file_name": "smollm2-360m-instruct-q8_0.gguf",
      "size_bytes": 386404992,
      "sha256": "{SMOLLM2_SHA}"
    }},
    "swarm": {{
      "metainfo_sha256": "{HEX64}",
      "infohash": "2222222222222222222222222222222222222222",
      "piece_length": 1048576
    }},
    "license": {{
      "spdx": "Apache-2.0",
      "text_sha256": "{LICENSE_SHA}",
      "text_source": "https://spdx.org/licenses/Apache-2.0.txt",
      "model_card": "https://huggingface.co/HuggingFaceTB/SmolLM2-360M-Instruct-GGUF"
    }},
    "prices": {{
      "input_cpt_per_million_tokens": 50,
      "output_cpt_per_million_tokens": 200
    }}
  }}]
}}"#
        )
    }

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn signed(json: &str) -> (Vec<u8>, String) {
        let bytes = json.as_bytes().to_vec();
        let signature = sign_catalog(&bytes, &signing_key());
        (bytes, signature)
    }

    #[test]
    fn a_signed_catalog_verifies_and_exposes_the_model() {
        let (bytes, signature) = signed(&fixture_json());
        let key = signing_key().verifying_key();
        let catalog = Catalog::verify_and_parse(&bytes, &signature, &key).expect("valid catalog");
        let model = catalog
            .model("smollm2-360m-instruct-q8_0")
            .expect("model present");
        assert_eq!(model.gguf.sha256, SMOLLM2_SHA);
        assert!(catalog.model("missing").is_err());
    }

    #[test]
    fn one_changed_byte_fails_the_signature_before_parsing() {
        let (mut bytes, signature) = signed(&fixture_json());
        let index = bytes.iter().position(|b| *b == b'1').expect("a digit");
        bytes[index] = b'2';
        let key = signing_key().verifying_key();
        assert_eq!(
            Catalog::verify_and_parse(&bytes, &signature, &key),
            Err(CatalogError::BadSignature)
        );
    }

    #[test]
    fn a_catalog_signed_by_another_key_is_refused() {
        let (bytes, signature) = signed(&fixture_json());
        let other = SigningKey::from_bytes(&[9u8; 32]).verifying_key();
        assert_eq!(
            Catalog::verify_and_parse(&bytes, &signature, &other),
            Err(CatalogError::BadSignature)
        );
    }

    #[test]
    fn oversized_bytes_are_refused_before_the_signature_is_checked() {
        let bytes = vec![b' '; MAX_CATALOG_BYTES + 1];
        let key = signing_key().verifying_key();
        assert!(matches!(
            Catalog::verify_and_parse(&bytes, "00", &key),
            Err(CatalogError::TooLarge(_))
        ));
    }

    #[test]
    fn a_license_outside_the_admitted_list_is_refused() {
        let json = fixture_json().replace("\"Apache-2.0\"", "\"Llama-3.2-Community\"");
        let err = signed(&json);
        let key = signing_key().verifying_key();
        match Catalog::verify_and_parse(&err.0, &err.1, &key) {
            Err(CatalogError::Invalid { field, .. }) => {
                assert_eq!(field, "models[].license.spdx");
            }
            other => panic!("expected a license refusal, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_model_ids_are_refused() {
        let mut value: serde_json::Value =
            serde_json::from_str(&fixture_json()).expect("fixture parses");
        let first = value["models"][0].clone();
        value["models"]
            .as_array_mut()
            .expect("models is an array")
            .push(first);
        let (bytes, signature) = signed(&value.to_string());
        let key = signing_key().verifying_key();
        assert!(matches!(
            Catalog::verify_and_parse(&bytes, &signature, &key),
            Err(CatalogError::Invalid {
                field: "models[].model_id",
                ..
            })
        ));
    }

    #[test]
    fn uppercase_or_short_digests_are_refused() {
        let json = fixture_json().replace(SMOLLM2_SHA, &SMOLLM2_SHA.to_uppercase());
        let (bytes, signature) = signed(&json);
        let key = signing_key().verifying_key();
        assert!(matches!(
            Catalog::verify_and_parse(&bytes, &signature, &key),
            Err(CatalogError::Invalid {
                field: "models[].gguf.sha256",
                ..
            })
        ));
    }

    #[test]
    fn a_file_name_that_escapes_the_directory_is_refused() {
        let json = fixture_json().replace(
            "\"smollm2-360m-instruct-q8_0.gguf\"",
            "\"../smollm2-360m-instruct-q8_0.gguf\"",
        );
        let (bytes, signature) = signed(&json);
        let key = signing_key().verifying_key();
        assert!(matches!(
            Catalog::verify_and_parse(&bytes, &signature, &key),
            Err(CatalogError::Invalid {
                field: "models[].gguf.file_name",
                ..
            })
        ));
    }

    #[test]
    fn unknown_fields_are_refused() {
        let json = fixture_json().replace("\"generation\": 1,", "\"generation\": 1, \"extra\": 1,");
        let (bytes, signature) = signed(&json);
        let key = signing_key().verifying_key();
        assert!(matches!(
            Catalog::verify_and_parse(&bytes, &signature, &key),
            Err(CatalogError::Parse(_))
        ));
    }

    #[test]
    fn keys_round_trip_through_hex() {
        let secret = hex::encode([7u8; 32]);
        let signer = parse_secret_key_hex(&secret).expect("seed");
        let public = hex::encode(signer.verifying_key().to_bytes());
        assert_eq!(
            parse_public_key_hex(&public).expect("public"),
            signer.verifying_key()
        );
        assert_eq!(
            parse_public_key_hex("abcd"),
            Err(CatalogError::MalformedKey)
        );
    }
}
