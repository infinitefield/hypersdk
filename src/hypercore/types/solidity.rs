//! Solidity struct definitions for EIP-712 signing.
//!
//! These structs define the EIP-712 types used for signing various actions
//! on the Hyperliquid exchange. Each struct corresponds to a specific action type.

use alloy::sol;

sol! {
    #[derive(serde::Serialize)]
    struct Agent {
        string source;
        bytes32 connectionId;
    }

    struct UsdSend {
        string hyperliquidChain;
        string destination;
        string amount;
        uint64 time;
    }

    struct SpotSend {
        string hyperliquidChain;
        string destination;
        string token;
        string amount;
        uint64 time;
    }

    struct SendAsset {
        string hyperliquidChain;
        string destination;
        string sourceDex;
        string destinationDex;
        string token;
        string amount;
        string fromSubAccount;
        uint64 nonce;
    }

    /// Core to EVM transfer carrying a data payload for `ICoreReceiveWithData` contracts.
    ///
    /// EIP-712 type: `HyperliquidTransaction:SendToEvmWithData`.
    struct SendToEvmWithData {
        string hyperliquidChain;
        string token;
        string amount;
        string sourceDex;
        string destinationRecipient;
        string addressEncoding;
        uint32 destinationChainId;
        uint64 gasLimit;
        bytes data;
        uint64 nonce;
    }

    struct ApproveAgent {
        string hyperliquidChain;
        address agentAddress;
        string agentName;
        uint64 nonce;
    }

    struct ApproveBuilderFee {
        string hyperliquidChain;
        string maxFeeRate;
        address builder;
        uint64 nonce;
    }

    struct ConvertToMultiSigUser {
        string hyperliquidChain;
        string signers;
        uint64 nonce;
    }

    struct SendMultiSig {
        string hyperliquidChain;
        bytes32 multiSigActionHash;
        uint64 nonce;
    }

    /// User-signed DEX abstraction action.
    ///
    /// Enables or disables DEX abstraction for a given user address.
    /// EIP-712 type: `HyperliquidTransaction:UserDexAbstraction`.
    struct UserDexAbstraction {
        string hyperliquidChain;
        address user;
        bool enabled;
        uint64 nonce;
    }

    /// User-signed set-abstraction action.
    ///
    /// Sets the account abstraction mode for a given user address.
    /// EIP-712 type: `HyperliquidTransaction:UserSetAbstraction`.
    struct UserSetAbstraction {
        string hyperliquidChain;
        address user;
        string abstraction;
        uint64 nonce;
    }

    /// User-signed portfolio margin toggle.
    ///
    /// EIP-712 type: `HyperliquidTransaction:UserPortfolioMargin`.
    struct UserPortfolioMargin {
        string hyperliquidChain;
        address user;
        bool enabled;
        uint64 nonce;
    }

    /// User-signed link of a staking account to a trading account.
    ///
    /// EIP-712 type: `HyperliquidTransaction:LinkStakingUser`.
    struct LinkStakingUser {
        string hyperliquidChain;
        address user;
        bool isFinalize;
        uint64 nonce;
    }

    /// User-signed removal of a trading account's staking link.
    ///
    /// EIP-712 type: `HyperliquidTransaction:StakingLinkDisableTradingUser`.
    struct StakingLinkDisableTradingUser {
        string hyperliquidChain;
        address tradingUser;
        uint64 nonce;
    }

    // The EIP-712 type is `HyperliquidTransaction:Withdraw`, not `Withdraw3`
    // (the action's `type` tag).
    struct Withdraw {
        string hyperliquidChain;
        string destination;
        string amount;
        uint64 time;
    }

    struct UsdClassTransfer {
        string hyperliquidChain;
        string amount;
        bool toPerp;
        uint64 nonce;
    }

    struct TokenDelegate {
        string hyperliquidChain;
        address validator;
        uint64 wei;
        bool isUndelegate;
        uint64 nonce;
    }

    struct CDeposit {
        string hyperliquidChain;
        uint64 wei;
        uint64 nonce;
    }

    struct CWithdraw {
        string hyperliquidChain;
        uint64 wei;
        uint64 nonce;
    }
}

/// Multisig-specific EIP-712 struct definitions.
///
/// These structs include additional fields for multisig operations,
/// including the multisig user address and outer signer address.
pub mod multisig {
    use alloy::sol;

    sol! {
        struct UsdSend {
            string hyperliquidChain;
            address payloadMultiSigUser;
            address outerSigner;
            string destination;
            string amount;
            uint64 time;
        }

        struct SpotSend {
            string hyperliquidChain;
            address payloadMultiSigUser;
            address outerSigner;
            string destination;
            string token;
            string amount;
            uint64 time;
        }

        struct SendAsset {
            string hyperliquidChain;
            address payloadMultiSigUser;
            address outerSigner;
            string destination;
            string sourceDex;
            string destinationDex;
            string token;
            string amount;
            string fromSubAccount;
            uint64 nonce;
        }

        struct ConvertToMultiSigUser {
            string hyperliquidChain;
            address payloadMultiSigUser;
            address outerSigner;
            string signers;
            uint64 nonce;
        }
    }
}
