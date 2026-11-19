//! Private swarm distribution for model files.
//!
//! This crate implements the parts of BitTorrent v1 that a private swarm needs:
//! strict bencode, single-file metainfo with the private flag, the peer wire
//! messages for one file, a seed that serves content, a leech that verifies
//! every piece, and an in-memory tracker registry. It does not implement DHT,
//! peer exchange, local service discovery, or protocol extensions.

pub mod bencode;
pub mod leech;
pub mod metainfo;
pub mod seed;
pub mod tracker;
pub mod wire;

pub use leech::{download, DownloadError, DownloadOptions, DownloadReport};
pub use metainfo::{Metainfo, MetainfoError};
pub use seed::{SeedServer, DEFAULT_MAX_CONNECTIONS};
pub use tracker::{Registry, RegistryError, DEFAULT_TTL};
pub use wire::{WireError, BLOCK_LEN};
