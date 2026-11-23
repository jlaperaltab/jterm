//! Núcleo de jTerm (sin interfaz). Port a Rust de los módulos de lógica de `Sources/jTerm`.
//!
//! Los archivos JSON usan el mismo formato que la versión Swift, así que los datos existentes se leen tal cual.

pub mod aws;
pub mod awsstore;
pub mod command_guard;
pub mod crypto;
pub mod logs;
pub mod models;
pub mod s3;
pub mod sigv4;
pub mod xml;
pub mod settings;
pub mod sftp;
pub mod ssh_jump;
pub mod store;
pub mod tunnels;
pub mod sync;
pub mod syncadapter;
pub mod themes;
