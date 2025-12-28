use hypersdk::hypercore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = hypercore::mainnet();

    let markets = client.spot().await?;
    for market in markets {
        println!(
            "{}\t{}/{}",
            market.name, market.tokens[0].name, market.tokens[1].name
        );
    }

    Ok(())
}
