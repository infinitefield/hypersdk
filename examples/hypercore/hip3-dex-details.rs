//! Show who operates each HIP-3 DEX.
//!
//! Prints every DEX's deployer, oracle updater, and fee recipient, followed by each
//! `perpDeploy` action it has delegated to sub-deployers and the addresses allowed to send it.
//!
//! ```bash
//! cargo run --example hip3-dex-details
//! ```

use hypersdk::hypercore::{self, SubDeployerPermission};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = hypercore::mainnet();

    for dex in client.perp_dex_details().await? {
        println!("\n{} ({}) index {}", dex.full_name, dex.name, dex.index);
        println!("  deployer        {}", dex.deployer);
        match dex.oracle_updater {
            Some(updater) => println!("  oracle updater  {updater}"),
            None => println!("  oracle updater  none"),
        }
        match dex.fee_recipient {
            Some(recipient) => println!("  fee recipient   {recipient}"),
            None => println!("  fee recipient   none"),
        }
        println!(
            "  markets with OI caps: {}",
            dex.asset_to_streaming_oi_cap.len()
        );

        for (permission, users) in &dex.sub_deployers {
            let permission = match permission {
                SubDeployerPermission::PerpDeploy(action) => action.clone(),
                SubDeployerPermission::Hip3Star { action } => format!("hip3Star:{action}"),
                SubDeployerPermission::Other(raw) => raw.to_string(),
            };
            let users: Vec<String> = users.iter().map(ToString::to_string).collect();
            println!("  {permission:<26} {}", users.join(", "));
        }
    }

    Ok(())
}
