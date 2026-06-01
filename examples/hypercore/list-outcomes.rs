//! List all HIP-4 outcome markets on Hyperliquid.
//!
//! Fetches outcome metadata and displays each market with its ID, name, description,
//! and the two tradable sides (YES / NO).

use hypersdk::hypercore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = hypercore::mainnet();

    // Fetch raw metadata
    let meta = client.outcome_meta().await?;

    if meta.outcomes.is_empty() {
        println!("No outcome markets found.");
        return Ok(());
    }

    println!("{:<6} {:<12} {:<8} Description", "ID", "Name", "Sides");
    println!("{}", "-".repeat(90));

    for outcome in &meta.outcomes {
        let sides: Vec<&str> = outcome.side_specs.iter().map(|s| s.name.as_str()).collect();
        println!(
            "{:<6} {:<12} {:<8} {}",
            outcome.outcome,
            outcome.name,
            sides.join(" / "),
            outcome.description
        );
    }

    // Show questions if any
    if !meta.questions.is_empty() {
        println!("\n{} Questions:", meta.questions.len());
        println!("{}", "-".repeat(90));
        for q in &meta.questions {
            println!(
                "  Question {} — outcomes: {:?}, fallback: {:?}",
                q.question, q.named_outcomes, q.fallback_outcome
            );
        }
    }

    Ok(())
}
