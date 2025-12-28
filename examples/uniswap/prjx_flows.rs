use std::collections::HashMap;

use alloy::{providers::Provider, rpc::types::Filter, sol_types::SolEvent};
use clap::Parser;
use hypersdk::hyperevm::{
    self, Address, IERC20,
    uniswap::{contracts::NFTPositionManager, prjx},
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Uniswap factory contract address.
    #[arg(
        short,
        long,
        default_value = "0xeaD19AE861c29bBb2101E834922B2FEee69B9091"
    )]
    contract_address: Address,
    /// Target address
    #[arg(short, long)]
    from: Address,
    /// RPC url
    #[arg(short, long, default_value = "http://127.0.0.1:8545")]
    rpc_url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = simple_logger::init_with_level(log::Level::Debug);
    let args = Cli::parse();

    let provider = hyperevm::mainnet_with_url(&args.rpc_url).await?;
    let current_block = provider.get_block_number().await?;

    let mut to_block = current_block;
    let mut tokens: HashMap<Address, (String, u8)> = HashMap::default();

    let prjx = prjx::mainnet_with_url(&args.rpc_url).await?;
    let positions = prjx.positions(args.from).await?;

    for pos in &positions {
        let provider = prjx.provider();
        let token0_client = IERC20::new(pos.token0, provider.clone());
        let token1_client = IERC20::new(pos.token1, provider.clone());

        let (symbol0, decimals0, symbol1, decimals1) = provider
            .multicall()
            .add(token0_client.symbol())
            .add(token0_client.decimals())
            .add(token1_client.symbol())
            .add(token1_client.decimals())
            .aggregate()
            .await?;

        tokens.insert(pos.token0, (symbol0, decimals0));
        tokens.insert(pos.token1, (symbol1, decimals1));
    }

    while to_block >= 4_000_000 {
        let from_block = to_block - 100_000;

        for pos in &positions {
            let (token0, _) = &tokens[&pos.token0];
            let (token1, _) = &tokens[&pos.token1];

            let filter = Filter::new()
                .address(args.contract_address)
                .event_signature(vec![
                    NFTPositionManager::IncreaseLiquidity::SIGNATURE_HASH,
                    NFTPositionManager::DecreaseLiquidity::SIGNATURE_HASH,
                ])
                .topic1(pos.token_id)
                .from_block(from_block)
                .to_block(to_block);

            let logs = provider.get_logs(&filter).await?;
            for log in logs {
                match *log.topic0().unwrap() {
                    NFTPositionManager::IncreaseLiquidity::SIGNATURE_HASH => {
                        let log = NFTPositionManager::IncreaseLiquidity::decode_log(&log.inner)?;
                        println!(
                            "Incresed liquidity on {} {token0}/{token1}: {} - {}",
                            log.tokenId, log.amount0, log.amount1,
                        );
                    }
                    NFTPositionManager::DecreaseLiquidity::SIGNATURE_HASH => {
                        let log = NFTPositionManager::DecreaseLiquidity::decode_log(&log.inner)?;
                        println!(
                            "Decreased liquidity on {} {token0}/{token1}: {} - {}",
                            log.tokenId, log.amount0, log.amount1,
                        );
                    }
                    _ => {}
                }
            }
        }

        to_block = from_block;
    }

    Ok(())
}
