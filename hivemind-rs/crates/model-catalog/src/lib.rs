//! Signed model catalog: schema, validation, Ed25519 verification, and the
//! CPT price table with prepaid token quota.
//!
//! The catalog is the only source of model identity, license, digests, swarm
//! metainfo, and price. Its bytes are verified against a pinned public key
//! before they are parsed, and every field is validated before use.

mod catalog;
mod license;
pub mod multiformat;
mod price;

pub use catalog::{
    parse_public_key_hex, parse_secret_key_hex, sha256_hex, sign_catalog, Catalog, CatalogError,
    GgufPin, LicensePin, ModelEntry, PricePin, RuntimePin, SwarmPin, CATALOG_SCHEMA,
    MAX_CATALOG_BYTES, MAX_CONTEXT_LENGTH, MAX_MODELS, MAX_MODEL_FILE_BYTES, MAX_PIECE_LENGTH,
    MIN_PIECE_LENGTH,
};
pub use license::{is_allowed_license, ALLOWED_SPDX_LICENSES};
pub use multiformat::{
    AcceleratorKind, ArtifactFormat, HardwareArtifactPin, MultiFormatModelEntry,
};
pub use price::{Admission, BlockQuota, QuotaError, TOKENS_PER_BLOCK};
