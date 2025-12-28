use hypersdk::hypercore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = hypercore::mainnet();

    let tokens = client.spot_tokens().await?;
    for token in tokens {
        println!(
            "{}\t{}\t{}\t{}\t{:?}\t{:?}",
            token.name,
            token.index,
            token.wei_decimals,
            token.evm_extra_decimals,
            token.cross_chain_address,
            token.evm_contract,
        );
    }

    Ok(())
}
