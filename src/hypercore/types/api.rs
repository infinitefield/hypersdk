//! Raw API types for Hyperliquid exchange actions.
//!
//! This module contains the core action types and request/response structures
//! used for interacting with the Hyperliquid exchange API. These types handle
//! signing, serialization, and API communication.

use alloy::{
    dyn_abi::TypedData,
    primitives::{Address, B256, Bytes},
    signers::{Signer, SignerSync, k256::ecdsa::RecoveryId},
};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use super::{deploy, solidity};
use crate::hypercore::{
    ApiError, Chain,
    types::{
        BatchCancel, BatchCancelCloid, BatchModify, BatchOrder, CORE_MAINNET_EIP712_DOMAIN,
        OidOrCloid, OrderRequest, OrderResponseStatus, ScheduleCancel, Signature,
    },
    utils::{self, get_typed_data},
};

/// Request for an action.
///
/// Contains the action, a nonce, signature, optional vault address, and optional expiry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRequest {
    /// Action.
    pub action: Action,
    /// Nonce of the message.
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
    /// Trading on behalf of
    pub vault_address: Option<Address>,
    /// Timestamp in milliseconds
    pub expires_after: Option<u64>,
}

impl ActionRequest {
    /// Recover the user who signed an action.
    ///
    /// See more [`Action::recover`].
    pub fn recover(&self, chain: Chain) -> anyhow::Result<Address> {
        self.action.recover(
            &self.signature,
            self.nonce,
            self.vault_address,
            self.expires_after
                .and_then(|ts| DateTime::<Utc>::from_timestamp_millis(ts as i64)),
            chain,
        )
    }
}

/// An action that requires signing.
///
/// Represents a request to the exchange that must be signed by the user.
#[derive(Debug, Clone, Serialize, Deserialize, derive_more::From)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum Action {
    /// Order insertion.
    Order(BatchOrder),
    /// Replace a single resting order.
    #[from(skip)]
    Modify(ModifyAction),
    /// Order modification.
    BatchModify(BatchModify),
    /// Order cancellation by oid.
    Cancel(BatchCancel),
    /// Order cancellation by cloid.
    CancelByCloid(BatchCancelCloid),
    /// Schedule cancellation of all orders.
    ScheduleCancel(ScheduleCancel),
    /// Core USDC transfer.
    UsdSend(UsdSendAction),
    /// Send asset.
    SendAsset(SendAssetAction),
    /// Agent-signed send asset (destination must equal source).
    AgentSendAsset(AgentSendAssetAction),
    /// Spot send.
    SpotSend(SpotSendAction),
    /// EVM user modify.
    EvmUserModify {
        // rename_all = "camelCase" on the enum applies to variant names but not
        // struct variant fields when using #[serde(tag = "type")]. Explicit rename
        // fixes both the JSON body and the msgpack signing hash.
        #[serde(rename = "usingBigBlocks")]
        using_big_blocks: bool,
    },
    ApproveAgent(ApproveAgent),
    /// Approve maximum builder fee for a builder address.
    ApproveBuilderFee(ApproveBuilderFee),
    /// Convert to multi-signature user.
    ConvertToMultiSigUser(ConvertToMultiSigUser),
    /// Update isolated margin.
    UpdateIsolatedMargin(UpdateIsolatedMargin),
    /// Update leverage for a perpetual asset.
    UpdateLeverage(UpdateLeverage),
    /// Deposit or withdraw from a vault.
    VaultTransfer(VaultTransfer),
    /// Multi-sig action.
    MultiSig(MultiSigAction),
    /// Invalidate a request.
    Noop,
    /// Gossip priority bid (Dutch auction for read priority).
    GossipPriorityBid(GossipPriorityBid),
    /// Agent-signed: Enable DEX abstraction (deprecated, being discontinued).
    AgentEnableDexAbstraction,
    /// Agent-signed: Set abstraction mode.
    AgentSetAbstraction {
        /// The target abstraction mode. Serialized as a short code (`"i"`, `"u"`, `"p"`).
        #[serde(
            serialize_with = "serialize_abstraction_agent",
            deserialize_with = "deserialize_abstraction_agent"
        )]
        abstraction: AbstractionMode,
    },
    /// User-signed: Enable/disable DEX abstraction for a user.
    UserDexAbstraction(UserDexAbstractionAction),
    /// User-signed: Set abstraction mode for a user.
    UserSetAbstraction(UserSetAbstractionAction),
    /// Place a TWAP order.
    #[from(skip)]
    TwapOrder {
        twap: TwapOrderParams,
    },
    /// Cancel a TWAP order.
    #[from(skip)]
    TwapCancel {
        /// Asset index.
        a: usize,
        /// TWAP ID to cancel.
        t: u64,
    },
    /// Place a native trailing stop.
    #[from(skip)]
    TrailingStop(TrailingStop),
    /// Withdraw to Arbitrum L1.
    #[from(skip)]
    Withdraw3(Withdraw3Action),
    /// Transfer between spot and perp balances.
    #[from(skip)]
    UsdClassTransfer(UsdClassTransferAction),
    /// Stake native token (HYPE).
    #[from(skip)]
    #[serde(rename = "cDeposit")]
    CDeposit {
        /// Amount in wei (native token).
        wei: u64,
    },
    /// Unstake native token (HYPE). 7-day queue.
    #[from(skip)]
    #[serde(rename = "cWithdraw")]
    CWithdraw {
        /// Amount in wei (native token).
        wei: u64,
    },
    /// Delegate or undelegate staked tokens to a validator.
    #[from(skip)]
    TokenDelegate(TokenDelegateAction),
    /// Reserve rate-limit request capacity.
    #[from(skip)]
    ReserveRequestWeight {
        /// Number of requests to reserve (0.0005 USDC per request).
        weight: u32,
        /// Account the reserved capacity is credited to. `None` credits the signer.
        #[serde(
            skip_serializing_if = "Option::is_none",
            serialize_with = "crate::hypercore::utils::serialize_option_address_as_hex"
        )]
        destination: Option<Address>,
    },
    /// HIP-3 backstop liquidator deposit/withdraw.
    #[from(skip)]
    #[serde(rename = "hip3LiquidatorTransfer")]
    Hip3LiquidatorTransfer(Hip3LiquidatorTransferAction),
    /// HIP-4 outcome token split/merge/negate.
    #[from(skip)]
    UserOutcome(UserOutcomeAction),
    /// Core to EVM transfer carrying a data payload.
    #[from(skip)]
    SendToEvmWithData(SendToEvmWithDataAction),
    /// Add isolated margin up to a target leverage instead of a USDC amount.
    #[from(skip)]
    TopUpIsolatedOnlyMargin(TopUpIsolatedOnlyMargin),
    /// Claim accrued referral and builder rewards.
    #[from(skip)]
    ClaimRewards,
    /// Authorize an AQAv2 role for an aligned quote asset.
    #[from(skip)]
    #[serde(rename = "authorizeAqav2Role")]
    AuthorizeAqav2Role(AuthorizeAqav2Role),
    /// Validator vote on the risk-free rate for an aligned quote asset.
    #[from(skip)]
    ValidatorL1Stream(ValidatorL1Stream),
    /// HIP-1/HIP-2 spot token deployment, and HIP-4 outcome deployment.
    #[from(skip)]
    SpotDeploy(deploy::SpotDeployAction),
    /// HIP-3 perp DEX deployment and operation.
    #[from(skip)]
    PerpDeploy(deploy::PerpDeployAction),
    /// HIP-4 outcome market deployment and settlement, for one venue.
    #[from(skip)]
    OutcomeDeploy(deploy::OutcomeDeploy),
    /// HIP-4 outcome deployer activation.
    #[from(skip)]
    ActivateOutcomeDeployer(deploy::ActivateOutcomeDeployer),

    // ----------------------------------------------------------------
    // Undocumented actions.
    //
    // The gitbook does not cover any of these, but the exchange accepts all of them on
    // mainnet. Shapes are cross-checked against the nktkas TypeScript SDK and probed live by
    // `undocumented_action_shapes_are_accepted`.
    // ----------------------------------------------------------------
    /// Supply, withdraw, repay or borrow against the borrow/lend markets.
    #[from(skip)]
    BorrowLend(BorrowLendAction),
    /// Create a sub-account owned by the signer.
    #[from(skip)]
    CreateSubAccount {
        /// Display name, 1 to 16 characters.
        name: String,
    },
    /// Rename an existing sub-account.
    #[from(skip)]
    SubAccountModify(SubAccountModify),
    /// Move perp USDC between the signer and one of its sub-accounts.
    #[from(skip)]
    SubAccountTransfer(SubAccountTransfer),
    /// Move a spot token between the signer and one of its sub-accounts.
    #[from(skip)]
    SubAccountSpotTransfer(SubAccountSpotTransfer),
    /// Create a vault led by the signer.
    #[from(skip)]
    CreateVault(CreateVault),
    /// Change a vault's deposit and withdrawal policy.
    #[from(skip)]
    VaultModify(VaultModify),
    /// Distribute USDC from a vault's leader balance to its depositors.
    #[from(skip)]
    VaultDistribute(VaultDistribute),
    /// Set the display name shown on the leaderboard.
    #[from(skip)]
    SetDisplayName {
        /// Name of at most 20 characters. Empty clears it.
        #[serde(rename = "displayName")]
        display_name: String,
    },
    /// Set the referrer whose code the signer was referred by. Can only be set once.
    #[from(skip)]
    SetReferrer {
        /// Referral code, 1 to 20 characters.
        code: String,
    },
    /// Claim a referral code for the signer to refer others with.
    #[from(skip)]
    RegisterReferrer {
        /// Referral code, 1 to 20 characters.
        code: String,
    },
    /// Spot account settings.
    #[from(skip)]
    SpotUser(SpotUserAction),
    /// Link a HyperEVM contract to a HIP-1 token so the token can be traded on the EVM.
    #[from(skip)]
    FinalizeEvmContract(FinalizeEvmContract),
    /// Validator signer action: jail or unjail the signer's own validator.
    #[from(skip)]
    #[serde(rename = "CSignerAction")]
    CSignerAction(CSignerAction),
    /// Validator action: register, change profile, or unregister.
    #[from(skip)]
    #[serde(rename = "CValidatorAction")]
    CValidatorAction(CValidatorAction),
    /// User-signed: link a staking account to a trading account.
    #[from(skip)]
    LinkStakingUser(LinkStakingUserAction),
    /// User-signed: disable a trading account's staking link.
    #[from(skip)]
    StakingLinkDisableTradingUser(StakingLinkDisableTradingUserAction),
    /// User-signed: enable or disable portfolio margin.
    #[from(skip)]
    UserPortfolioMargin(UserPortfolioMarginAction),
}

impl Action {
    /// Hash the action for signing.
    ///
    /// The hash is generated by serializing the action to MessagePack, appending the nonce,
    /// optional vault address, and optional expiry, then Keccak256 hashing.
    #[inline]
    pub fn hash(
        &self,
        nonce: u64,
        maybe_vault_address: Option<Address>,
        maybe_expires_after: Option<u64>,
    ) -> Result<B256, rmp_serde::encode::Error> {
        utils::rmp_hash(self, nonce, maybe_vault_address, maybe_expires_after)
    }
}

impl Action {
    /// Classify every action once for both ordinary and multisig signing.
    fn user_signed_typed_data(&self, chain: Chain) -> Option<TypedData> {
        let typed_data = match self {
            // Actions signed over the msgpack hash wrapped in `Agent`.
            Action::Order(_)
            | Action::Modify(_)
            | Action::BatchModify(_)
            | Action::Cancel(_)
            | Action::CancelByCloid(_)
            | Action::ScheduleCancel(_)
            | Action::EvmUserModify { .. }
            | Action::UpdateIsolatedMargin(_)
            | Action::UpdateLeverage(_)
            | Action::VaultTransfer(_)
            | Action::VaultModify(_)
            | Action::VaultDistribute(_)
            | Action::CreateVault(_)
            | Action::CreateSubAccount { .. }
            | Action::SubAccountModify(_)
            | Action::SubAccountTransfer(_)
            | Action::SubAccountSpotTransfer(_)
            | Action::SetDisplayName { .. }
            | Action::SetReferrer { .. }
            | Action::RegisterReferrer { .. }
            | Action::SpotUser(_)
            | Action::BorrowLend(_)
            | Action::FinalizeEvmContract(_)
            | Action::CSignerAction(_)
            | Action::CValidatorAction(_)
            | Action::AgentSendAsset(_)
            | Action::Noop
            | Action::GossipPriorityBid(_)
            | Action::AgentEnableDexAbstraction
            | Action::AgentSetAbstraction { .. }
            | Action::TwapOrder { .. }
            | Action::TwapCancel { .. }
            | Action::TrailingStop(_)
            | Action::CDeposit { .. }
            | Action::CWithdraw { .. }
            | Action::ReserveRequestWeight { .. }
            | Action::Hip3LiquidatorTransfer(_)
            | Action::UserOutcome(_)
            | Action::TopUpIsolatedOnlyMargin(_)
            | Action::ClaimRewards
            | Action::AuthorizeAqav2Role(_)
            | Action::ValidatorL1Stream(_)
            | Action::SpotDeploy(_)
            | Action::PerpDeploy(_)
            | Action::OutcomeDeploy(_)
            | Action::ActivateOutcomeDeployer(_)
            | Action::MultiSig(_) => return None,

            // Actions signed as EIP-712 typed data.
            Action::UsdSend(inner) => get_typed_data::<solidity::UsdSend>(inner, chain, None),
            Action::SendAsset(inner) => get_typed_data::<solidity::SendAsset>(inner, chain, None),
            Action::SendToEvmWithData(inner) => {
                get_typed_data::<solidity::SendToEvmWithData>(inner, chain, None)
            }
            Action::SpotSend(inner) => get_typed_data::<solidity::SpotSend>(inner, chain, None),
            Action::ApproveAgent(inner) => {
                get_typed_data::<solidity::ApproveAgent>(inner, chain, None)
            }
            Action::ApproveBuilderFee(inner) => {
                get_typed_data::<solidity::ApproveBuilderFee>(inner, chain, None)
            }
            Action::ConvertToMultiSigUser(inner) => {
                get_typed_data::<solidity::ConvertToMultiSigUser>(inner, chain, None)
            }
            Action::UserDexAbstraction(inner) => {
                get_typed_data::<solidity::UserDexAbstraction>(inner, chain, None)
            }
            Action::UserSetAbstraction(inner) => {
                get_typed_data::<solidity::UserSetAbstraction>(inner, chain, None)
            }
            Action::UserPortfolioMargin(inner) => {
                get_typed_data::<solidity::UserPortfolioMargin>(inner, chain, None)
            }
            Action::LinkStakingUser(inner) => {
                get_typed_data::<solidity::LinkStakingUser>(inner, chain, None)
            }
            Action::StakingLinkDisableTradingUser(inner) => {
                get_typed_data::<solidity::StakingLinkDisableTradingUser>(inner, chain, None)
            }
            Action::Withdraw3(inner) => get_typed_data::<solidity::Withdraw3>(inner, chain, None),
            Action::UsdClassTransfer(inner) => {
                get_typed_data::<solidity::UsdClassTransfer>(inner, chain, None)
            }
            Action::TokenDelegate(inner) => {
                get_typed_data::<solidity::TokenDelegate>(inner, chain, None)
            }
        };
        Some(typed_data)
    }

    /// Resolve typed data, including the request context for a multisig envelope.
    fn signing_typed_data(
        &self,
        nonce: u64,
        maybe_vault_address: Option<Address>,
        expires_after: Option<u64>,
        chain: Chain,
    ) -> anyhow::Result<Option<TypedData>> {
        if let Action::MultiSig(inner) = self {
            let multi_sig_hash =
                utils::rmp_hash(&inner, nonce, maybe_vault_address, expires_after)?;

            #[derive(Serialize)]
            #[serde(rename_all = "camelCase")]
            struct Envelope {
                hyperliquid_chain: String,
                multi_sig_action_hash: String,
                nonce: u64,
            }

            let envelope = Envelope {
                hyperliquid_chain: chain.to_string(),
                multi_sig_action_hash: multi_sig_hash.to_string(),
                nonce,
            };

            Ok(Some(get_typed_data::<solidity::SendMultiSig>(
                &envelope, chain, None,
            )))
        } else {
            Ok(self.user_signed_typed_data(chain))
        }
    }

    /// The `Agent` struct an msgpack-signed action wraps its hash in.
    fn agent(
        &self,
        nonce: u64,
        maybe_vault_address: Option<Address>,
        expires_after: Option<u64>,
        chain: Chain,
    ) -> anyhow::Result<solidity::Agent> {
        let connection_id = self
            .hash(nonce, maybe_vault_address, expires_after)
            .map_err(|e| anyhow::anyhow!("Failed to hash action: {}", e))?;
        Ok(solidity::Agent {
            source: if chain.is_mainnet() { "a" } else { "b" }.to_string(),
            connectionId: connection_id,
        })
    }

    /// Add multisig fields to any user-signed action's EIP-712 schema.
    /// L1 actions return `None` and use the multisig msgpack envelope instead.
    pub fn typed_data_multisig(
        &self,
        multi_sig_user: Address,
        lead: Address,
        chain: Chain,
    ) -> Option<TypedData> {
        self.user_signed_typed_data(chain)
            .map(|data| utils::multisig_typed_data(data, multi_sig_user, lead))
    }
}

/// API response wrapper.
///
/// The `Ok` variant contains a successful response, while `Err` holds an error message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "response")]
#[serde(rename_all = "camelCase")]
pub enum Response {
    Ok(OkResponse),
    Err(String),
}

/// Successful API response data.
///
/// Currently supports order responses and a default placeholder.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "camelCase")]
pub enum OkResponse {
    Order {
        statuses: Vec<OrderResponseStatus>,
    },
    Cancel {
        statuses: Vec<OrderResponseStatus>,
    },
    /// Reply to a TWAP order. A rejected TWAP is still a `status: ok` reply, with the
    /// reason in [`TwapOrderStatus::Error`].
    TwapOrder {
        status: TwapOrderStatus,
    },
    /// Reply to a TWAP cancel. A failed cancel is still a `status: ok` reply, with the
    /// reason in [`TwapCancelStatus::Error`].
    TwapCancel {
        status: TwapCancelStatus,
    },
    /// Reply to a trailing stop: the ID of the order it placed.
    TrailingStop {
        oid: u64,
    },
    /// Address of the sub-account just created. `data` is the bare address.
    CreateSubAccount(Address),
    /// Address of the vault just created. `data` is the bare address.
    CreateVault(Address),
    // should be ok?
    Default,
}

/// Outcome of a TWAP order, from the `twapOrder` reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TwapOrderStatus {
    /// The TWAP is running.
    Running {
        /// ID to cancel it with.
        #[serde(rename = "twapId")]
        twap_id: u64,
    },
    /// The exchange rejected the TWAP.
    Error(String),
}

/// Outcome of a TWAP cancel, from the `twapCancel` reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TwapCancelStatus {
    /// The TWAP was canceled.
    Success,
    /// The cancel failed, e.g. because the TWAP was already canceled or filled.
    Error(String),
}

impl Response {
    pub fn into_default(self) -> anyhow::Result<()> {
        match self {
            Response::Ok(OkResponse::Default) => Ok(()),
            Response::Err(err) => Err(ApiError(err).into()),
            other => Err(ApiError(format!("unexpected response: {other:?}")).into()),
        }
    }
}

impl Action {
    /// Signs this action synchronously and returns an `ActionRequest`.
    ///
    /// Computes the prehash using the action's signing method (RMP+Agent for orders/cancels,
    /// EIP-712 for transfers), then signs it with the provided signer.
    pub fn sign_sync<S: SignerSync>(
        self,
        signer: &S,
        nonce: u64,
        maybe_vault_address: Option<Address>,
        maybe_expires_after: Option<DateTime<Utc>>,
        chain: Chain,
    ) -> anyhow::Result<ActionRequest> {
        let expires_after = maybe_expires_after.map(|after| after.timestamp_millis() as u64);

        let alloy_sig =
            match self.signing_typed_data(nonce, maybe_vault_address, expires_after, chain)? {
                Some(typed_data) => signer.sign_dynamic_typed_data_sync(&typed_data)?,
                None => {
                    let agent = self.agent(nonce, maybe_vault_address, expires_after, chain)?;
                    signer.sign_typed_data_sync(&agent, &CORE_MAINNET_EIP712_DOMAIN)?
                }
            };

        let signature: Signature = alloy_sig.into();

        // Build the action request
        Ok(ActionRequest {
            signature,
            action: self,
            nonce,
            vault_address: maybe_vault_address,
            expires_after,
        })
    }

    /// Signs this action asynchronously and returns an `ActionRequest`.
    ///
    /// Computes the prehash using the action's signing method (RMP+Agent for orders/cancels,
    /// EIP-712 for transfers), then signs it with the provided signer.
    pub async fn sign<S: Signer + Send + Sync>(
        self,
        signer: &S,
        nonce: u64,
        maybe_vault_address: Option<Address>,
        maybe_expires_after: Option<DateTime<Utc>>,
        chain: Chain,
    ) -> anyhow::Result<ActionRequest> {
        let expires_after = maybe_expires_after.map(|after| after.timestamp_millis() as u64);

        let alloy_sig =
            match self.signing_typed_data(nonce, maybe_vault_address, expires_after, chain)? {
                Some(typed_data) => signer.sign_dynamic_typed_data(&typed_data).await?,
                None => {
                    let agent = self.agent(nonce, maybe_vault_address, expires_after, chain)?;
                    let typed_data =
                        TypedData::from_struct(&agent, Some(CORE_MAINNET_EIP712_DOMAIN.clone()));
                    signer.sign_dynamic_typed_data(&typed_data).await?
                }
            };

        let signature: Signature = alloy_sig.into();

        // Build the action request
        Ok(ActionRequest {
            signature,
            action: self,
            nonce,
            vault_address: maybe_vault_address,
            expires_after,
        })
    }

    /// Computes the hash to be signed for this action.
    ///
    /// Uses RMP serialization with Agent wrapper for orders/cancels, or EIP-712 typed data
    /// for transfers. Returns the final hash ready for signing.
    pub fn prehash(
        &self,
        nonce: u64,
        maybe_vault_address: Option<Address>,
        maybe_expires_after: Option<DateTime<Utc>>,
        chain: Chain,
    ) -> anyhow::Result<B256> {
        let expires_after = maybe_expires_after.map(|after| after.timestamp_millis() as u64);

        match self.signing_typed_data(nonce, maybe_vault_address, expires_after, chain)? {
            Some(typed_data) => Ok(typed_data.eip712_signing_hash()?),
            None => {
                let connection_id = self
                    .hash(nonce, maybe_vault_address, expires_after)
                    .map_err(|e| anyhow::anyhow!("Failed to hash action: {}", e))?;
                Ok(crate::hypercore::signing::agent_signing_hash(
                    chain,
                    connection_id,
                ))
            }
        }
    }

    /// Recovers the signer's address from a signature.
    ///
    /// Computes the prehash for this action and recovers the Ethereum address that
    /// created the signature using ECDSA recovery.
    pub fn recover(
        &self,
        signature: &Signature,
        nonce: u64,
        maybe_vault_address: Option<Address>,
        maybe_expires_after: Option<DateTime<Utc>>,
        chain: Chain,
    ) -> anyhow::Result<Address> {
        let recid = RecoveryId::from_byte(signature.v as u8 - 27_u8)
            .ok_or_else(|| anyhow::anyhow!("unable to convert recovery_id: {}", signature.v))?;
        let sig = alloy::signers::Signature::new(signature.r, signature.s, recid.is_y_odd());
        let prehash = self.prehash(nonce, maybe_vault_address, maybe_expires_after, chain)?;
        Ok(sig.recover_address_from_prehash(&prehash)?)
    }
}

/// Send USDC from the perpetual balance.
///
/// This action transfers USDC from your perpetual trading balance to another address.
/// The transfer happens on the Hyperliquid L1 and requires EIP-712 signature.
///
/// # Fields
///
/// - `signature_chain_id`: The chain ID for signature verification (use [`crate::hypercore::ARBITRUM_MAINNET_CHAIN_ID`] or [`crate::hypercore::ARBITRUM_TESTNET_CHAIN_ID`])
/// - `hyperliquid_chain`: Whether this is mainnet or testnet
/// - `destination`: The recipient's address
/// - `amount`: Amount of USDC to send (in USDC, not wei)
/// - `time`: Timestamp in milliseconds (should match the nonce)
///
/// # Example
///
/// ```rust,ignore
/// use hypersdk::hypercore::types::raw::UsdSendAction;
/// use rust_decimal::dec;
///
/// let send = UsdSendAction {
///     signature_chain_id: ARBITRUM_MAINNET_CHAIN_ID,
///     hyperliquid_chain: Chain::Mainnet,
///     destination: "0x1234...".parse()?,
///     amount: dec!(100), // 100 USDC
///     time: chrono::Utc::now().timestamp_millis() as u64,
/// };
/// ```
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#core-usdc-transfer>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsdSendAction {
    /// Signature chain ID.
    ///
    /// For arbitrum use [`crate::hypercore::ARBITRUM_MAINNET_CHAIN_ID`] or [`crate::hypercore::ARBITRUM_TESTNET_CHAIN_ID`].
    pub signature_chain_id: String,
    /// The chain this action is being executed on.
    pub hyperliquid_chain: Chain,
    /// The destination address.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub destination: Address,
    /// The amount.
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    /// Current time, should match the nonce
    pub time: u64,
}

/// Send spot tokens to another address.
///
/// This action transfers spot tokens (like PURR, HYPE, etc.) from your spot balance
/// to another address. The transfer happens on the Hyperliquid L1 and requires EIP-712 signature.
///
/// # Fields
///
/// - `signature_chain_id`: The chain ID for signature verification (use [`crate::hypercore::ARBITRUM_MAINNET_CHAIN_ID`] or [`crate::hypercore::ARBITRUM_TESTNET_CHAIN_ID`])
/// - `hyperliquid_chain`: Whether this is mainnet or testnet
/// - `destination`: The recipient's address
/// - `token`: The spot token to send (wrapped in `SendToken`)
/// - `amount`: Amount to send (in token's native units)
/// - `time`: Timestamp in milliseconds (should match the nonce)
///
/// # Example
///
/// ```rust,ignore
/// use hypersdk::hypercore::types::raw::{SpotSendAction, SendToken};
/// use rust_decimal::dec;
///
/// let send = SpotSendAction {
///     signature_chain_id: ARBITRUM_MAINNET_CHAIN_ID,
///     hyperliquid_chain: Chain::Mainnet,
///     destination: "0x1234...".parse()?,
///     token: SendToken(purr_token),
///     amount: dec!(1000),
///     time: chrono::Utc::now().timestamp_millis() as u64,
/// };
/// ```
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#core-spot-transfer>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpotSendAction {
    /// Signature chain ID.
    ///
    /// For arbitrum use [`crate::hypercore::ARBITRUM_MAINNET_CHAIN_ID`] or [`crate::hypercore::ARBITRUM_TESTNET_CHAIN_ID`].
    pub signature_chain_id: String,
    /// The chain this action is being executed on.
    pub hyperliquid_chain: Chain,
    /// The destination address.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub destination: Address,
    /// Token
    pub token: String,
    /// The amount.
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    /// Current time, should match the nonce
    pub time: u64,
}

/// Send asset.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#send-asset>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SendAssetAction {
    /// Signature chain ID.
    ///
    /// For arbitrum use [`crate::hypercore::ARBITRUM_MAINNET_CHAIN_ID`] or [`crate::hypercore::ARBITRUM_TESTNET_CHAIN_ID`].
    pub signature_chain_id: String,
    /// The chain this action is being executed on.
    pub hyperliquid_chain: Chain,
    /// The destination address.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub destination: Address,
    /// Source DEX, can be empty
    pub source_dex: String,
    /// Destination DEX, can be empty
    pub destination_dex: String,
    /// Token
    pub token: String,
    /// The amount.
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    /// From subaccount, can be empty
    pub from_sub_account: String,
    /// Request nonce
    pub nonce: u64,
}

/// Agent-signed send asset.
///
/// Similar to [`SendAssetAction`] but signed with an agent (API wallet) using
/// L1-action signing (msgpack + `Agent` wrapper). The `destination` must equal
/// the source address, so this is restricted to self-transfers across DEXes,
/// the spot balance, or between subaccounts.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#agent-send-asset>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentSendAssetAction {
    /// The destination address (must equal the source address).
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub destination: Address,
    /// Source DEX, empty string for the default USDC perp DEX or "spot" for spot.
    pub source_dex: String,
    /// Destination DEX, empty string for the default USDC perp DEX or "spot" for spot.
    pub destination_dex: String,
    /// Token, e.g. `"PURR:0xc4bf3f870c0e9465323c0b6ed28096c2"`.
    pub token: String,
    /// Amount to send.
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    /// Source subaccount address, or empty string if sending from the main account.
    pub from_sub_account: String,
    /// Request nonce (timestamp in ms); must match the outer nonce.
    pub nonce: u64,
}

/// Approve agent
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#approve-an-api-wallet>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApproveAgent {
    /// Signature chain ID.
    ///
    /// For arbitrum use [`crate::hypercore::ARBITRUM_MAINNET_CHAIN_ID`] or [`crate::hypercore::ARBITRUM_TESTNET_CHAIN_ID`].
    pub signature_chain_id: String,
    /// The chain this action is being executed on.
    pub hyperliquid_chain: Chain,
    /// The agent address.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub agent_address: Address,
    /// Agent name.
    ///
    /// An account can have 1 unnamed approved wallet,
    /// up to 3 named ones, and 2 named agents per subaccount.
    #[serde(serialize_with = "utils::serialize_agent_name")]
    pub agent_name: Option<String>,
    /// Request nonce
    pub nonce: u64,
}

/// Approve builder fee.
///
/// Approves the maximum fee rate a builder is allowed to charge for routed orders.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#approve-a-builder-fee>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApproveBuilderFee {
    /// Signature chain ID.
    ///
    /// For arbitrum use [`crate::hypercore::ARBITRUM_MAINNET_CHAIN_ID`] or [`crate::hypercore::ARBITRUM_TESTNET_CHAIN_ID`].
    pub signature_chain_id: String,
    /// The chain this action is being executed on.
    pub hyperliquid_chain: Chain,
    /// The maximum allowed builder fee rate as a percent string; e.g. "0.001%".
    pub max_fee_rate: String,
    /// Builder address.
    pub builder: Address,
    /// Request nonce (timestamp in milliseconds).
    /// Must match nonce in outer request body.
    pub nonce: u64,
}

/// Multisig configuration for converting an account to multisig.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SignersConfig {
    /// Addresses authorized to sign for this multisig account
    pub authorized_users: Vec<Address>,
    /// Minimum number of signatures required (e.g., 2 for 2-of-3)
    pub threshold: usize,
}

/// Convert account to multi-signature user.
///
/// Converts a regular account to a multisig account by specifying authorized signers
/// and the required signature threshold.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConvertToMultiSigUser {
    /// Signature chain ID.
    ///
    /// For arbitrum use [`crate::hypercore::ARBITRUM_MAINNET_CHAIN_ID`] or [`crate::hypercore::ARBITRUM_TESTNET_CHAIN_ID`].
    pub signature_chain_id: String,
    /// The chain this action is being executed on.
    pub hyperliquid_chain: Chain,
    /// Signers configuration (authorized users and threshold) as JSON string
    #[serde(serialize_with = "crate::hypercore::utils::serialize_signers_as_json")]
    #[serde(deserialize_with = "crate::hypercore::utils::deserialize_signers_as_json")]
    pub signers: SignersConfig,
    /// Request nonce
    pub nonce: u64,
}

/// Request to update isolated margin for a position.
///
/// Allows adding or removing margin from an isolated-margin position.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIsolatedMargin {
    /// Asset index of the position.
    pub asset: usize,
    /// `true` for a long position, `false` for a short position.
    pub is_buy: bool,
    /// Margin delta in USD with 6 decimals (`1_000_000` is $1). Positive adds margin to the
    /// position, negative removes it.
    pub ntli: i64,
}

/// Request to update leverage for a perpetual asset.
///
/// Sets the leverage and margin mode (cross or isolated) for a specific asset.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#update-leverage>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLeverage {
    /// Asset index of the perpetual.
    pub asset: usize,
    /// `true` for cross margin, `false` for isolated margin.
    pub is_cross: bool,
    /// Leverage value (e.g., 10 for 10x).
    pub leverage: u32,
}

/// Deposit or withdraw USDC from a vault.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#vault-transfer>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VaultTransfer {
    /// The vault address to deposit into or withdraw from.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub vault_address: Address,
    /// `true` for deposit, `false` for withdrawal.
    pub is_deposit: bool,
    /// Amount of USDC in micro-units (1 USD = 1,000,000).
    pub usd: u64,
}

/// Account abstraction mode for Hyperliquid.
///
/// Determines how spot and perps balances interact:
/// - **Standard** (`"i"` / `"disabled"`): Separate perp and spot balances, separate DEX balances.
///   No daily action limits. Required for builder fee accrual.
/// - **UnifiedAccount** (`"u"` / `"unifiedAccount"`): Single balance per asset across all DEXes.
///   Limited to 50k user actions per day.
/// - **PortfolioMargin** (`"p"` / `"portfolioMargin"`): Most capital-efficient. Pre-alpha.
///   Limited to 50k user actions per day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, derive_more::Display)]
pub enum AbstractionMode {
    #[default]
    Standard,
    UnifiedAccount,
    PortfolioMargin,
}

impl AbstractionMode {
    /// Returns the full API string (used in info queries and user-signed actions).
    #[must_use]
    pub fn api_str(&self) -> &'static str {
        match self {
            Self::Standard => "disabled",
            Self::UnifiedAccount => "unifiedAccount",
            Self::PortfolioMargin => "portfolioMargin",
        }
    }

    /// Returns the short code used in agent-signed actions.
    #[must_use]
    pub fn agent_code(&self) -> &'static str {
        match self {
            Self::Standard => "i",
            Self::UnifiedAccount => "u",
            Self::PortfolioMargin => "p",
        }
    }

    /// Parses an abstraction mode from its API string or short code.
    pub fn from_api_str(s: &str) -> Result<Self, String> {
        match s {
            "disabled" | "i" | "standard" | "Standard" => Ok(Self::Standard),
            "unifiedAccount" | "u" | "unified" => Ok(Self::UnifiedAccount),
            "portfolioMargin" | "p" | "portfolio" => Ok(Self::PortfolioMargin),
            other => Err(format!("unknown abstraction mode: {other}")),
        }
    }

    #[must_use]
    pub const fn is_standard(&self) -> bool {
        matches!(self, Self::Standard)
    }

    #[must_use]
    pub const fn is_unified_account(&self) -> bool {
        matches!(self, Self::UnifiedAccount)
    }

    #[must_use]
    pub const fn is_portfolio_margin(&self) -> bool {
        matches!(self, Self::PortfolioMargin)
    }

    #[must_use]
    pub const fn has_daily_action_limit(&self) -> bool {
        !matches!(self, Self::Standard)
    }
}

fn serialize_abstraction_api<S>(mode: &AbstractionMode, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(mode.api_str())
}

fn deserialize_abstraction_api<'de, D>(deserializer: D) -> Result<AbstractionMode, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    AbstractionMode::from_api_str(&s).map_err(serde::de::Error::custom)
}

fn serialize_abstraction_agent<S>(mode: &AbstractionMode, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(mode.agent_code())
}

fn deserialize_abstraction_agent<'de, D>(deserializer: D) -> Result<AbstractionMode, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    AbstractionMode::from_api_str(&s).map_err(serde::de::Error::custom)
}

/// Gossip priority bid action.
///
/// Bids on a Dutch auction slot for read-priority gossip data. Lower slotId = higher
/// priority. Fees are deducted from the spot HYPE balance and burned.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/priority-fees>
#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GossipPriorityBid {
    /// Slot index (0–4). Lower index = higher priority (~10ms faster per slot).
    pub slot_id: u8,
    /// IP address to receive prioritized gossip data.
    pub ip: String,
    /// Maximum HYPE to bid in wei (1 HYPE = 10^18 wei).
    ///
    /// Serialized as a plain JSON number since Hyperliquid's API accepts u64-safe values.
    pub max_gas: u64,
}

/// User-signed DEX abstraction action.
///
/// Enables or disables DEX abstraction for a given user address. This uses EIP-712
/// signing with the `HyperliquidTransaction:UserDexAbstraction` type.
///
/// > **Deprecated**: DEX abstraction is being discontinued. Prefer [`UserSetAbstractionAction`]
/// > with [`crate::hypercore::AbstractionMode`] instead.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#enable-dex-abstraction-user-signed>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UserDexAbstractionAction {
    /// Signature chain ID (e.g., `"0x66eee"` for testnet, `"0xa4b1"` for mainnet).
    pub signature_chain_id: String,
    /// The chain this action is being executed on.
    pub hyperliquid_chain: Chain,
    /// The user address to enable/disable DEX abstraction for (lowercase hex).
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub user: Address,
    /// `true` to enable, `false` to disable DEX abstraction.
    pub enabled: bool,
    /// Request nonce (timestamp in ms).
    pub nonce: u64,
}

/// User-signed set-abstraction action.
///
/// Sets the account abstraction mode (Standard, UnifiedAccount, or PortfolioMargin)
/// for a given user address. This uses EIP-712 signing with the
/// `HyperliquidTransaction:UserSetAbstraction` type.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#set-account-abstraction-mode-user-signed>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UserSetAbstractionAction {
    /// Signature chain ID (e.g., `"0x66eee"` for testnet, `"0xa4b1"` for mainnet).
    pub signature_chain_id: String,
    /// The chain this action is being executed on.
    pub hyperliquid_chain: Chain,
    /// The user address to set the abstraction mode for (lowercase hex).
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub user: Address,
    /// The abstraction mode (e.g., Standard, UnifiedAccount, PortfolioMargin).
    #[serde(
        serialize_with = "serialize_abstraction_api",
        deserialize_with = "deserialize_abstraction_api"
    )]
    pub abstraction: AbstractionMode,
    /// Request nonce (timestamp in ms).
    pub nonce: u64,
}

/// Multi-signature action payload.
///
/// Contains the multisig user address, outer signer, and the inner action to execute.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MultiSigPayload {
    /// The multisig account address
    pub multi_sig_user: String,
    /// The address executing the multisig action
    pub outer_signer: String,
    /// The inner action to execute
    pub action: Box<Action>,
}

impl MultiSigPayload {
    /// Compute the multisig L1 connection ID, including the outer request context.
    fn connection_id(
        &self,
        nonce: u64,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
    ) -> anyhow::Result<B256> {
        let user: Address = self.multi_sig_user.parse()?;
        let lead: Address = self.outer_signer.parse()?;
        Ok(utils::rmp_hash(
            &(
                const_hex::encode_prefixed(user.as_slice()),
                const_hex::encode_prefixed(lead.as_slice()),
                &self.action,
            ),
            nonce,
            vault_address,
            expires_after.map(|time| time.timestamp_millis() as u64),
        )?)
    }

    /// Compute the signing hash without a vault or expiry.
    pub fn prehash(&self, nonce: u64, chain: Chain) -> anyhow::Result<B256> {
        self.prehash_with_context(nonce, None, None, chain)
    }

    /// Compute the signing hash using the same vault and expiry as the outer request.
    pub fn prehash_with_context(
        &self,
        nonce: u64,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
        chain: Chain,
    ) -> anyhow::Result<B256> {
        if let Some(data) = self.action.typed_data_multisig(
            self.multi_sig_user.parse()?,
            self.outer_signer.parse()?,
            chain,
        ) {
            Ok(data.eip712_signing_hash()?)
        } else {
            Ok(crate::hypercore::signing::agent_signing_hash(
                chain,
                self.connection_id(nonce, vault_address, expires_after)?,
            ))
        }
    }

    /// Sign synchronously without a vault or expiry.
    pub fn sign_sync<S: SignerSync>(
        &self,
        signer: &S,
        nonce: u64,
        chain: Chain,
    ) -> anyhow::Result<Signature> {
        self.sign_sync_with_context(signer, nonce, None, None, chain)
    }

    /// Sign synchronously using the outer request's vault and expiry.
    pub fn sign_sync_with_context<S: SignerSync>(
        &self,
        signer: &S,
        nonce: u64,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
        chain: Chain,
    ) -> anyhow::Result<Signature> {
        if let Some(data) = self.action.typed_data_multisig(
            self.multi_sig_user.parse()?,
            self.outer_signer.parse()?,
            chain,
        ) {
            Ok(signer.sign_dynamic_typed_data_sync(&data)?.into())
        } else {
            let agent = solidity::Agent {
                source: if chain.is_mainnet() { "a" } else { "b" }.to_string(),
                connectionId: self.connection_id(nonce, vault_address, expires_after)?,
            };
            Ok(signer
                .sign_typed_data_sync(&agent, &CORE_MAINNET_EIP712_DOMAIN)?
                .into())
        }
    }

    /// Sign asynchronously without a vault or expiry.
    pub async fn sign<S: Signer + Send + Sync>(
        &self,
        signer: &S,
        nonce: u64,
        chain: Chain,
    ) -> anyhow::Result<Signature> {
        self.sign_with_context(signer, nonce, None, None, chain)
            .await
    }

    /// Sign asynchronously using the outer request's vault and expiry.
    pub async fn sign_with_context<S: Signer + Send + Sync>(
        &self,
        signer: &S,
        nonce: u64,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
        chain: Chain,
    ) -> anyhow::Result<Signature> {
        if let Some(data) = self.action.typed_data_multisig(
            self.multi_sig_user.parse()?,
            self.outer_signer.parse()?,
            chain,
        ) {
            Ok(signer.sign_dynamic_typed_data(&data).await?.into())
        } else {
            crate::hypercore::signing::sign_l1_action(
                signer,
                chain,
                self.connection_id(nonce, vault_address, expires_after)?,
            )
            .await
        }
    }

    /// Recover a signature made without a vault or expiry.
    pub fn recover(
        &self,
        signature: &Signature,
        nonce: u64,
        chain: Chain,
    ) -> anyhow::Result<Address> {
        self.recover_with_context(signature, nonce, None, None, chain)
    }

    /// Recover a signature using the outer request's vault and expiry.
    pub fn recover_with_context(
        &self,
        signature: &Signature,
        nonce: u64,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
        chain: Chain,
    ) -> anyhow::Result<Address> {
        let parity = alloy::primitives::normalize_v(signature.v)
            .ok_or_else(|| anyhow::anyhow!("invalid signature recovery id: {}", signature.v))?;
        let sig = alloy::signers::Signature::new(signature.r, signature.s, parity);
        let hash = self.prehash_with_context(nonce, vault_address, expires_after, chain)?;
        Ok(sig.recover_address_from_prehash(&hash)?)
    }
}

/// Multi-signature action wrapper.
///
/// Wraps any action with multiple signatures for multisig execution.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MultiSigAction {
    /// Signature chain ID (0x66eee for L1 multisig)
    pub signature_chain_id: String,
    /// Signatures from authorized signers
    pub signatures: Vec<Signature>,
    /// The multisig payload
    pub payload: MultiSigPayload,
}

/// TWAP order parameters.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TwapOrderParams {
    /// Asset index.
    pub a: usize,
    /// `true` for buy, `false` for sell.
    pub b: bool,
    /// Size.
    #[serde(with = "rust_decimal::serde::str")]
    pub s: Decimal,
    /// Reduce only.
    pub r: bool,
    /// Duration in minutes.
    pub m: u32,
    /// Randomize execution timing.
    pub t: bool,
}

/// A native trailing stop: a market order that triggers once the mark price retraces from
/// its best level since activation by `retracement`. Perp markets only.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#place-a-trailing-stop-order>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrailingStop {
    /// Asset index.
    pub asset: usize,
    /// `true` for buy, `false` for sell.
    pub is_buy: bool,
    /// Size.
    #[serde(with = "crate::hypercore::utils::decimal_normalized")]
    pub sz: Decimal,
    /// Reduce only.
    pub reduce_only: bool,
    /// How far the price retraces before the stop triggers.
    pub retracement: TrailingStopRetracement,
    /// Price at which the stop starts trailing. `None` starts it immediately.
    ///
    /// Sent as `null` when unset rather than omitted: the exchange hashes the field either
    /// way, so leaving it out changes the signature.
    #[serde(with = "crate::hypercore::utils::decimal_normalized_option")]
    pub activation_px: Option<Decimal>,
}

/// How far the price retraces before a [`TrailingStop`] triggers.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrailingStopRetracement {
    /// A percentage, e.g. `dec!(1.5)` for 1.5%. Sent as `"1.5%"`.
    Pct(#[serde(with = "percent")] Decimal),
    /// A price distance.
    Px(#[serde(with = "crate::hypercore::utils::decimal_normalized")] Decimal),
}

/// A percentage as the exchange writes it, e.g. `"1.5%"`.
mod percent {
    use std::str::FromStr;

    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serializer, de};

    pub fn serialize<S>(value: &Decimal, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{}%", value.normalize()))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let digits = s
            .strip_suffix('%')
            .ok_or_else(|| de::Error::custom(format!("percentage without %: `{s}`")))?;
        Decimal::from_str(digits)
            .map(|d| d.normalize())
            .map_err(de::Error::custom)
    }
}

/// Withdraw to Arbitrum L1.
///
/// Uses EIP-712 human-readable signing. $1 fee, ~5 minute finalization.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Withdraw3Action {
    pub signature_chain_id: String,
    pub hyperliquid_chain: Chain,
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub destination: Address,
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    pub time: u64,
}

/// Transfer between spot and perp balances.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsdClassTransferAction {
    pub signature_chain_id: String,
    pub hyperliquid_chain: Chain,
    /// Amount to transfer, optionally with " subaccount:0x..." suffix.
    pub amount: String,
    /// `true` to transfer to perp, `false` to transfer to spot.
    pub to_perp: bool,
    pub nonce: u64,
}

/// Delegate or undelegate staked tokens to a validator.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TokenDelegateAction {
    /// Signature chain ID used for EIP-712 signing.
    pub signature_chain_id: String,
    /// Target Hyperliquid network.
    pub hyperliquid_chain: Chain,
    /// Validator address.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub validator: Address,
    /// Amount in wei of native token.
    pub wei: u64,
    /// `true` to undelegate, `false` to delegate.
    pub is_undelegate: bool,
    /// Request nonce, matching the outer request.
    pub nonce: u64,
}

/// Encoding of [`SendToEvmWithDataAction::destination_recipient`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AddressEncoding {
    /// 20-byte address in `0x`-prefixed hex, the usual EVM form.
    Hex,
    /// Base58-encoded address, for destination chains that use it.
    Base58,
}

/// Transfer a token from Core to the EVM with an extra data payload.
///
/// The destination contract must implement `ICoreReceiveWithData`. Unlike
/// [`crate::hypercore::HttpClient::transfer_to_evm`], this carries `data` to the recipient and can
/// target a chain other than HyperEVM.
///
/// This is a user-signed EIP-712 action (`HyperliquidTransaction:SendToEvmWithData`).
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#send-to-evm-with-data>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SendToEvmWithDataAction {
    /// Signature chain ID.
    ///
    /// For arbitrum use [`crate::hypercore::ARBITRUM_MAINNET_CHAIN_ID`] or [`crate::hypercore::ARBITRUM_TESTNET_CHAIN_ID`].
    pub signature_chain_id: String,
    /// The chain this action is being executed on.
    pub hyperliquid_chain: Chain,
    /// Token identifier, e.g. `"PURR:0xc4bf3f870c0e9465323c0b6ed28096c2"`.
    pub token: String,
    /// Amount of the token to send (not in wei).
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    /// Name of the perp DEX to transfer from. Empty for the spot balance.
    pub source_dex: String,
    /// Recipient on the destination chain, in [`Self::address_encoding`] format.
    pub destination_recipient: String,
    /// How [`Self::destination_recipient`] is encoded.
    pub address_encoding: AddressEncoding,
    /// Destination chain ID.
    pub destination_chain_id: u32,
    /// Gas limit for execution on the destination chain.
    pub gas_limit: u64,
    /// Data payload passed to the receiving contract. Empty is `0x`.
    pub data: Bytes,
    /// Request nonce, must match the outer nonce.
    pub nonce: u64,
}

/// Add isolated margin to reach a target leverage.
///
/// The alternative to [`UpdateIsolatedMargin`], which moves a fixed USDC amount instead.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#update-isolated-margin>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TopUpIsolatedOnlyMargin {
    /// Asset index.
    pub asset: u32,
    /// Target leverage.
    #[serde(with = "rust_decimal::serde::str")]
    pub leverage: Decimal,
}

/// The role being authorized by [`Action::AuthorizeAqav2Role`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Aqav2Role {
    /// Technical operator role.
    Technical,
    /// Treasury operator role.
    Treasury,
}

/// Authorize an AQAv2 role for an aligned quote asset.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#authorize-aqav2-role>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizeAqav2Role {
    /// Token index, e.g. `0` for USDC.
    pub token: u32,
    /// The role to authorize.
    pub role: Aqav2Role,
}

/// Validator vote on the risk-free rate for an aligned quote asset.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#validator-vote-on-risk-free-rate-for-aligned-quote-asset>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ValidatorL1Stream {
    /// Annualized risk-free rate, e.g. `0.04` for 4%.
    #[serde(with = "rust_decimal::serde::str")]
    pub risk_free_rate: Decimal,
}

/// Replace a single resting order (`modify`).
///
/// The batch form is [`BatchModify`], which carries the same `always_place` flag.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#modify-an-order>
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModifyAction {
    /// Order to replace, either a numeric `oid` or a client-supplied `cloid`.
    #[serde(with = "crate::hypercore::utils::oid_or_cloid")]
    pub oid: OidOrCloid,
    /// Replacement order.
    pub order: OrderRequest,
    /// Place the new order even if the cancel failed.
    ///
    /// When `false` the new order must be a non-trigger ALO, or a non-executable GTC whose TIF
    /// is then overridden to ALO. Serialized as `a`, and omitted when `false`: the exchange
    /// rejects an action hashed with `a: false`.
    #[serde(
        rename = "a",
        default,
        skip_serializing_if = "std::ops::Not::not",
        alias = "alwaysPlace"
    )]
    pub always_place: bool,
}

/// Borrow/lend market operation (`borrowLend`).
///
/// Undocumented, live on mainnet.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BorrowLendAction {
    /// Which side of the market to act on.
    pub operation: BorrowLendOperation,
    /// Token index.
    pub token: u32,
    /// Amount, or `None` for the maximum available.
    #[serde(with = "rust_decimal::serde::str_option")]
    pub amount: Option<Decimal>,
}

/// The operation of a [`BorrowLendAction`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BorrowLendOperation {
    /// Lend tokens into the reserve.
    Supply,
    /// Take lent tokens back out of the reserve.
    Withdraw,
    /// Pay back borrowed tokens.
    Repay,
    /// Borrow tokens against collateral.
    Borrow,
}

/// Rename a sub-account (`subAccountModify`).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SubAccountModify {
    /// The sub-account to rename.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub sub_account_user: Address,
    /// New display name, 1 to 16 characters.
    pub name: String,
}

/// Move perp USDC to or from a sub-account (`subAccountTransfer`).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SubAccountTransfer {
    /// The sub-account on the other side of the transfer.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub sub_account_user: Address,
    /// `true` moves funds into the sub-account, `false` moves them back out.
    pub is_deposit: bool,
    /// Amount in 1e-6 USDC units.
    pub usd: u64,
}

/// Move a spot token to or from a sub-account (`subAccountSpotTransfer`).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SubAccountSpotTransfer {
    /// The sub-account on the other side of the transfer.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub sub_account_user: Address,
    /// `true` moves funds into the sub-account, `false` moves them back out.
    pub is_deposit: bool,
    /// Token identifier in `NAME:0x...` form, as returned by `spotMeta`.
    pub token: String,
    /// Amount as a decimal string.
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
}

/// Create a vault (`createVault`).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateVault {
    /// Vault name, 3 to 50 characters.
    pub name: String,
    /// Vault description, 10 to 250 characters.
    pub description: String,
    /// Leader's initial deposit in 1e-6 USDC units. At least 100 USDC.
    pub initial_usd: u64,
    /// Creation nonce. Carried inside the action as well as alongside it.
    pub nonce: u64,
}

/// Change a vault's deposit and withdrawal policy (`vaultModify`).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VaultModify {
    /// The vault to change.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub vault_address: Address,
    /// Whether new deposits are accepted. `None` leaves it unchanged.
    pub allow_deposits: Option<bool>,
    /// Whether a withdrawal always closes the depositor's share of open positions.
    /// `None` leaves it unchanged.
    pub always_close_on_withdraw: Option<bool>,
}

/// Pay USDC from a vault leader's balance out to depositors (`vaultDistribute`).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VaultDistribute {
    /// The vault distributing.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub vault_address: Address,
    /// Amount in 1e-6 USDC units. `0` closes the vault.
    pub usd: u64,
}

/// Spot account settings (`spotUser`).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpotUserAction {
    /// Opt in or out of automatic dusting of small spot balances.
    pub toggle_spot_dusting: ToggleSpotDusting,
}

/// The dusting setting carried by [`SpotUserAction`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct ToggleSpotDusting {
    /// `true` stops the exchange dusting small balances into USDC.
    pub opt_out: bool,
}

/// Link a HyperEVM contract to a HIP-1 token (`finalizeEvmContract`).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FinalizeEvmContract {
    /// Token index.
    pub token: u32,
    /// How ownership of the contract is proven.
    pub input: FinalizeEvmContractInput,
}

/// The ownership proof carried by [`FinalizeEvmContract`].
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum FinalizeEvmContractInput {
    /// The contract was deployed by an EOA at this nonce.
    Create {
        /// Deployment nonce of the EOA.
        nonce: u64,
    },
    /// The deployer address is in the contract's first storage slot.
    #[serde(rename = "firstStorageSlot")]
    FirstStorageSlot,
    /// The deployer address is in a custom storage slot.
    #[serde(rename = "customStorageSlot")]
    CustomStorageSlot,
}

/// Validator signer action (`CSignerAction`).
///
/// Serializes as `{"type": "CSignerAction", "<variant>": null}`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum CSignerAction {
    /// Take the signer's validator out of consensus.
    JailSelf(()),
    /// Return the signer's validator to consensus.
    UnjailSelf(()),
}

/// Validator action (`CValidatorAction`).
///
/// Unlike the rest of the exchange API these payloads use snake_case field names, and the
/// node address is keyed `Ip` with a capital I.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum CValidatorAction {
    /// Register a new validator.
    Register(ValidatorRegistration),
    /// Change an existing validator's profile.
    ChangeProfile(ValidatorProfileChange),
    /// Deregister the signer's validator. Serializes as `{"unregister": null}`.
    Unregister(()),
}

/// A new validator's registration, carried by [`CValidatorAction::Register`].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ValidatorRegistration {
    /// Public profile shown in the validator list.
    pub profile: ValidatorProfile,
    /// Whether the validator starts in consensus rather than jailed.
    pub unjailed: bool,
    /// Self-delegated stake in wei.
    pub initial_wei: u64,
}

/// A validator's public profile, as given at registration.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ValidatorProfile {
    /// Consensus node address.
    pub node_ip: ValidatorNodeIp,
    /// Display name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Whether delegations from others are refused.
    pub delegations_disabled: bool,
    /// Commission rate in basis points.
    pub commission_bps: u64,
    /// Address authorized to sign consensus messages.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub signer: Address,
}

/// A change to an existing validator's profile, carried by
/// [`CValidatorAction::ChangeProfile`]. Every `None` field is left as it was.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ValidatorProfileChange {
    /// Consensus node address.
    pub node_ip: Option<ValidatorNodeIp>,
    /// Display name.
    pub name: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Whether the validator is in consensus.
    pub unjailed: bool,
    /// Whether delegations from others are refused.
    pub disable_delegations: Option<bool>,
    /// Commission rate in basis points.
    pub commission_bps: Option<u64>,
    /// Address authorized to sign consensus messages.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_option_address_as_hex",
        default
    )]
    pub signer: Option<Address>,
}

/// A validator's consensus node address.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ValidatorNodeIp {
    /// IP address of the node.
    #[serde(rename = "Ip")]
    pub ip: String,
}

/// User-signed link of a staking account to a trading account (`linkStakingUser`).
///
/// EIP-712 type `HyperliquidTransaction:LinkStakingUser`. Sent twice: once by the staking
/// account with `is_finalize = false`, then by the trading account with `is_finalize = true`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LinkStakingUserAction {
    /// Signature chain ID (e.g., `"0x66eee"` for testnet, `"0xa4b1"` for mainnet).
    pub signature_chain_id: String,
    /// The chain this action is being executed on.
    pub hyperliquid_chain: Chain,
    /// The account on the other side of the link (lowercase hex).
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub user: Address,
    /// `false` proposes the link, `true` accepts it.
    pub is_finalize: bool,
    /// Request nonce (timestamp in ms).
    pub nonce: u64,
}

/// User-signed removal of a trading account's staking link
/// (`stakingLinkDisableTradingUser`).
///
/// EIP-712 type `HyperliquidTransaction:StakingLinkDisableTradingUser`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StakingLinkDisableTradingUserAction {
    /// Signature chain ID (e.g., `"0x66eee"` for testnet, `"0xa4b1"` for mainnet).
    pub signature_chain_id: String,
    /// The chain this action is being executed on.
    pub hyperliquid_chain: Chain,
    /// The trading account to unlink (lowercase hex).
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub trading_user: Address,
    /// Request nonce (timestamp in ms).
    pub nonce: u64,
}

/// User-signed portfolio margin toggle (`userPortfolioMargin`).
///
/// EIP-712 type `HyperliquidTransaction:UserPortfolioMargin`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UserPortfolioMarginAction {
    /// Signature chain ID (e.g., `"0x66eee"` for testnet, `"0xa4b1"` for mainnet).
    pub signature_chain_id: String,
    /// The chain this action is being executed on.
    pub hyperliquid_chain: Chain,
    /// The account to change (lowercase hex).
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub user: Address,
    /// `true` enables portfolio margin.
    pub enabled: bool,
    /// Request nonce (timestamp in ms).
    pub nonce: u64,
}

/// HIP-3 backstop liquidator transfer.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Hip3LiquidatorTransferAction {
    /// DEX name.
    pub dex: String,
    /// Notional amount in 1e-6 units (must be multiple of 1,000,000,000).
    pub ntl: u64,
    /// `true` to deposit, `false` to withdraw.
    pub is_deposit: bool,
}

/// HIP-4 outcome token action (`userOutcome`).
///
/// Outcome markets are fully collateralized binary contracts. The quote token (e.g. USDH)
/// can be split into one share of each side of an outcome, and matching shares can be merged
/// back into the quote token. Exactly one of the optional operations should be set; the others
/// are omitted from the request.
///
/// - [`split_outcome`](Self::split_outcome): burn `amount` of the quote token, mint `amount` of
///   each side (e.g. 1 YES + 1 NO) of `outcome`.
/// - [`merge_outcome`](Self::merge_outcome): burn `amount` of each side of `outcome`, returning
///   `amount` of the quote token. `amount = None` merges the maximum available.
/// - [`merge_question`](Self::merge_question): merge a full set of mutually-exclusive outcomes
///   within a categorical `question` back into the quote token. `amount = None` merges the max.
/// - [`negate_outcome`](Self::negate_outcome): within a categorical `question`, convert shares of
///   one `outcome` into shares of the complementary basket (the "No" of that outcome).
///
/// `outcome` and `question` are the integer IDs from the `outcomeMeta` info endpoint
/// (see [`crate::hypercore::OutcomeInfo::outcome`] and [`crate::hypercore::OutcomeQuestion::question`]).
///
/// This is an L1 (agent-signable) action signed via the same msgpack + `Agent` wrapper used by
/// orders and cancels.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/hyperliquid-improvement-proposals-hips/hip-4-outcome-markets>
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserOutcomeAction {
    /// Mint one share of each side of an outcome from the quote token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_outcome: Option<SplitOutcome>,
    /// Burn matching shares of an outcome back into the quote token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_outcome: Option<MergeOutcome>,
    /// Merge a full set of outcomes within a question back into the quote token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_question: Option<MergeQuestion>,
    /// Convert shares of one outcome into the complementary basket within a question.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negate_outcome: Option<NegateOutcome>,
}

impl UserOutcomeAction {
    /// Build a [`Self`] that splits the quote token into both sides of `outcome`.
    #[must_use]
    pub fn split(outcome: u32, amount: Decimal) -> Self {
        Self {
            split_outcome: Some(SplitOutcome { outcome, amount }),
            ..Default::default()
        }
    }

    /// Build a [`Self`] that merges both sides of `outcome` back into the quote token.
    ///
    /// `amount = None` merges the maximum available.
    #[must_use]
    pub fn merge(outcome: u32, amount: Option<Decimal>) -> Self {
        Self {
            merge_outcome: Some(MergeOutcome { outcome, amount }),
            ..Default::default()
        }
    }

    /// Build a [`Self`] that merges a full set of outcomes within `question`.
    ///
    /// `amount = None` merges the maximum available.
    #[must_use]
    pub fn merge_question(question: u32, amount: Option<Decimal>) -> Self {
        Self {
            merge_question: Some(MergeQuestion { question, amount }),
            ..Default::default()
        }
    }

    /// Build a [`Self`] that negates `outcome` within `question`.
    #[must_use]
    pub fn negate(question: u32, outcome: u32, amount: Decimal) -> Self {
        Self {
            negate_outcome: Some(NegateOutcome {
                question,
                outcome,
                amount,
            }),
            ..Default::default()
        }
    }
}

/// Split the quote token into one share of each side of an outcome.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SplitOutcome {
    /// Outcome ID from `outcomeMeta`.
    pub outcome: u32,
    /// Amount of the quote token to split.
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
}

/// Merge matching shares of an outcome back into the quote token.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MergeOutcome {
    /// Outcome ID from `outcomeMeta`.
    pub outcome: u32,
    /// Amount to merge, or `None` for the maximum available.
    #[serde(default, with = "rust_decimal::serde::str_option")]
    pub amount: Option<Decimal>,
}

/// Merge a full set of mutually-exclusive outcomes within a question.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MergeQuestion {
    /// Question ID from `outcomeMeta`.
    pub question: u32,
    /// Amount to merge, or `None` for the maximum available.
    #[serde(default, with = "rust_decimal::serde::str_option")]
    pub amount: Option<Decimal>,
}

/// Convert shares of one outcome into the complementary basket within a question.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NegateOutcome {
    /// Question ID from `outcomeMeta`.
    pub question: u32,
    /// Outcome ID to negate from `outcomeMeta`.
    pub outcome: u32,
    /// Amount to negate.
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
}

/// The address the exchange recovered from a signed request, when it is not `signer`.
///
/// A request signed by an account that does not exist is answered with "User or API Wallet
/// 0x… does not exist." or "Must deposit before performing actions. User: 0x…", naming the
/// address recovered from the signature. Any address other than the signer's means the signed
/// bytes differ from what the exchange hashes, even though the payload parsed.
#[cfg(all(test, feature = "client"))]
pub(crate) fn recovered_other_signer(reply: &str, signer: Address) -> Option<String> {
    let start = ["User or API Wallet ", "User: "]
        .iter()
        .find_map(|marker| Some(reply.find(&format!("{marker}0x"))? + marker.len()))?;
    let recovered = reply.get(start..start + 42)?;
    (!recovered.eq_ignore_ascii_case(&format!("{signer:#x}"))).then(|| recovered.to_owned())
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{address, b256};

    use super::*;

    /// TWAP replies carry their own `type`, which used to fail to deserialize, so a TWAP
    /// that the exchange accepted was reported as an error. These are the documented replies.
    #[test]
    fn twap_replies_deserialize() {
        let parse = |text: &str| serde_json::from_str::<Response>(text).unwrap();

        assert!(matches!(
            parse(
                r#"{"status":"ok","response":{"type":"twapOrder","data":{"status":{"running":{"twapId":77738308}}}}}"#
            ),
            Response::Ok(OkResponse::TwapOrder {
                status: TwapOrderStatus::Running { twap_id: 77738308 }
            })
        ));
        assert!(matches!(
            parse(
                r#"{"status":"ok","response":{"type":"twapOrder","data":{"status":{"error":"Invalid TWAP duration: 1 min(s)"}}}}"#
            ),
            Response::Ok(OkResponse::TwapOrder {
                status: TwapOrderStatus::Error(error)
            }) if error == "Invalid TWAP duration: 1 min(s)"
        ));
        assert!(matches!(
            parse(
                r#"{"status":"ok","response":{"type":"twapCancel","data":{"status":"success"}}}"#
            ),
            Response::Ok(OkResponse::TwapCancel {
                status: TwapCancelStatus::Success
            })
        ));
        assert!(matches!(
            parse(
                r#"{"status":"ok","response":{"type":"twapCancel","data":{"status":{"error":"TWAP was never placed, already canceled, or filled."}}}}"#
            ),
            Response::Ok(OkResponse::TwapCancel {
                status: TwapCancelStatus::Error(_)
            })
        ));
    }

    /// The documented shape, with the percentage suffixed and an unset `activationPx` sent
    /// as `null`, which the exchange hashes.
    #[test]
    fn trailing_stop_serializes_as_documented() {
        use rust_decimal::dec;

        let pct = Action::TrailingStop(TrailingStop {
            asset: 0,
            is_buy: false,
            sz: dec!(0.0010),
            reduce_only: true,
            retracement: TrailingStopRetracement::Pct(dec!(1.50)),
            activation_px: None,
        });
        assert_eq!(
            serde_json::to_value(&pct).unwrap(),
            serde_json::json!({
                "type": "trailingStop", "asset": 0, "isBuy": false, "sz": "0.001",
                "reduceOnly": true, "retracement": {"pct": "1.5%"}, "activationPx": null
            })
        );

        let px = Action::TrailingStop(TrailingStop {
            asset: 3,
            is_buy: true,
            sz: dec!(2),
            reduce_only: false,
            retracement: TrailingStopRetracement::Px(dec!(500.0)),
            activation_px: Some(dec!(120000.0)),
        });
        let json = serde_json::to_value(&px).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "trailingStop", "asset": 3, "isBuy": true, "sz": "2",
                "reduceOnly": false, "retracement": {"px": "500"}, "activationPx": "120000"
            })
        );

        let Action::TrailingStop(back) = serde_json::from_value(json).unwrap() else {
            panic!("expected a trailing stop");
        };
        assert_eq!(back.retracement, TrailingStopRetracement::Px(dec!(500)));
        assert_eq!(back.activation_px, Some(dec!(120000)));

        let back: TrailingStopRetracement =
            serde_json::from_value(serde_json::json!({"pct": "1.234%"})).unwrap();
        assert_eq!(back, TrailingStopRetracement::Pct(dec!(1.234)));
        assert!(
            serde_json::from_value::<TrailingStopRetracement>(serde_json::json!({"pct": "1.2"}))
                .is_err()
        );
    }

    #[test]
    fn trailing_stop_reply_deserializes() {
        let reply: Response = serde_json::from_str(
            r#"{"status":"ok","response":{"type":"trailingStop","data":{"oid":77738308}}}"#,
        )
        .unwrap();
        assert!(matches!(
            reply,
            Response::Ok(OkResponse::TrailingStop { oid: 77738308 })
        ));
    }

    /// The exchange hashes addresses as lowercase hex strings, but `alloy` encodes an `Address`
    /// as 20 raw bytes in msgpack. Every address inside an L1 action must be serialized
    /// explicitly, or the signature recovers to a different address.
    #[test]
    fn addresses_in_l1_actions_are_signed_as_hex_strings() {
        use crate::hypercore::types::deploy::*;

        let user = address!("0x5e89b26d8d66da9888c835c9bfcc2aa51813e152");
        let hex = format!("{user:#x}");
        let actions = [
            Action::SubAccountTransfer(SubAccountTransfer {
                sub_account_user: user,
                is_deposit: true,
                usd: 1,
            }),
            Action::VaultDistribute(VaultDistribute {
                vault_address: user,
                usd: 1,
            }),
            Action::ReserveRequestWeight {
                weight: 1,
                destination: Some(user),
            },
            Action::PerpDeploy(PerpDeployAction::SetFeeRecipient(SetFeeRecipient {
                dex: "abc".into(),
                fee_recipient: user,
            })),
            Action::PerpDeploy(PerpDeployAction::SetSubDeployers(SetSubDeployers {
                dex: "abc".into(),
                sub_deployers: vec![SubDeployerInput {
                    variant: "setOracle".into(),
                    user,
                    allowed: true,
                }],
            })),
            Action::SpotDeploy(SpotDeployAction::UserGenesis(UserGenesis {
                token: 1,
                user_and_wei: vec![(user, "1".into())],
                existing_token_and_wei: vec![],
                blacklist_users: None,
            })),
        ];

        for action in actions {
            let bytes = rmp_serde::to_vec_named(&action).unwrap();
            assert!(
                bytes
                    .windows(hex.len())
                    .any(|window| window == hex.as_bytes()),
                "{action:?} does not carry the address as a hex string"
            );
            assert!(
                !bytes.windows(20).any(|window| window == user.as_slice()),
                "{action:?} carries the address as raw bytes"
            );
        }
    }

    #[test]
    fn test_deser() {
        let text =
            r#"{"status":"ok","response":{"type":"cancel","data":{"statuses":["success"]}}}"#;
        let _data: Response = serde_json::from_str(text).unwrap();
    }

    #[test]
    fn update_isolated_margin() {
        let text = r#"{"action":{"type":"updateIsolatedMargin","asset":173,"isBuy":true,"ntli":2000000},"nonce":1768223623573,"signature":{"r":"0xf85df30c97a4f2cd6b463b5f385d1f93e029791ffc9bb49fdcad2616608350e2","s":"0x3763da7c7ef7a4d7a528815bddff75b854d540487dfb1f1c75e7201f57c2ea6e","v":28}}"#;
        let req: ActionRequest = serde_json::from_str(text).unwrap();
        let address = req.recover(Chain::Mainnet).unwrap();
        assert_eq!(
            address,
            address!("0x5eCb62791B22A3108367c2A2024019Ee7eA88431")
        );
    }

    /// Hashes from `action_hash` in the Python SDK 0.24.0 for the same actions. A negative
    /// `ntli` removes margin and must hash as a signed msgpack integer, as Python packs it.
    #[test]
    fn update_isolated_margin_hash_matches_python_sdk() {
        for (ntli, expected) in [
            (
                -1_000_000,
                b256!("0xa7ca3c20db07af75236a790cac3e8549264d796fe6f269ccc43dad43b7ca3274"),
            ),
            (
                1_000_000,
                b256!("0x840973ced4652cca505364f3a41e8d2a71f45d5bf76f9a74ba801ce40594ccdf"),
            ),
        ] {
            let action = Action::UpdateIsolatedMargin(UpdateIsolatedMargin {
                asset: 124,
                is_buy: true,
                ntli,
            });
            assert_eq!(action.hash(1760000000000, None, None).unwrap(), expected);
        }
    }

    #[test]
    fn send_to_evm_with_data_serialization() {
        use rust_decimal::dec;

        let action = Action::SendToEvmWithData(SendToEvmWithDataAction {
            signature_chain_id: "0xa4b1".to_string(),
            hyperliquid_chain: Chain::Mainnet,
            token: "USDC".to_string(),
            amount: dec!(1),
            source_dex: String::new(),
            destination_recipient: "0x0d1d9635d0640821d15e323ac8adadfa9c111414".to_string(),
            address_encoding: AddressEncoding::Hex,
            destination_chain_id: 42161,
            gas_limit: 200_000,
            data: Bytes::default(),
            nonce: 1690393044548,
        });

        assert_eq!(
            serde_json::to_string(&action).unwrap(),
            r#"{"type":"sendToEvmWithData","signatureChainId":"0xa4b1","hyperliquidChain":"Mainnet","token":"USDC","amount":"1","sourceDex":"","destinationRecipient":"0x0d1d9635d0640821d15e323ac8adadfa9c111414","addressEncoding":"hex","destinationChainId":42161,"gasLimit":200000,"data":"0x","nonce":1690393044548}"#
        );
    }

    /// The EIP-712 payload must resolve against `HyperliquidTransaction:SendToEvmWithData`
    /// with the field order the exchange expects, and `data` must coerce from its hex form.
    #[test]
    fn send_to_evm_with_data_prehash() {
        use rust_decimal::dec;

        let action = Action::SendToEvmWithData(SendToEvmWithDataAction {
            signature_chain_id: "0xa4b1".to_string(),
            hyperliquid_chain: Chain::Mainnet,
            token: "USDC".to_string(),
            amount: dec!(1),
            source_dex: String::new(),
            destination_recipient: "0x0d1d9635d0640821d15e323ac8adadfa9c111414".to_string(),
            address_encoding: AddressEncoding::Hex,
            destination_chain_id: 42161,
            gas_limit: 200_000,
            data: Bytes::from_static(&[0xde, 0xad, 0xbe, 0xef]),
            nonce: 1690393044548,
        });

        let typed_data = match &action {
            Action::SendToEvmWithData(inner) => {
                get_typed_data::<solidity::SendToEvmWithData>(inner, Chain::Mainnet, None)
            }
            _ => unreachable!(),
        };
        assert_eq!(
            typed_data.primary_type,
            "HyperliquidTransaction:SendToEvmWithData"
        );
        assert_eq!(
            typed_data.encode_type().unwrap(),
            "HyperliquidTransaction:SendToEvmWithData(string hyperliquidChain,string token,\
             string amount,string sourceDex,string destinationRecipient,string addressEncoding,\
             uint32 destinationChainId,uint64 gasLimit,bytes data,uint64 nonce)"
        );
        // Resolving the hash exercises coercion of every field, `data` included.
        action
            .prehash(1690393044548, None, None, Chain::Mainnet)
            .unwrap();
    }

    #[test]
    fn l1_action_serialization() {
        use rust_decimal::dec;

        assert_eq!(
            serde_json::to_string(&Action::ClaimRewards).unwrap(),
            r#"{"type":"claimRewards"}"#
        );

        assert_eq!(
            serde_json::to_string(&Action::TopUpIsolatedOnlyMargin(TopUpIsolatedOnlyMargin {
                asset: 4,
                leverage: dec!(12.5),
            }))
            .unwrap(),
            r#"{"type":"topUpIsolatedOnlyMargin","asset":4,"leverage":"12.5"}"#
        );

        assert_eq!(
            serde_json::to_string(&Action::AuthorizeAqav2Role(AuthorizeAqav2Role {
                token: 0,
                role: Aqav2Role::Treasury,
            }))
            .unwrap(),
            r#"{"type":"authorizeAqav2Role","token":0,"role":"treasury"}"#
        );

        assert_eq!(
            serde_json::to_string(&Action::ValidatorL1Stream(ValidatorL1Stream {
                risk_free_rate: dec!(0.04),
            }))
            .unwrap(),
            r#"{"type":"validatorL1Stream","riskFreeRate":"0.04"}"#
        );
    }

    /// `f` is omitted entirely when false, which keeps the msgpack signing hash unchanged
    /// for callers that never opt into fast cancels.
    /// The pre-`fast` wire shape, as it existed before the field was added.
    #[derive(serde::Serialize)]
    struct OldBatchCancel {
        cancels: Vec<crate::hypercore::types::Cancel>,
    }

    #[derive(serde::Serialize)]
    #[serde(tag = "type", rename_all = "camelCase")]
    enum OldAction {
        Cancel(OldBatchCancel),
    }

    /// Adding `fast` must not change the msgpack bytes for existing callers, since the
    /// signature is taken over that encoding.
    #[test]
    fn fast_false_is_byte_identical_to_the_old_shape() {
        let cancels = vec![
            crate::hypercore::types::Cancel { asset: 1, oid: 42 },
            crate::hypercore::types::Cancel { asset: 7, oid: 99 },
        ];

        let old = rmp_serde::to_vec_named(&OldAction::Cancel(OldBatchCancel {
            cancels: cancels.clone(),
        }))
        .unwrap();

        let new = rmp_serde::to_vec_named(&Action::Cancel(BatchCancel {
            cancels: cancels.clone(),
            fast: false,
        }))
        .unwrap();

        assert_eq!(old, new, "fast:false changed the signing bytes");

        let fast = rmp_serde::to_vec_named(&Action::Cancel(BatchCancel {
            cancels,
            fast: true,
        }))
        .unwrap();
        assert_ne!(old, fast, "fast:true should change the signing bytes");
    }

    #[test]
    fn cancel_fast_flag_is_omitted_when_false() {
        let cancels = vec![crate::hypercore::types::Cancel { asset: 1, oid: 42 }];

        let slow = Action::Cancel(BatchCancel {
            cancels: cancels.clone(),
            fast: false,
        });
        assert_eq!(
            serde_json::to_string(&slow).unwrap(),
            r#"{"type":"cancel","cancels":[{"a":1,"o":42}]}"#
        );

        let fast = Action::Cancel(BatchCancel {
            cancels,
            fast: true,
        });
        assert_eq!(
            serde_json::to_string(&fast).unwrap(),
            r#"{"type":"cancel","cancels":[{"a":1,"o":42}],"f":true}"#
        );
    }

    /// `destination` is omitted when absent so existing single-argument reservations sign
    /// exactly as they did before the field was added.
    #[test]
    fn reserve_request_weight_destination_is_optional() {
        assert_eq!(
            serde_json::to_string(&Action::ReserveRequestWeight {
                weight: 10,
                destination: None,
            })
            .unwrap(),
            r#"{"type":"reserveRequestWeight","weight":10}"#
        );

        assert_eq!(
            serde_json::to_string(&Action::ReserveRequestWeight {
                weight: 10,
                destination: Some(address!("0x0D1d9635D0640821d15e323ac8AdADfA9c111414")),
            })
            .unwrap(),
            r#"{"type":"reserveRequestWeight","weight":10,"destination":"0x0d1d9635d0640821d15e323ac8adadfa9c111414"}"#
        );
    }

    #[test]
    fn user_outcome_serialization() {
        use rust_decimal::dec;

        // split: exactly one operation present, others omitted.
        let action = Action::UserOutcome(UserOutcomeAction::split(20, dec!(10)));
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(
            json,
            r#"{"type":"userOutcome","splitOutcome":{"outcome":20,"amount":"10"}}"#
        );

        // merge with explicit amount.
        let action = Action::UserOutcome(UserOutcomeAction::merge(20, Some(dec!(5))));
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(
            json,
            r#"{"type":"userOutcome","mergeOutcome":{"outcome":20,"amount":"5"}}"#
        );

        // merge with no amount => max (serialized as null).
        let action = Action::UserOutcome(UserOutcomeAction::merge(20, None));
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(
            json,
            r#"{"type":"userOutcome","mergeOutcome":{"outcome":20,"amount":null}}"#
        );

        // mergeQuestion.
        let action = Action::UserOutcome(UserOutcomeAction::merge_question(3, Some(dec!(2))));
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(
            json,
            r#"{"type":"userOutcome","mergeQuestion":{"question":3,"amount":"2"}}"#
        );

        // negate.
        let action = Action::UserOutcome(UserOutcomeAction::negate(3, 20, dec!(1)));
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(
            json,
            r#"{"type":"userOutcome","negateOutcome":{"question":3,"outcome":20,"amount":"1"}}"#
        );

        // Round-trip.
        let deserialized: Action = serde_json::from_str(&json).unwrap();
        let Action::UserOutcome(a) = deserialized else {
            panic!("expected UserOutcome");
        };
        let neg = a.negate_outcome.unwrap();
        assert_eq!(neg.question, 3);
        assert_eq!(neg.outcome, 20);
        assert_eq!(neg.amount, dec!(1));
    }

    #[test]
    fn vault_transfer_serialization() {
        use alloy::primitives::address;

        let action = Action::VaultTransfer(VaultTransfer {
            vault_address: address!("dfc24b077bc1425ad1dea75bcb6f8158e10df303"),
            is_deposit: true,
            usd: 100_500_000, // 100.5 USDC in micro-units
        });

        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\":\"vaultTransfer\""));
        assert!(json.contains("\"vaultAddress\":\"0xdfc24b077bc1425ad1dea75bcb6f8158e10df303\""));
        assert!(json.contains("\"isDeposit\":true"));
        assert!(json.contains("\"usd\":100500000"));

        // Round-trip
        let deserialized: Action = serde_json::from_str(&json).unwrap();
        if let Action::VaultTransfer(vt) = deserialized {
            assert!(vt.is_deposit);
            assert_eq!(vt.usd, 100_500_000);
        } else {
            assert!(false, "wrong variant");
        }
    }

    #[test]
    fn agent_send_asset_serialization() {
        use rust_decimal::dec;

        let action = Action::AgentSendAsset(AgentSendAssetAction {
            destination: address!("0x5eCb62791B22A3108367c2A2024019Ee7eA88431"),
            source_dex: String::new(),
            destination_dex: "spot".to_string(),
            token: "PURR:0xc4bf3f870c0e9465323c0b6ed28096c2".to_string(),
            amount: dec!(0.01),
            from_sub_account: String::new(),
            nonce: 1_700_000_000_000,
        });

        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\":\"agentSendAsset\""));
        assert!(json.contains("\"destination\":\"0x5ecb62791b22a3108367c2a2024019ee7ea88431\""));
        assert!(json.contains("\"sourceDex\":\"\""));
        assert!(json.contains("\"destinationDex\":\"spot\""));
        assert!(json.contains("\"token\":\"PURR:0xc4bf3f870c0e9465323c0b6ed28096c2\""));
        assert!(json.contains("\"amount\":\"0.01\""));
        assert!(json.contains("\"fromSubAccount\":\"\""));
        assert!(json.contains("\"nonce\":1700000000000"));

        let deserialized: Action = serde_json::from_str(&json).unwrap();
        match deserialized {
            Action::AgentSendAsset(inner) => {
                assert_eq!(inner.source_dex, "");
                assert_eq!(inner.destination_dex, "spot");
                assert_eq!(inner.nonce, 1_700_000_000_000);
                assert_eq!(inner.amount, dec!(0.01));
            }
            _ => assert!(false, "wrong variant"),
        }
    }

    #[test]
    fn update_leverage_serialization() {
        let action = Action::UpdateLeverage(UpdateLeverage {
            asset: 0,
            is_cross: true,
            leverage: 10,
        });

        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\":\"updateLeverage\""));
        assert!(json.contains("\"asset\":0"));
        assert!(json.contains("\"isCross\":true"));
        assert!(json.contains("\"leverage\":10"));

        // Round-trip
        let deserialized: Action = serde_json::from_str(&json).unwrap();
        if let Action::UpdateLeverage(ul) = deserialized {
            assert_eq!(ul.asset, 0);
            assert!(ul.is_cross);
            assert_eq!(ul.leverage, 10);
        } else {
            assert!(false, "wrong variant");
        }
    }

    #[test]
    fn approve_builder_fee_serialization() {
        let action = Action::ApproveBuilderFee(ApproveBuilderFee {
            signature_chain_id: "0xa4b1".to_string(),
            hyperliquid_chain: Chain::Mainnet,
            max_fee_rate: "0.001%".to_string(),
            builder: "0x8c967e73e7b15087c42a10d344cff4c96d877f1d"
                .parse()
                .unwrap(),
            nonce: 1_700_000_000_000,
        });

        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\":\"approveBuilderFee\""));
        assert!(json.contains("\"maxFeeRate\":\"0.001%\""));
        assert!(json.contains("\"builder\":\"0x8c967e73e7b15087c42a10d344cff4c96d877f1d\""));
        assert!(json.contains("\"nonce\":1700000000000"));

        let deserialized: Action = serde_json::from_str(&json).unwrap();
        match deserialized {
            Action::ApproveBuilderFee(inner) => {
                assert_eq!(inner.max_fee_rate, "0.001%");
                assert_eq!(
                    inner.builder,
                    "0x8c967e73e7b15087c42a10d344cff4c96d877f1d"
                        .parse::<Address>()
                        .unwrap()
                );
                assert_eq!(inner.nonce, 1_700_000_000_000);
            }
            _ => assert!(false, "wrong variant"),
        }
    }

    #[test]
    fn agent_set_abstraction_serialization() {
        // Agent-signed action should serialize abstraction as short code
        let action = Action::AgentSetAbstraction {
            abstraction: AbstractionMode::UnifiedAccount,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\":\"agentSetAbstraction\""));
        assert!(json.contains("\"abstraction\":\"u\""));

        // Round-trip through JSON
        let deserialized: Action = serde_json::from_str(&json).unwrap();
        match deserialized {
            Action::AgentSetAbstraction { abstraction } => {
                assert_eq!(abstraction, AbstractionMode::UnifiedAccount);
            }
            _ => assert!(false, "wrong variant"),
        }

        // Test all modes
        for (mode, expected_code) in [
            (AbstractionMode::Standard, "i"),
            (AbstractionMode::UnifiedAccount, "u"),
            (AbstractionMode::PortfolioMargin, "p"),
        ] {
            let action = Action::AgentSetAbstraction { abstraction: mode };
            let json = serde_json::to_string(&action).unwrap();
            assert!(
                json.contains(&format!("\"abstraction\":\"{expected_code}\"")),
                "mode {:?} should serialize to \"{expected_code}\", got: {json}",
                mode
            );
        }
    }

    #[test]
    fn user_set_abstraction_serialization() {
        use alloy::primitives::address;

        // User-signed action should serialize abstraction as full API string
        let action = UserSetAbstractionAction {
            signature_chain_id: "0xa4b1".to_string(),
            hyperliquid_chain: Chain::Mainnet,
            user: address!("0x5eCb62791B22A3108367c2A2024019Ee7eA88431"),
            abstraction: AbstractionMode::PortfolioMargin,
            nonce: 1_700_000_000_000,
        };

        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"abstraction\":\"portfolioMargin\""));
        assert!(json.contains("\"signatureChainId\":\"0xa4b1\""));

        // Standard mode
        let action = UserSetAbstractionAction {
            signature_chain_id: "0xa4b1".to_string(),
            hyperliquid_chain: Chain::Mainnet,
            user: address!("0x5eCb62791B22A3108367c2A2024019Ee7eA88431"),
            abstraction: AbstractionMode::Standard,
            nonce: 1_700_000_000_000,
        };

        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"abstraction\":\"disabled\""));
    }

    #[test]
    fn abstraction_mode_conversions() {
        assert_eq!(AbstractionMode::Standard.api_str(), "disabled");
        assert_eq!(AbstractionMode::UnifiedAccount.api_str(), "unifiedAccount");
        assert_eq!(
            AbstractionMode::PortfolioMargin.api_str(),
            "portfolioMargin"
        );

        assert_eq!(AbstractionMode::Standard.agent_code(), "i");
        assert_eq!(AbstractionMode::UnifiedAccount.agent_code(), "u");
        assert_eq!(AbstractionMode::PortfolioMargin.agent_code(), "p");

        assert_eq!(
            AbstractionMode::from_api_str("disabled").unwrap(),
            AbstractionMode::Standard
        );
        assert_eq!(
            AbstractionMode::from_api_str("i").unwrap(),
            AbstractionMode::Standard
        );
        assert_eq!(
            AbstractionMode::from_api_str("unifiedAccount").unwrap(),
            AbstractionMode::UnifiedAccount
        );
        assert_eq!(
            AbstractionMode::from_api_str("portfolioMargin").unwrap(),
            AbstractionMode::PortfolioMargin
        );
        assert!(AbstractionMode::from_api_str("unknown").is_err());
        assert!(AbstractionMode::default().is_standard());
    }

    #[test]
    fn modify_omits_always_place_when_false() {
        use rust_decimal::dec;

        use crate::hypercore::types::{Cloid, OrderTypePlacement, TimeInForce};

        let order = || OrderRequest {
            asset: 0,
            is_buy: true,
            limit_px: dec!(1),
            sz: dec!(1),
            reduce_only: false,
            order_type: OrderTypePlacement::Limit {
                tif: TimeInForce::Alo,
            },
            cloid: Cloid::default(),
        };

        // The exchange rejects an action hashed with `a: false`, so the field must vanish.
        let action = Action::Modify(ModifyAction {
            oid: either::Either::Left(1),
            order: order(),
            always_place: false,
        });
        let json = serde_json::to_value(&action).unwrap();
        assert_eq!(json["type"], "modify");
        assert!(json.get("a").is_none(), "a must be omitted when false");

        let action = Action::Modify(ModifyAction {
            oid: either::Either::Left(1),
            order: order(),
            always_place: true,
        });
        assert_eq!(serde_json::to_value(&action).unwrap()["a"], true);

        let batch = Action::BatchModify(crate::hypercore::types::BatchModify {
            modifies: vec![],
            always_place: false,
        });
        assert!(
            serde_json::to_value(&batch).unwrap().get("a").is_none(),
            "a must be omitted when false"
        );
    }

    #[test]
    fn undocumented_actions_serialize_as_expected() {
        use rust_decimal::dec;

        let action = Action::BorrowLend(BorrowLendAction {
            operation: BorrowLendOperation::Supply,
            token: 0,
            amount: Some(dec!(1.5)),
        });
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            serde_json::json!({
                "type": "borrowLend", "operation": "supply", "token": 0, "amount": "1.5"
            })
        );

        // `null` amount means "the maximum available", so it must be sent, not skipped.
        let action = Action::BorrowLend(BorrowLendAction {
            operation: BorrowLendOperation::Repay,
            token: 3,
            amount: None,
        });
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            serde_json::json!({
                "type": "borrowLend", "operation": "repay", "token": 3, "amount": null
            })
        );

        // The validator payloads are snake_case, and the node address key is capitalized.
        let action =
            Action::CValidatorAction(CValidatorAction::ChangeProfile(ValidatorProfileChange {
                node_ip: Some(ValidatorNodeIp {
                    ip: "1.2.3.4".to_string(),
                }),
                name: None,
                description: None,
                unjailed: true,
                disable_delegations: None,
                commission_bps: Some(500),
                signer: None,
            }));
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            serde_json::json!({
                "type": "CValidatorAction",
                "changeProfile": {
                    "node_ip": {"Ip": "1.2.3.4"},
                    "name": null,
                    "description": null,
                    "unjailed": true,
                    "disable_delegations": null,
                    "commission_bps": 500,
                    "signer": null
                }
            })
        );

        let action = Action::CSignerAction(CSignerAction::JailSelf(()));
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            serde_json::json!({"type": "CSignerAction", "jailSelf": null})
        );

        let action = Action::FinalizeEvmContract(FinalizeEvmContract {
            token: 1,
            input: FinalizeEvmContractInput::FirstStorageSlot,
        });
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            serde_json::json!({
                "type": "finalizeEvmContract", "token": 1, "input": "firstStorageSlot"
            })
        );
    }

    #[cfg(feature = "client")]
    /// Checks that mainnet still parses every undocumented action shape this module builds.
    ///
    /// None of these are in the gitbook, so a docs diff cannot catch them drifting. Each is
    /// signed with a throwaway key so it cannot take effect; what matters is which error
    /// comes back. "does not exist" means the payload parsed. HTTP 422 "Failed to
    /// deserialize" means the wire format moved and this module needs updating.
    ///
    /// Ignored by default because it hits the network:
    ///
    /// ```bash
    /// cargo test --lib undocumented_action_shapes_are_accepted -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "hits mainnet; run manually when auditing the SDK against the API"]
    async fn undocumented_action_shapes_are_accepted() {
        use alloy::signers::local::PrivateKeySigner;
        use rust_decimal::dec;

        use crate::hypercore;

        let signer = PrivateKeySigner::random();
        let client = hypercore::mainnet();
        let chain = Chain::Mainnet;
        let base = chrono::Utc::now().timestamp_millis() as u64;
        let other = Address::ZERO;

        let user_signed = |nonce: u64| (chain.arbitrum_id().to_owned(), chain, nonce);

        let cases: Vec<(&str, Action)> = vec![
            (
                "modify",
                Action::Modify(ModifyAction {
                    oid: either::Either::Left(1),
                    order: OrderRequest {
                        asset: 0,
                        is_buy: true,
                        limit_px: dec!(1),
                        sz: dec!(1),
                        reduce_only: false,
                        order_type: crate::hypercore::types::OrderTypePlacement::Limit {
                            tif: crate::hypercore::types::TimeInForce::Alo,
                        },
                        cloid: crate::hypercore::types::Cloid::default(),
                    },
                    always_place: true,
                }),
            ),
            (
                "borrowLend",
                Action::BorrowLend(BorrowLendAction {
                    operation: BorrowLendOperation::Supply,
                    token: 0,
                    amount: Some(dec!(1)),
                }),
            ),
            (
                "createSubAccount",
                Action::CreateSubAccount {
                    name: "probe".into(),
                },
            ),
            (
                "subAccountModify",
                Action::SubAccountModify(SubAccountModify {
                    sub_account_user: other,
                    name: "probe".into(),
                }),
            ),
            (
                "subAccountTransfer",
                Action::SubAccountTransfer(SubAccountTransfer {
                    sub_account_user: other,
                    is_deposit: true,
                    usd: 1,
                }),
            ),
            (
                "subAccountSpotTransfer",
                Action::SubAccountSpotTransfer(SubAccountSpotTransfer {
                    sub_account_user: other,
                    is_deposit: true,
                    token: "USDC:0x6d1e7cde53ba9467b783cb7c530ce054".into(),
                    amount: dec!(1),
                }),
            ),
            (
                "createVault",
                Action::CreateVault(CreateVault {
                    name: "probe vault".into(),
                    description: "a probe vault description".into(),
                    initial_usd: 100_000_000,
                    nonce: base,
                }),
            ),
            (
                "vaultModify",
                Action::VaultModify(VaultModify {
                    vault_address: other,
                    allow_deposits: Some(true),
                    always_close_on_withdraw: None,
                }),
            ),
            (
                "vaultDistribute",
                Action::VaultDistribute(VaultDistribute {
                    vault_address: other,
                    usd: 0,
                }),
            ),
            (
                "setDisplayName",
                Action::SetDisplayName {
                    display_name: "probe".into(),
                },
            ),
            (
                "setReferrer",
                Action::SetReferrer {
                    code: "PROBE".into(),
                },
            ),
            (
                "registerReferrer",
                Action::RegisterReferrer {
                    code: "PROBE".into(),
                },
            ),
            (
                "spotUser",
                Action::SpotUser(SpotUserAction {
                    toggle_spot_dusting: ToggleSpotDusting { opt_out: false },
                }),
            ),
            (
                "finalizeEvmContract",
                Action::FinalizeEvmContract(FinalizeEvmContract {
                    token: 1,
                    input: FinalizeEvmContractInput::Create { nonce: 1 },
                }),
            ),
            (
                "CSignerAction/jailSelf",
                Action::CSignerAction(CSignerAction::JailSelf(())),
            ),
            (
                "CValidatorAction/unregister",
                Action::CValidatorAction(CValidatorAction::Unregister(())),
            ),
            ("linkStakingUser", {
                let (signature_chain_id, hyperliquid_chain, nonce) = user_signed(base);
                Action::LinkStakingUser(LinkStakingUserAction {
                    signature_chain_id,
                    hyperliquid_chain,
                    user: other,
                    is_finalize: false,
                    nonce,
                })
            }),
            ("stakingLinkDisableTradingUser", {
                let (signature_chain_id, hyperliquid_chain, nonce) = user_signed(base);
                Action::StakingLinkDisableTradingUser(StakingLinkDisableTradingUserAction {
                    signature_chain_id,
                    hyperliquid_chain,
                    trading_user: other,
                    nonce,
                })
            }),
            ("userPortfolioMargin", {
                let (signature_chain_id, hyperliquid_chain, nonce) = user_signed(base);
                Action::UserPortfolioMargin(UserPortfolioMarginAction {
                    signature_chain_id,
                    hyperliquid_chain,
                    user: other,
                    enabled: true,
                    nonce,
                })
            }),
            (
                "reserveRequestWeight/destination",
                Action::ReserveRequestWeight {
                    weight: 1,
                    destination: Some(other),
                },
            ),
            (
                "CValidatorAction/register",
                Action::CValidatorAction(CValidatorAction::Register(ValidatorRegistration {
                    profile: ValidatorProfile {
                        node_ip: ValidatorNodeIp {
                            ip: "1.2.3.4".into(),
                        },
                        name: "zz".into(),
                        description: "zz".into(),
                        delegations_disabled: false,
                        commission_bps: 100,
                        signer: other,
                    },
                    unjailed: false,
                    initial_wei: 1,
                })),
            ),
            (
                "CValidatorAction/changeProfile",
                Action::CValidatorAction(CValidatorAction::ChangeProfile(ValidatorProfileChange {
                    node_ip: None,
                    name: None,
                    description: None,
                    unjailed: false,
                    disable_delegations: None,
                    commission_bps: None,
                    signer: Some(other),
                })),
            ),
            // Documented, but the hash depends on `activationPx` being sent as `null` when
            // unset, which only the exchange can confirm.
            (
                "trailingStop",
                Action::TrailingStop(TrailingStop {
                    asset: 0,
                    is_buy: false,
                    sz: dec!(0.001),
                    reduce_only: true,
                    retracement: TrailingStopRetracement::Pct(dec!(1.5)),
                    activation_px: None,
                }),
            ),
            (
                "trailingStop with activationPx",
                Action::TrailingStop(TrailingStop {
                    asset: 0,
                    is_buy: false,
                    sz: dec!(0.001),
                    reduce_only: true,
                    retracement: TrailingStopRetracement::Px(dec!(500)),
                    activation_px: Some(dec!(120000)),
                }),
            ),
        ];

        let mut failures = Vec::new();
        for (i, (label, mut action)) in cases.into_iter().enumerate() {
            let nonce = base + i as u64;

            // These four repeat the nonce inside the payload, where it has to match the one
            // the request is sent with.
            match &mut action {
                Action::LinkStakingUser(inner) => inner.nonce = nonce,
                Action::StakingLinkDisableTradingUser(inner) => inner.nonce = nonce,
                Action::UserPortfolioMargin(inner) => inner.nonce = nonce,
                Action::CreateVault(inner) => inner.nonce = nonce,
                _ => {}
            }

            let req = action.sign_sync(&signer, nonce, None, None, chain).unwrap();
            let out = match client.send(req).await {
                Ok(resp) => format!("{resp:?}"),
                Err(err) => format!("ERR {err}"),
            };
            println!(
                "{label:34} => {}",
                out.chars().take(120).collect::<String>()
            );

            if out.contains("Failed to deserialize") {
                failures.push(format!("{label}: {out}"));
            }
            // Parsing is not enough: the signature covers the msgpack encoding, so a field
            // encoded differently from how the exchange hashes it recovers another address.
            if let Some(recovered) = super::recovered_other_signer(&out, signer.address()) {
                failures.push(format!("{label}: signature recovered to {recovered}"));
            }

            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }

        assert!(
            failures.is_empty(),
            "the exchange does not parse these action shapes, or hashes them differently \
             from how they were signed:\n{}",
            failures.join("\n")
        );
    }
}
