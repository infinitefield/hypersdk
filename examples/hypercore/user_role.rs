//! Query the role of an address on Hyperliquid.
//!
//! This example demonstrates how to determine if an address is a regular user,
//! vault, agent, subaccount, or not found in the system.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example user_role -- <ADDRESS>
//! ```

use clap::Parser;
use hypersdk::{Address, hypercore, hypercore::types::UserRoleType};

#[derive(Parser)]
struct Args {
    /// Address to query the role for
    address: Address,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let client = hypercore::mainnet();

    let response = client.user_role(args.address).await?;

    println!("Address: {:?}", args.address);
    println!();

    match response.role {
        UserRoleType::User => {
            println!("Role: User");
            println!("This is a regular trading account.");
        }
        UserRoleType::Vault => {
            println!("Role: Vault");
            println!("This is a vault account that can accept deposits from followers.");
        }
        UserRoleType::Agent => {
            println!("Role: Agent");
            println!("This is an agent wallet authorized to trade on behalf of another account.");
        }
        UserRoleType::SubAccount => {
            println!("Role: Subaccount");
            println!("This is a subaccount controlled by a master account.");
        }
        UserRoleType::Missing => {
            println!("Role: Missing");
            println!("This address was not found in the Hyperliquid system.");
        }
    }

    Ok(())
}
