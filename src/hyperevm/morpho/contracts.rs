use alloy::sol;

sol! {
    type Id is bytes32;

    #[derive(Debug)]
    struct Market {
        uint128 totalSupplyAssets;
        uint128 totalSupplyShares;
        uint128 totalBorrowAssets;
        uint128 totalBorrowShares;
        uint128 lastUpdate;
        uint128 fee;
    }

    #[derive(Debug)]
    struct MarketParams {
        address loanToken;
        address collateralToken;
        address oracle;
        address irm;
        uint256 lltv;
    }

    #[derive(Debug)]
    struct MarketConfig {
        uint184 cap;
        bool enabled;
        uint64 removableAt;
    }

    #[sol(rpc)]
    contract Morpho {
        // ========== events ============
        event CreateMarket(Id indexed id, MarketParams marketParams);

        // ========= functions =========
        function market(Id market) returns (Market);
        function idToMarketParams(Id market) returns (MarketParams);
        function convertToAssets(uint256 shares) external view returns (uint256 assets);
        function position(bytes32 id, address user)
            external
            view
            returns (uint256 supplyShares, uint128 borrowShares, uint128 collateral);
    }

    #[sol(rpc)]
    contract MetaMorpho {
        bytes32[] public supplyQueue;

        function MORPHO() external view returns (address);
        function fee() returns (uint96);
        function supplyQueueLength() external view returns (uint256);
        function config(Id market) returns (MarketConfig);
    }

    #[sol(rpc)]
    contract AdaptativeCurveIrm {
        function MORPHO() external view returns (address);
        function borrowRateView(MarketParams memory marketParams, Market memory market) external returns (uint256);
    }
}
