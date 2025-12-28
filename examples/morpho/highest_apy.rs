use std::cmp::Reverse;

use alloy::{providers::Provider, rpc::types::Filter, sol, sol_types::SolEvent};
use clap::Parser;
use hypersdk::{
    Address,
    hyperevm::{self, DynProvider},
};
use tokio::sync::mpsc::unbounded_channel;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// IRM
    #[arg(
        short,
        long,
        default_value = "0xD4a426F010986dCad727e8dd6eed44cA4A9b7483"
    )]
    contract_address: Address,
    /// RPC url
    #[arg(short, long, default_value = "http://127.0.0.1:8545")]
    rpc_url: String,
}

sol! {
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

    #[sol(rpc)]
    contract Morpho {
        event CreateMarket(Id indexed id, MarketParams marketParams);

        function market(bytes32 market) returns (Market);
    }

    #[sol(rpc)]
    contract AdaptativeCurveIrm {
        function MORPHO() external view returns (address);
        function borrowRateView(MarketParams memory marketParams, Market memory market) external returns (uint256);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    println!("Connecting to RPC endpoint: {}", args.rpc_url);

    let provider = DynProvider::new(hyperevm::mainnet_with_url(&args.rpc_url).await?);
    let current_block = provider.get_block_number().await?;

    let irm = AdaptativeCurveIrm::new(args.contract_address, provider.clone());
    let morpho_address = irm.MORPHO().call().await?;
    let morpho = Morpho::new(morpho_address, provider.clone());

    let (tx, mut rx) = unbounded_channel();
    for from_block in (0..current_block).step_by(100_000) {
        let provider = provider.clone();
        let morpho = morpho.clone();
        let irm = irm.clone();
        let tx = tx.clone();

        let filter = Filter::new()
            .address(morpho_address)
            .event_signature(Morpho::CreateMarket::SIGNATURE_HASH)
            .from_block(from_block)
            .to_block(from_block + 100_000);

        // gather all the created markets, then load the current rate
        tokio::spawn(async move {
            let logs = provider.get_logs(&filter).await?;
            for log in logs {
                let Some(topic0) = log.topic0() else {
                    continue;
                };

                if topic0 == &Morpho::CreateMarket::SIGNATURE_HASH {
                    if let Ok(market) = Morpho::CreateMarket::decode_log_data(&log.inner) {
                        // let collateral =
                        //     IERC20::new(market.marketParams.collateralToken, provider.clone());
                        // let loan = IERC20::new(market.marketParams.loanToken, provider.clone());
                        // let (collateral, loan) = provider
                        //     .multicall()
                        //     .add(collateral.symbol())
                        //     .add(loan.symbol())
                        //     .aggregate()
                        //     .await?;
                        let params = market.marketParams;
                        let market = morpho.market(market.id).call().await?;
                        if market.totalBorrowAssets == 0 || market.totalSupplyAssets == 0 {
                            return Ok(());
                        }

                        let rate = irm
                            .borrowRateView(params.clone(), market.clone())
                            .call()
                            .await?;
                        let utilization =
                            market.totalBorrowAssets as f64 / market.totalSupplyAssets as f64;
                        let fee = market.fee as f64 / 1e18;
                        let rate = rate.to::<u64>() as f64 / 1e18;
                        let borrow_apy = (rate * 31_536_000f64).exp() - 1.0;
                        let supply_apy = borrow_apy * utilization * (1.0 - fee);
                        let _ = tx.send((params, market, borrow_apy, supply_apy));
                    }
                }
            }

            Ok::<_, anyhow::Error>(())
        });
    }

    drop(tx);

    let mut markets = vec![];
    while let Some(data) = rx.recv().await {
        markets.push(data);
    }

    markets.sort_by_key(|(_, market, _, _)| Reverse(market.totalBorrowAssets));

    for (params, market, borrow_apy, supply_apy) in markets.iter().take(10) {
        println!("----------------");
        println!("collateral: {}", params.collateralToken);
        println!("loan token: {}", params.loanToken);
        println!("LLTV: {}", params.lltv);
        println!("borrowed: {}", market.totalBorrowAssets);
        println!("supplied: {}", market.totalSupplyAssets);
        println!("borrow apy: {}", borrow_apy * 100.0);
        println!("supply apy: {}", supply_apy * 100.0);
    }

    Ok(())
}
