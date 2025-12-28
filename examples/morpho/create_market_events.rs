use alloy::{
    primitives::FixedBytes, providers::Provider, rpc::types::Filter, sol, sol_types::SolEvent,
};
use clap::Parser;
use hypersdk::{
    Address, U256,
    hyperevm::{self, DynProvider, IERC20},
};
use tokio::sync::mpsc::unbounded_channel;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// The address of the morpho vault.
    #[arg(
        short,
        long,
        default_value = "0x68e37de8d93d3496ae143f2e900490f6280c57cd"
    )]
    contract_address: Address,
    /// RPC url
    #[arg(short, long, default_value = "http://127.0.0.1:8545")]
    rpc_url: String,
}

sol! {
    #[sol(rpc)]
    contract Morpho {
        type Id is bytes32;

        struct Market {
            uint128 totalSupplyAssets;
            uint128 totalSupplyShares;
            uint128 totalBorrowAssets;
            uint128 totalBorrowShares;
            uint128 lastUpdate;
            uint128 fee;
        }

        struct MarketParams {
            address loanToken;
            address collateralToken;
            address oracle;
            address irm;
            uint256 lltv;
        }

        event CreateMarket(Id indexed id, MarketParams marketParams);

        function market(bytes32 market) returns (Market);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = simple_logger::init_with_level(log::Level::Debug);
    let args = Cli::parse();

    println!("Connecting to RPC endpoint: {}", args.rpc_url);

    let provider = DynProvider::new(hyperevm::mainnet_with_url(&args.rpc_url).await?);
    let current_block = provider.get_block_number().await?;

    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    struct MarketParams {
        id: FixedBytes<32>,
        collateral_token: String,
        loan_token: String,
        irm: Address,
        oracle: Address,
        lltv: U256,
    }

    let (tx, mut rx) = unbounded_channel();
    for from_block in (0..current_block).step_by(100_000) {
        let provider = provider.clone();
        let tx = tx.clone();

        let filter = Filter::new()
            .address(args.contract_address)
            .event_signature(Morpho::CreateMarket::SIGNATURE_HASH)
            .from_block(from_block)
            .to_block(from_block + 100_000);

        tokio::spawn(async move {
            let logs = provider.get_logs(&filter).await?;
            for log in logs {
                let Some(topic0) = log.topic0() else {
                    continue;
                };

                if topic0 == &Morpho::CreateMarket::SIGNATURE_HASH {
                    if let Ok(market) = Morpho::CreateMarket::decode_log_data(&log.inner) {
                        let collateral =
                            IERC20::new(market.marketParams.collateralToken, provider.clone());
                        let loan = IERC20::new(market.marketParams.loanToken, provider.clone());
                        let (collateral, loan) = provider
                            .multicall()
                            .add(collateral.symbol())
                            .add(loan.symbol())
                            .aggregate()
                            .await?;
                        let _ = tx.send(MarketParams {
                            id: market.id,
                            collateral_token: collateral,
                            loan_token: loan,
                            irm: market.marketParams.irm,
                            oracle: market.marketParams.oracle,
                            lltv: market.marketParams.lltv,
                        });
                    }
                }
            }

            Ok::<_, anyhow::Error>(())
        });
    }

    drop(tx);

    let mut market_params = vec![];
    while let Some(create_market) = rx.recv().await {
        market_params.push(create_market);
    }

    let mut markets = vec![];
    let morpho = Morpho::new(args.contract_address, provider);
    for params in &market_params {
        let data = morpho.market(params.id).call().await?;
        markets.push(data);
    }

    let mut markets = market_params.into_iter().zip(markets).collect::<Vec<_>>();
    markets.sort_by(|(_, a), (_, b)| a.totalBorrowAssets.cmp(&b.totalBorrowAssets));

    for (params, market) in markets {
        println!("------------");
        println!("market: {}", params.id);
        println!("collateral: {}", params.collateral_token);
        println!("loan token: {}", params.loan_token);
        println!("oracle: {}", params.oracle);
        println!("irm: {}", params.irm);
        println!("LLTV: {}", params.lltv);
        println!("borrowed: {}", market.totalBorrowAssets);
        println!("supplied: {}", market.totalSupplyAssets);
    }

    Ok(())
}
