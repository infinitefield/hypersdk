//! Account management commands for keystore operations.
//!
//! This module provides commands for managing Ethereum keystores:
//! - Creating new accounts with random private keys
//! - Importing existing private keys into keystores
//! - Listing available keystores

use std::fs;

use clap::{Args, Subcommand};
use hypersdk::hypercore::PrivateKeySigner;

use crate::utils::keystore_dir;

/// Account management commands.
#[derive(Subcommand)]
pub enum AccountCmd {
    /// Create a new keystore (generate new key or import existing)
    Create(CreateCmd),
    /// List available keystores
    List(ListCmd),
}

impl AccountCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Create(cmd) => cmd.run().await,
            Self::List(cmd) => cmd.run().await,
        }
    }
}

/// Create a new keystore.
///
/// By default, generates a new random private key. Use `--private-key` to import
/// an existing key instead.
///
/// # Examples
///
/// Create a new account with a random key:
/// ```bash
/// hypecli account create --name my-wallet
/// ```
///
/// Import an existing private key:
/// ```bash
/// hypecli account create --name imported-wallet --private-key 0x...
/// ```
#[derive(Args)]
pub struct CreateCmd {
    /// Name for the keystore file
    #[arg(long)]
    pub name: String,

    /// Password for encrypting the keystore
    /// If not provided, will be prompted interactively
    #[arg(long)]
    pub password: Option<String>,
}

impl CreateCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let dir = keystore_dir()?;

        // Create the keystore directory if it doesn't exist
        fs::create_dir_all(&dir)?;

        // Check if keystore already exists
        let keystore_path = dir.join(&self.name);
        if keystore_path.exists() {
            anyhow::bail!("Keystore '{}' already exists", self.name);
        }

        // Get password
        let password = match self.password {
            Some(p) => p,
            None => {
                let pass = rpassword::prompt_password("Enter password for keystore: ")?;
                let confirm = rpassword::prompt_password("Confirm password: ")?;
                if pass != confirm {
                    anyhow::bail!("Passwords do not match");
                }
                pass
            }
        };

        // Encrypt and save using eth_keystore
        let (signer, _) = PrivateKeySigner::new_keystore(
            &dir,
            &mut rand_08::thread_rng(),
            password.as_str(),
            Some(self.name.as_str()),
        )?;

        println!("Keystore created: {}", self.name);
        println!("Address: {}", signer.address());
        println!("Path: {}", keystore_path.display());

        Ok(())
    }
}

/// List available keystores.
///
/// Shows all keystores in ~/.foundry/keystores/
#[derive(Args)]
pub struct ListCmd {}

impl ListCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let dir = keystore_dir()?;

        if !dir.exists() {
            println!("No keystores found (directory does not exist)");
            println!("Path: {}", dir.display());
            return Ok(());
        }

        let entries: Vec<_> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .collect();

        if entries.is_empty() {
            println!("No keystores found");
            println!("Path: {}", dir.display());
            return Ok(());
        }

        println!("Available keystores ({}):", dir.display());

        for entry in entries {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Try to read and parse the keystore to get the address
            let path = entry.path();
            match fs::read_to_string(&path) {
                Ok(content) => {
                    // Parse JSON to extract address
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(addr) = json.get("address").and_then(|a| a.as_str()) {
                            println!("  {} (0x{})", name_str, addr);
                        } else {
                            println!("  {}", name_str);
                        }
                    } else {
                        println!("  {}", name_str);
                    }
                }
                Err(_) => {
                    println!("  {}", name_str);
                }
            }
        }

        Ok(())
    }
}
