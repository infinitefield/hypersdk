//! Fetch detailed information about a Hyperliquid vault.
//!
//! This example demonstrates how to query vault summaries and then fetch
//! detailed information about a specific vault including followers, APR,
//! and portfolio data.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example vault_details
//! cargo run --example vault_details -- 0xYourVaultAddress
//! ```
//!
//! # Output
//!
//! ```text
//! Available Vaults:
//! ================
//! HLP                     TVL: $125,432,100.00  Open: Yes
//! Hyperliquidity Provider TVL: $45,678,900.00   Open: Yes
//! ...
//!
//! Vault Details: HLP
//! ==================
//! Leader: 0x1234...abcd
//! APR: 12.45%
//! Commission: 10.00%
//! Followers: 1,234
//! ...
//! ```

use hypersdk::hypercore;
use hypersdk::Address;
use rust_decimal::Decimal;
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a mainnet client
    let client = hypercore::mainnet();

    // Check if a specific vault address was provided as argument
    let args: Vec<String> = env::args().collect();
    let specific_vault: Option<Address> = args.get(1).and_then(|s| s.parse().ok());

    if let Some(vault_address) = specific_vault {
        // Fetch details for the specific vault
        println!("Fetching details for vault: {:?}\n", vault_address);
        fetch_and_display_vault_details(&client, vault_address).await?;
        return Ok(());
    }

    // First, fetch all vault summaries
    println!("Fetching vault summaries...\n");
    let summaries = client.vault_summaries().await?;

    if summaries.is_empty() {
        println!("No vaults found via vaultSummaries endpoint.");
        println!("\nTip: You can query a specific vault directly by passing its address:");
        println!("  cargo run --example vault_details -- 0xYourVaultAddress");
        return Ok(());
    }

    println!("Available Vaults:");
    println!("{:=<70}", "");

    // Display top 10 vaults by TVL
    let mut sorted_vaults = summaries.clone();
    sorted_vaults.sort_by(|a, b| b.tvl.cmp(&a.tvl));

    for vault in sorted_vaults.iter().take(10) {
        let status = if vault.is_closed { "Closed" } else { "Open" };
        println!(
            "{:<30} TVL: ${:<15} Status: {}",
            truncate(&vault.name, 28),
            format_usd(vault.tvl),
            status
        );
    }

    println!("\nTotal vaults: {}", summaries.len());

    // Now fetch details for the first vault (if any)
    if let Some(vault_summary) = sorted_vaults.first() {
        println!("\n{:=<70}", "");
        println!("Vault Details: {}", vault_summary.name);
        println!("{:=<70}", "");
        fetch_and_display_vault_details(&client, vault_summary.vault_address).await?;
    }

    Ok(())
}

/// Fetch and display details for a specific vault.
async fn fetch_and_display_vault_details(
    client: &hypercore::HttpClient,
    vault_address: Address,
) -> anyhow::Result<()> {
    let details = client.vault_details(vault_address).await?;

    println!("Address:      {:?}", details.vault_address);
    println!("Leader:       {:?}", details.leader);
    println!("Name:         {}", details.name);
    println!("Description:  {}", truncate(&details.description, 50));
    println!("APR:          {:.2}%", details.apr_percent());
    println!("Commission:   {:.2}%", details.commission_percent());
    println!("Followers:    {}", details.follower_count());
    println!(
        "Open:         {}",
        if details.accepts_deposits() {
            "Yes"
        } else {
            "No"
        }
    );
    println!(
        "Max Withdraw: ${}",
        format_usd(details.max_withdrawable_decimal())
    );

    // Show top followers
    if !details.followers.is_empty() {
        println!("\nTop Followers:");
        println!("{:-<70}", "");

        for follower in details.followers.iter().take(5) {
            println!(
                "  {:?}  Equity: ${:<12}  PnL: ${:<12}  Days: {}",
                follower.user,
                format_usd(follower.equity_decimal()),
                format_usd_signed(follower.pnl_decimal()),
                follower.days_following
            );
        }

        if details.followers.len() > 5 {
            println!("  ... and {} more followers", details.followers.len() - 5);
        }
    }

    // Show total follower equity
    let total_equity = details.total_follower_equity();
    println!("\nTotal Follower Equity: ${}", format_usd(total_equity));

    Ok(())
}

/// Format a decimal as USD with commas.
fn format_usd(value: Decimal) -> String {
    let value_f64 = value.to_string().parse::<f64>().unwrap_or(0.0);
    format!("{:.2}", value_f64)
        .chars()
        .rev()
        .enumerate()
        .fold(String::new(), |mut acc, (i, c)| {
            if i > 0 && i % 3 == 0 && c != '-' && c != '.' {
                // Check if we're in the integer part
                let pos_from_decimal = acc.chars().filter(|&x| x != ',').count();
                if !acc.contains('.') || pos_from_decimal > acc.find('.').map(|p| acc.len() - p - 1).unwrap_or(0) + 2 {
                    if acc.chars().last() != Some('.') {
                        acc.push(',');
                    }
                }
            }
            acc.push(c);
            acc
        })
        .chars()
        .rev()
        .collect()
}

/// Format a decimal as signed USD.
fn format_usd_signed(value: Decimal) -> String {
    let formatted = format_usd(value.abs());
    if value.is_sign_negative() {
        format!("-{}", formatted)
    } else {
        formatted
    }
}

/// Truncate a string to a maximum length.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
