//! Morpho contracts
//!
//! Types generated from ABI JSON files.
//!
//! [`Id`], [`Market`], [`MarketParams`] and [`MarketConfig`] types are created to avoid
//! the redundant definitions of such types.

use alloy::sol;

macro_rules! transmute_this {
    ($from:ty,$into:ty) => {
        impl From<$from> for $into {
            fn from(value: $from) -> $into {
                unsafe { std::mem::transmute(value) }
            }
        }

        impl From<$into> for $from {
            fn from(value: $into) -> $from {
                unsafe { std::mem::transmute(value) }
            }
        }
    };
}

sol! {
    type Id is bytes32;

    #[derive(Debug, Copy)]
    struct Market {
        uint128 totalSupplyAssets;
        uint128 totalSupplyShares;
        uint128 totalBorrowAssets;
        uint128 totalBorrowShares;
        uint128 lastUpdate;
        uint128 fee;
    }

    #[derive(Debug, Copy)]
    struct MarketParams {
        address loanToken;
        address collateralToken;
        address oracle;
        address irm;
        uint256 lltv;
    }
}

sol!(
    #[derive(Debug)]
    #[sol(rpc)]
    IMorpho,
    "abi/IMorpho.json"
);

transmute_this!(IMorpho::Market, Market);
transmute_this!(IMorpho::MarketParams, MarketParams);

sol!(
    #[derive(Debug)]
    #[sol(rpc)]
    IMetaMorpho,
    "abi/IMetaMorphoV1_1.json"
);

transmute_this!(IMetaMorpho::MarketParams, MarketParams);

sol!(
    #[derive(Debug)]
    #[sol(rpc)]
    IIrm,
    "abi/IIrm.json"
);

transmute_this!(IIrm::Market, Market);
transmute_this!(IIrm::MarketParams, MarketParams);

sol!(
    #[derive(Debug)]
    #[sol(rpc)]
    MorphoEvents,
    "abi/MorphoEventsLib.json"
);

transmute_this!(MorphoEvents::MarketParams, MarketParams);

sol!(
    #[derive(Debug)]
    #[sol(rpc)]
    MorphoIOracle,
    "abi/MorphoIOracle.json"
);
