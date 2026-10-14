//! Deployer actions for HIP-1/HIP-2 spot tokens, HIP-3 perp DEXes, and HIP-4 outcome markets.
//!
//! These are the actions a *deployer* sends, as opposed to the trading actions in
//! [`super::api`]. All of them are L1 actions: they are signed with the msgpack + `Agent`
//! scheme used by orders and cancels, so an API wallet can send them.
//!
//! Each family is a single exchange action carrying exactly one operation:
//!
//! - [`SpotDeployAction`] is `{"type": "spotDeploy", "<variant>": {...}}`
//! - [`PerpDeployAction`] is `{"type": "perpDeploy", "<variant>": {...}}`
//! - [`OutcomeDeployAction`] is `{"type": "outcomeDeploy", "<variant>": {...}}`
//! - [`ActivateOutcomeDeployer`] is `{"type": "activateOutcomeDeployer", "<variant>": {...}}`
//!
//! # Sorting
//!
//! Every list of tuples must be sorted lexicographically by its first element **before
//! signing**. The exchange rejects unsorted input, and because the signature covers the
//! msgpack encoding, sorting after signing corrupts the request. Helpers that build these
//! lists here sort on construction; if you populate the `Vec` fields directly, sort them
//! yourself.
//!
//! <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/deploying-hip-1-and-hip-2-assets>
//! <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/hip-3-deployer-actions>
//! <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/hip-4-deployer-actions>

use alloy::primitives::Address;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::{BatchCancel, BatchOrder, SubDeployerPermission};

// ========================================================
// HIP-1 / HIP-2 SPOT DEPLOY
// ========================================================

/// A HIP-1/HIP-2 spot deploy action.
///
/// Deploying a token is a five-step sequence: [`RegisterToken2`](Self::RegisterToken2),
/// [`UserGenesis`](Self::UserGenesis), [`Genesis`](Self::Genesis),
/// [`RegisterSpot`](Self::RegisterSpot), then
/// [`RegisterHyperliquidity`](Self::RegisterHyperliquidity). The remaining variants are
/// optional and may be sent later.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/deploying-hip-1-and-hip-2-assets>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum SpotDeployAction {
    /// Step 1: reserve the token and its precision.
    RegisterToken2(RegisterToken2),
    /// Step 2: allocate genesis balances. May be sent multiple times.
    UserGenesis(UserGenesis),
    /// Step 3: fix the max supply, which checksums the preceding genesis calls.
    Genesis(Genesis),
    /// Step 4: create the trading pair.
    RegisterSpot(RegisterSpot),
    /// Step 5: seed hyperliquidity.
    RegisterHyperliquidity(RegisterHyperliquidity),
    /// Lower the deployer's share of trading fees. Cannot be increased.
    SetDeployerTradingFeeShare(SetDeployerTradingFeeShare),
    /// Allow the token to be used as a quote asset.
    EnableQuoteToken(TokenRef),
    /// Stop the token being used as a quote asset.
    DisableQuoteToken(TokenRef),
    /// Set the category, description and keywords shown for the token.
    ///
    /// Can be changed at most once per day.
    SetTokenAnnotation(SetTokenAnnotation),
    /// Set the label displayed on all of the deployer's tokens.
    ///
    /// Can only be set once per deployer, and must be unique across perp DEX names and other
    /// deployer labels.
    SetDeployerLabel(SetDeployerLabel),
    //
    // `enableAlignedQuoteToken` and `disableAlignedQuoteToken` are documented but not
    // implemented: both mainnet and testnet reject them at the JSON parser for every payload
    // shape tried, the same way they reject a nonexistent variant, while the plain
    // `enableQuoteToken`/`disableQuoteToken` above are accepted. The related
    // `alignedQuoteTokenInfo` info request is dead too. Add them once the exchange accepts them.
}

/// A bare token index, used by the quote-token variants of [`SpotDeployAction`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct TokenRef {
    /// Token index.
    pub token: u32,
}

/// Reserve a token name and its precision.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RegisterToken2 {
    /// Name and decimal precision.
    pub spec: TokenSpec,
    /// Max gas in native token wei.
    pub max_gas: u64,
    /// Optional long-form name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
}

/// Name and precision of a new token.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TokenSpec {
    /// Token name.
    pub name: String,
    /// Decimals used for sizes.
    pub sz_decimals: u32,
    /// Decimals used for wei amounts.
    pub wei_decimals: u32,
}

/// Allocate genesis balances for a token.
///
/// May be sent multiple times before [`Genesis`], which checksums the total.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UserGenesis {
    /// Token index.
    pub token: u32,
    /// `(user, wei)` pairs, sorted by user.
    #[serde(serialize_with = "crate::hypercore::utils::serialize_address_pairs_as_hex")]
    pub user_and_wei: Vec<(Address, String)>,
    /// `(existing token, total wei for its holders)` pairs, sorted by token.
    pub existing_token_and_wei: Vec<(u32, String)>,
    /// `(user, is_blacklisted)` pairs, sorted by user.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::hypercore::utils::serialize_option_address_pairs_as_hex"
    )]
    pub blacklist_users: Option<Vec<(Address, bool)>>,
}

/// Fix a token's maximum supply, completing genesis.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Genesis {
    /// Token index.
    pub token: u32,
    /// Total supply. Acts as a checksum over the preceding [`UserGenesis`] calls.
    pub max_supply: String,
    /// Set the hyperliquidity balance to zero.
    ///
    /// When `true`, [`RegisterHyperliquidity::n_orders`] must be `0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_hyperliquidity: Option<bool>,
}

/// Create a spot trading pair.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RegisterSpot {
    /// `[base token index, quote token index]`.
    pub tokens: [u32; 2],
}

/// Seed hyperliquidity for a spot pair.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RegisterHyperliquidity {
    /// Spot index, which differs from the base token index.
    pub spot: u32,
    /// Starting price.
    pub start_px: String,
    /// Size of each order, as a float rather than wei.
    pub order_sz: String,
    /// Number of orders. Must be `0` if genesis set `no_hyperliquidity`.
    pub n_orders: u32,
    /// Levels to seed with USDC instead of tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_seeded_levels: Option<u32>,
}

/// Lower the deployer's share of trading fees.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetDeployerTradingFeeShare {
    /// Token index.
    pub token: u32,
    /// Share in `["0%", "100%"]`, e.g. `"0.012%"` or `"99.4%"`. Never increasing.
    pub share: String,
}

/// Set the annotation displayed for a spot token.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetTokenAnnotation {
    /// Token index.
    pub token: u32,
    /// The annotation to display.
    pub annotation: TokenAnnotation,
}

/// The text shown for a token, carried by [`SetTokenAnnotation`].
///
/// The same shape is used for perps by [`SetPerpAnnotation`], which keys it by coin instead.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TokenAnnotation {
    /// Category, at most 15 characters.
    pub category: String,
    /// Description, at most 400 characters.
    pub description: String,
    /// Short display name, at most 9 characters.
    pub display_name: Option<String>,
    /// At most 2 keywords, each at most 10 characters.
    pub keywords: Vec<String>,
}

/// Set the label shown on all of a deployer's tokens.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetDeployerLabel {
    /// 2 to 4 lowercase characters. Unique across perp DEXes and other deployer labels.
    pub label: String,
}

// ========================================================
// HIP-4 OUTCOME DEPLOY
// ========================================================

/// Register a deployer as an outcome market venue, or retire it.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/hip-4-deployer-actions#activation>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ActivateOutcomeDeployer {
    /// Claim a venue name and become an active outcome deployer.
    Activate(OutcomeVenue),
    /// Permanently retire the deployer. Serializes as `{"deactivate": null}`.
    ///
    /// Requires that 183 days have elapsed and the deployer has no active outcomes. The
    /// venue name stays reserved and the account can never activate again.
    Deactivate(()),
}

/// The venue name claimed by [`ActivateOutcomeDeployer::Activate`].
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeVenue {
    /// 2 to 4 lowercase ASCII letters, unique across all venue and perp DEX names.
    pub venue_name: String,
}

/// A HIP-4 outcome deployer action, sent as
/// `{"type": "outcomeDeploy", "venue": "<venue>", "operation": {"<variant>": {...}}}`.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/hip-4-deployer-actions#action-format>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeDeploy {
    /// The deployer's venue name. A sub-deployer passes the venue of the deployer it acts
    /// for, and settlements can only target that deployer's outcomes and questions.
    pub venue: String,
    /// The deployment or settlement to perform.
    pub operation: OutcomeDeployAction,
}

impl OutcomeDeploy {
    /// Builds an action for the given venue.
    #[must_use]
    pub fn new(venue: impl Into<String>, operation: OutcomeDeployAction) -> Self {
        Self {
            venue: venue.into(),
            operation,
        }
    }
}

/// HIP-4 outcome market deployment and settlement, carried by [`OutcomeDeploy`].
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/hip-4-deployer-actions#action-reference>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum OutcomeDeployAction {
    /// Deploy a single YES/NO market from a standalone outcome template.
    RegisterStandaloneOutcomeFromTemplate(TemplateInstance),
    /// Deploy a question and its outcomes in one action.
    RegisterQuestionFromTemplate(RegisterQuestionFromTemplate),
    /// Add one outcome to a live question.
    RegisterAndAssociateNamedOutcomeFromTemplate(RegisterAndAssociateNamedOutcome),
    /// Settle one outcome.
    SettleOutcome(OutcomeSettlement),
    /// Settle every remaining outcome of a question at once.
    ///
    /// Replaces the discontinued `settleQuestion`.
    SettleQuestion2(SettleQuestion2),
    /// Grant or revoke sub-deployer permissions.
    ///
    /// Each [`SubDeployerInput::variant`] names an [`OutcomeDeployAction`] variant. The
    /// `settleQuestion` grant authorizes [`SettleQuestion2`](Self::SettleQuestion2).
    ///
    /// Unlike the HIP-3 form this carries a bare list, with no DEX name: the venue is the
    /// one in [`OutcomeDeploy::venue`].
    SetSubDeployers(Vec<SubDeployerInput>),
}

/// An instantiation of an outcome template.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInstance {
    /// Template ID.
    pub id: String,
    /// One value per template keyword, sorted by keyword.
    pub keyword_to_value: Vec<(String, String)>,
    /// Deployer fee scale, a decimal in `[0, 10]`.
    #[serde(with = "rust_decimal::serde::str")]
    pub deployer_fee_scale: Decimal,
}

impl TemplateInstance {
    /// Builds an instance, sorting `keyword_to_value` as the exchange requires.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        keyword_to_value: impl IntoIterator<Item = (String, String)>,
        deployer_fee_scale: Decimal,
    ) -> Self {
        let mut keyword_to_value: Vec<_> = keyword_to_value.into_iter().collect();
        keyword_to_value.sort_by(|a, b| a.0.cmp(&b.0));
        Self {
            id: id.into(),
            keyword_to_value,
            deployer_fee_scale,
        }
    }
}

/// An instantiation of a question outcome template.
///
/// Carries no fee scale of its own: it inherits the parent question's.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NamedOutcomeTemplateInstance {
    /// Template ID, which must declare the question's template as its parent.
    pub id: String,
    /// One value per template keyword, sorted by keyword.
    pub keyword_to_value: Vec<(String, String)>,
}

impl NamedOutcomeTemplateInstance {
    /// Builds an instance, sorting `keyword_to_value` as the exchange requires.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        keyword_to_value: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        let mut keyword_to_value: Vec<_> = keyword_to_value.into_iter().collect();
        keyword_to_value.sort_by(|a, b| a.0.cmp(&b.0));
        Self {
            id: id.into(),
            keyword_to_value,
        }
    }
}

/// Deploy a question together with its outcomes.
///
/// Registers N + 1 outcomes: one per entry plus an automatic fallback.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RegisterQuestionFromTemplate {
    /// The question itself, which carries the fee scale for the whole question.
    pub question_template_instance: TemplateInstance,
    /// The question's outcomes. At most 100 per question.
    pub named_outcome_template_instances: Vec<NamedOutcomeTemplateInstance>,
}

/// Add one outcome to a live question.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RegisterAndAssociateNamedOutcome {
    /// Question index.
    pub question: u64,
    /// The outcome to add.
    pub named_outcome_template_instance: NamedOutcomeTemplateInstance,
}

/// Settlement of a single outcome.
///
/// `name_and_description` and `side_names` must match the deployed outcome exactly.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeSettlement {
    /// Outcome index.
    pub outcome: u64,
    /// Fraction paid to the YES side, a decimal in `[0, 1]`.
    ///
    /// Standalone outcomes may settle to any fraction; outcomes belonging to a question
    /// must settle to exactly `0` or `1`.
    #[serde(with = "rust_decimal::serde::str")]
    pub settle_fraction: Decimal,
    /// Must be empty.
    pub details: String,
    /// `[name, description]` of the outcome being settled.
    pub name_and_description: [String; 2],
    /// `[YES side name, NO side name]`.
    pub side_names: [String; 2],
}

/// Settle every remaining outcome of a question.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettleQuestion2 {
    /// Question index.
    pub question: u64,
    /// Exactly the question's remaining active outcomes, with exactly one settling to `1`.
    pub outcome_settlements: Vec<OutcomeSettlement>,
    /// `[name, description]` of the question.
    pub name_and_description: [String; 2],
}

// ========================================================
// HIP-3 PERP DEPLOY
// ========================================================

/// A HIP-3 builder-deployed perp DEX action.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/hip-3-deployer-actions>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum PerpDeployAction {
    /// Register an asset, optionally initializing a new DEX at the same time.
    RegisterAsset2(RegisterAsset2),
    /// Register an asset using the older request shape.
    RegisterAsset(RegisterAsset),
    /// Push oracle, mark, and external prices.
    ///
    /// Must be called at least every 3 seconds, and at most once per 2.5 seconds.
    SetOracle(SetOracle),
    /// Scale the funding rate per asset.
    SetFundingMultipliers(SetFundingMultipliers),
    /// Set the interest rate component of funding per asset.
    SetFundingInterestRates(SetFundingInterestRates),
    /// Halt or resume trading on one coin.
    HaltTrading(HaltTrading),
    /// Assign margin tables to assets.
    SetMarginTableIds(SetMarginTableIds),
    /// Change where the DEX's fees are paid.
    SetFeeRecipient(SetFeeRecipient),
    /// Set per-asset notional open interest caps.
    SetOpenInterestCaps(SetOpenInterestCaps),
    /// Grant or revoke sub-deployer permissions per action variant.
    SetSubDeployers(SetSubDeployers),
    /// Set per-asset margin modes.
    SetMarginModes(SetMarginModes),
    /// Set the deployer's fee scale per asset. Rate limited to one change per 30 days on mainnet.
    SetDeployerFees(SetDeployerFees),
    /// Attach a category, description, and keywords to a coin.
    SetPerpAnnotation(SetPerpAnnotation),
    /// Insert a margin table into the DEX.
    InsertMarginTable(InsertMarginTable),
    /// Permanently disable the DEX.
    DisableDex(String),
    /// Operate a HIP-3\* venue: manage its allowlist and act on its users' behalf.
    ///
    /// HIP-3\* venues are testnet-only.
    Star(Hip3StarAction),
}

/// Margin mode for a HIP-3 asset.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MarginMode {
    /// Isolated only, and isolated margin cannot be withdrawn from open positions.
    StrictIsolated,
    /// Isolated only.
    NoCross,
    /// Cross and isolated both allowed.
    ///
    /// Accepted by [`RegisterAssetRequest2`] but not by [`SetMarginModes`].
    Normal,
}

/// Register an asset, optionally initializing a new DEX.
///
/// If `schema` is omitted the DEX must already exist, and this may be sent repeatedly to
/// add further assets to it.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RegisterAsset2 {
    /// Max gas in native token wei. `None` uses the current deploy auction price.
    ///
    /// `Some(0)` requests a reserve deployment, which succeeds at the current auction price
    /// even after the auction ends. A reserve deployment is consumed whether or not the
    /// auction has completed, so query the auction status first.
    ///
    /// Sent as `null` when `None`: the exchange hashes it that way.
    pub max_gas: Option<u64>,
    /// The asset being listed.
    pub asset_request: RegisterAssetRequest2,
    /// Perp DEX name, 2 to 4 lowercase characters.
    pub dex: String,
    /// New DEX parameters. Present only when creating the DEX; sent as `null` otherwise.
    pub schema: Option<PerpDexSchemaInput>,
}

/// [`RegisterAsset2`] using the older [`RegisterAssetRequest`] shape.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RegisterAsset {
    /// Max gas in native token wei. `None` uses the current deploy auction price.
    ///
    /// Sent as `null` when `None`: the exchange hashes it that way.
    pub max_gas: Option<u64>,
    /// The asset being listed.
    pub asset_request: RegisterAssetRequest,
    /// Perp DEX name, 2 to 4 lowercase characters.
    pub dex: String,
    /// New DEX parameters. Present only when creating the DEX; sent as `null` otherwise.
    pub schema: Option<PerpDexSchemaInput>,
}

/// Listing parameters for a new HIP-3 asset.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RegisterAssetRequest2 {
    /// Coin name.
    pub coin: String,
    /// Decimals used for sizes.
    ///
    /// Size-denominated open interest is capped at 1B per asset, so pick this so the minimal
    /// size increment is worth $1 to $10 at the initial mark price.
    pub sz_decimals: u32,
    /// Initial oracle price.
    pub oracle_px: String,
    /// Margin table ID. Must be non-zero.
    pub margin_table_id: u32,
    /// Margin mode for the asset.
    pub margin_mode: MarginMode,
}

/// Listing parameters for a new HIP-3 asset, older shape.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RegisterAssetRequest {
    /// Coin name.
    pub coin: String,
    /// Decimals used for sizes.
    pub sz_decimals: u32,
    /// Initial oracle price.
    pub oracle_px: String,
    /// Margin table ID. Must be non-zero.
    pub margin_table_id: u32,
    /// Whether the asset is isolated-only.
    pub only_isolated: bool,
}

/// Parameters for a new perp DEX.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PerpDexSchemaInput {
    /// Full name of the perp DEX.
    pub full_name: String,
    /// Collateral token index.
    pub collateral_token: u32,
    /// Address allowed to push oracle updates. `None` means the deployer, and is sent as
    /// `null`.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_option_address_as_hex",
        default
    )]
    pub oracle_updater: Option<Address>,
    /// Create the DEX as a HIP-3\* venue, with an allowlist and proxied user operations.
    /// Only settable when the DEX is created. HIP-3\* venues are testnet-only.
    ///
    /// Omitted from the request (and from the signing hash) when `false`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_star: bool,
}

/// Push oracle, mark, and external prices for a DEX.
///
/// `mark_pxs` may hold 0, 1, or 2 inner lists; their median together with the local mark
/// price becomes the new mark. Prices are clamped to 10x the start-of-day value and mark
/// moves are clamped to 1% per update.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetOracle {
    /// Perp DEX name.
    pub dex: String,
    /// `(asset, oracle price)` pairs, sorted by asset.
    pub oracle_pxs: Vec<(String, String)>,
    /// Lists of `(asset, mark price)` pairs; each inner list sorted by asset.
    pub mark_pxs: Vec<Vec<(String, String)>>,
    /// `(asset, external price)` pairs, sorted by asset. Must cover every asset.
    pub external_perp_pxs: Vec<(String, String)>,
}

/// `(asset, multiplier)` pairs, sorted by asset. Multipliers are in `[0, 10]`.
pub type SetFundingMultipliers = Vec<(String, String)>;

/// `(asset, 8 hour interest rate)` pairs, sorted by asset. Rates are in `[-0.01, 0.01]`.
pub type SetFundingInterestRates = Vec<(String, String)>;

/// `(asset, margin table ID)` pairs, sorted by asset. IDs must be non-zero.
pub type SetMarginTableIds = Vec<(String, u32)>;

/// `(asset, notional cap)` pairs, sorted by asset.
///
/// Caps must be at least the greater of 1,000,000 and half the current open interest.
pub type SetOpenInterestCaps = Vec<(String, u64)>;

/// `(coin, margin mode)` pairs, sorted by coin.
///
/// Only [`MarginMode::StrictIsolated`] and [`MarginMode::NoCross`] are accepted here.
pub type SetMarginModes = Vec<(String, MarginMode)>;

/// `(asset, fee settings)` pairs, sorted by asset.
pub type SetDeployerFees = Vec<(String, DeployerFee)>;

/// Halt or resume trading on one coin.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HaltTrading {
    /// Coin name.
    pub coin: String,
    /// `true` halts trading, `false` resumes it.
    pub is_halted: bool,
}

/// Change where a DEX's fees are paid.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetFeeRecipient {
    /// Perp DEX name.
    pub dex: String,
    /// Address receiving the fees.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub fee_recipient: Address,
}

/// Grant or revoke sub-deployer permissions.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetSubDeployers {
    /// Perp DEX name.
    pub dex: String,
    /// The permission changes to apply.
    pub sub_deployers: Vec<SubDeployerInput>,
}

/// One sub-deployer permission change.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SubDeployerInput {
    /// The permission being delegated: a [`PerpDeployAction`] variant such as `"setOracle"`,
    /// an [`OutcomeDeployAction`] variant such as `"settleOutcome"`, or a HIP-3\* operation
    /// such as [`SubDeployerPermission::Hip3Star`]. Strings convert with `.into()`.
    pub variant: SubDeployerPermission,
    /// The sub-deployer.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub user: Address,
    /// `true` adds the sub-deployer to the authorized set, `false` removes it.
    pub allowed: bool,
}

/// An operation on a HIP-3\* venue.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/hip-3-deployer-actions#hip-3-testnet-only>
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Hip3StarAction {
    /// Perp DEX name.
    pub dex: String,
    /// The operation to apply.
    pub operation: Hip3StarOperation,
}

/// The operations a HIP-3\* venue supports.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum Hip3StarOperation {
    /// Update the venue's oracle prices.
    ///
    /// Needs its own grant, `{"hip3Star": "setOracle"}`: the regular `"setOracle"` grant,
    /// including the one given to the oracle updater, does not cover it.
    SetOracle(Hip3StarSetOracle),
    /// Apply an operation to one user of the venue, sent as `{"proxy": [user, operation]}`.
    Proxy(
        #[serde(
            serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
            deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
        )]
        Address,
        Hip3StarProxyOperation,
    ),
}

/// An operation the deployer, or a sub-deployer granted it, applies to one user of a
/// HIP-3\* venue.
///
/// Each operation needs its own grant: [`SubDeployerPermission::Hip3Star`] with the
/// operation's name, e.g. `"modifyApproval"`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum Hip3StarProxyOperation {
    /// `true` adds the user to the allowlist, `false` removes them and clears their flags.
    /// Both are no-ops when the user is already in that state, so re-approving an approved
    /// user keeps their flags; a removed user who is approved again starts with the defaults.
    ModifyApproval(bool),
    /// `true` lets an approved user deposit into and withdraw from the venue's backstop
    /// liquidator via `hip3LiquidatorTransfer`; `false` revokes it.
    ModifyBackstopLiquidatorApproval(bool),
    /// `true` restricts an approved user to reducing their positions; `false` restores full
    /// trading.
    SetReduceOnly(bool),
    /// Cancel the user's resting orders by oid.
    Cancel(BatchCancel),
    /// Cancel the user's resting orders and TWAPs on the venue.
    CancelAll(Hip3StarCancelAll),
    /// Place orders for the user. Every order must be reduce-only.
    Order(BatchOrder),
    /// Move collateral from the user's account on the venue to another account on it.
    SendAsset(Hip3StarSendAsset),
}

/// Oracle prices for [`Hip3StarOperation::SetOracle`].
///
/// HIP-3\* venues send only spot oracle prices. The exchange derives each asset's external
/// perp price from the main-DEX asset of the same name, and its mark price onchain.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Hip3StarSetOracle {
    /// `(asset, oracle price)` pairs, sorted by asset.
    pub oracle_pxs: Vec<(String, String)>,
}

/// Which assets [`Hip3StarProxyOperation::CancelAll`] cancels on.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Hip3StarCancelAll {
    /// 1 to 10 asset IDs, or `None` for every asset on the venue. Sent as `null` when `None`.
    pub assets: Option<Vec<usize>>,
}

/// Collateral transfer for [`Hip3StarProxyOperation::SendAsset`].
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Hip3StarSendAsset {
    /// Account on the same venue receiving the collateral.
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub destination: Address,
    /// Amount of the venue's collateral token.
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
}

/// The deployer's fee settings for one asset.
///
/// See the fee table in the HIP-3 docs for how `scale` and `growth_mode` combine.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeployerFee {
    /// Decimal in `[0, 3]`, or `[0, 10)` when [`Self::growth_mode`] is set.
    #[serde(with = "rust_decimal::serde::str")]
    pub scale: Decimal,
    /// Whether growth mode applies, which scales the protocol's share down tenfold.
    pub growth_mode: bool,
}

/// Category, description, and keywords for a coin.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetPerpAnnotation {
    /// Coin name.
    pub coin: String,
    /// At most 15 characters.
    pub category: String,
    /// At most 400 characters.
    pub description: String,
    /// At most 9 characters.
    pub display_name: Option<String>,
    /// At most 2 keywords, each at most 10 characters.
    pub keywords: Vec<String>,
}

/// Insert a margin table into a DEX.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InsertMarginTable {
    /// Perp DEX name.
    pub dex: String,
    /// The table to insert.
    pub margin_table: RawMarginTable,
}

/// A margin table: a description plus up to 3 leverage tiers.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RawMarginTable {
    /// Human-readable description.
    pub description: String,
    /// Tiers sorted by increasing `lower_bound` and decreasing `max_leverage`. At most 3.
    pub margin_tiers: Vec<RawMarginTier>,
}

/// One leverage tier of a [`RawMarginTable`].
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RawMarginTier {
    /// Position notional above which [`Self::max_leverage`] applies.
    pub lower_bound: u64,
    /// Max leverage, in `[1, 50]`.
    pub max_leverage: u32,
}

#[cfg(test)]
mod tests {
    use rust_decimal::dec;
    use serde_json::json;

    use super::*;
    use crate::hypercore::api::Action;

    /// The inner variant key has to land alongside `"type"` in one flat object, because
    /// that is the shape the exchange parses and the msgpack hash is taken over.
    #[test]
    fn spot_deploy_flattens_into_the_action_tag() {
        let action = Action::SpotDeploy(SpotDeployAction::RegisterSpot(RegisterSpot {
            tokens: [1, 0],
        }));
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            json!({"type": "spotDeploy", "registerSpot": {"tokens": [1, 0]}})
        );
    }

    #[test]
    fn perp_deploy_flattens_into_the_action_tag() {
        let action = Action::PerpDeploy(PerpDeployAction::HaltTrading(HaltTrading {
            coin: "test:ABC".to_string(),
            is_halted: true,
        }));
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            json!({
                "type": "perpDeploy",
                "haltTrading": {"coin": "test:ABC", "isHalted": true}
            })
        );
    }

    /// `disableDex` carries a bare string rather than an object.
    #[test]
    fn disable_dex_carries_a_bare_string() {
        let action = Action::PerpDeploy(PerpDeployAction::DisableDex("abc".to_string()));
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            json!({"type": "perpDeploy", "disableDex": "abc"})
        );
    }

    /// Deactivation is spelled `{"deactivate": null}`, not a bare `"deactivate"` string.
    #[test]
    fn deactivate_serializes_as_null() {
        let action = Action::ActivateOutcomeDeployer(ActivateOutcomeDeployer::Deactivate(()));
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            json!({"type": "activateOutcomeDeployer", "deactivate": null})
        );

        let action =
            Action::ActivateOutcomeDeployer(ActivateOutcomeDeployer::Activate(OutcomeVenue {
                venue_name: "ab".to_string(),
            }));
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            json!({"type": "activateOutcomeDeployer", "activate": {"venueName": "ab"}})
        );
    }

    /// `outcomeDeploy` is its own action, carrying the venue and the operation, as documented.
    #[test]
    fn outcome_deploy_is_a_top_level_action() {
        let outcome = |operation| Action::OutcomeDeploy(OutcomeDeploy::new("ab", operation));
        let action = outcome(OutcomeDeployAction::RegisterStandaloneOutcomeFromTemplate(
            TemplateInstance::new(
                "abc",
                [
                    ("underlying".to_string(), "ABC".to_string()),
                    ("expiry".to_string(), "20260801-0600".to_string()),
                    ("target".to_string(), "100".to_string()),
                ],
                dec!(1),
            ),
        ));

        // Keywords are sorted on construction, as the exchange requires.
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            json!({
                "type": "outcomeDeploy",
                "venue": "ab",
                "operation": {
                    "registerStandaloneOutcomeFromTemplate": {
                        "id": "abc",
                        "keywordToValue": [
                            ["expiry", "20260801-0600"],
                            ["target", "100"],
                            ["underlying", "ABC"]
                        ],
                        "deployerFeeScale": "1"
                    }
                }
            })
        );
    }

    /// The HIP-4 sub-deployer grant carries a bare list, unlike the HIP-3 one.
    #[test]
    fn outcome_set_sub_deployers_has_no_dex() {
        let action = Action::OutcomeDeploy(OutcomeDeploy::new(
            "ab",
            OutcomeDeployAction::SetSubDeployers(vec![SubDeployerInput {
                variant: "settleOutcome".into(),
                user: Address::ZERO,
                allowed: true,
            }]),
        ));

        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            json!({
                "type": "outcomeDeploy",
                "venue": "ab",
                "operation": {
                    "setSubDeployers": [{
                        "variant": "settleOutcome",
                        "user": "0x0000000000000000000000000000000000000000",
                        "allowed": true
                    }]
                }
            })
        );
    }

    /// The two spot annotation variants that the docs list but the SDK used to omit.
    #[test]
    fn spot_deploy_annotation_variants_serialize() {
        let action = Action::SpotDeploy(SpotDeployAction::SetTokenAnnotation(SetTokenAnnotation {
            token: 7,
            annotation: TokenAnnotation {
                category: "meme".to_string(),
                description: "a token".to_string(),
                display_name: None,
                keywords: vec!["dog".to_string()],
            },
        }));
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            json!({
                "type": "spotDeploy",
                "setTokenAnnotation": {
                    "token": 7,
                    "annotation": {
                        "category": "meme",
                        "description": "a token",
                        "displayName": null,
                        "keywords": ["dog"]
                    }
                }
            })
        );

        let action = Action::SpotDeploy(SpotDeployAction::SetDeployerLabel(SetDeployerLabel {
            label: "abcd".to_string(),
        }));
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            json!({"type": "spotDeploy", "setDeployerLabel": {"label": "abcd"}})
        );
    }

    #[cfg(feature = "client")]
    /// Checks that testnet still parses every deployer action shape this module builds.
    ///
    /// Signs each action with a throwaway key, so none of them can take effect: an account
    /// that does not exist cannot deploy anything. What matters is *which* error comes back.
    /// An authorization error ("does not exist", "Must deposit before performing actions")
    /// means the exchange understood the payload. An HTTP 422 "Failed to deserialize" means
    /// the wire format has drifted and this module needs updating, which is exactly how the
    /// dead `enableAlignedQuoteToken` variant was caught.
    ///
    /// Ignored by default because it hits the network. Run it when the API docs change:
    ///
    /// ```bash
    /// cargo test --lib deployer_action_shapes -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "hits testnet; run manually when auditing the SDK against the API docs"]
    async fn deployer_action_shapes_are_still_accepted() {
        use crate::hypercore::{self, api::Action};
        use alloy::signers::local::PrivateKeySigner;

        let signer = PrivateKeySigner::random();
        let client = hypercore::testnet();
        let base = chrono::Utc::now().timestamp_millis() as u64;
        // Mixed-case hex, so a checksummed or byte encoding cannot pass by accident.
        const USER: Address =
            alloy::primitives::address!("0x5e89b26d8d66da9888c835c9bfcc2aa51813e152");
        let outcome = |operation| Action::OutcomeDeploy(OutcomeDeploy::new("zz", operation));
        let star = |operation| {
            Action::PerpDeploy(PerpDeployAction::Star(Hip3StarAction {
                dex: "zzz".into(),
                operation: Hip3StarOperation::Proxy(USER, operation),
            }))
        };

        let cases: Vec<(&str, Action)> = vec![
            ("claimRewards", Action::ClaimRewards),
            (
                "topUpIsolatedOnlyMargin",
                Action::TopUpIsolatedOnlyMargin(crate::hypercore::api::TopUpIsolatedOnlyMargin {
                    asset: 0,
                    leverage: dec!(5),
                }),
            ),
            (
                "authorizeAqav2Role",
                Action::AuthorizeAqav2Role(crate::hypercore::api::AuthorizeAqav2Role {
                    token: 0,
                    role: crate::hypercore::api::Aqav2Role::Technical,
                }),
            ),
            (
                "validatorL1Stream",
                Action::ValidatorL1Stream(crate::hypercore::api::ValidatorL1Stream {
                    risk_free_rate: dec!(0.04),
                }),
            ),
            (
                "perpDeploy/haltTrading",
                Action::PerpDeploy(PerpDeployAction::HaltTrading(HaltTrading {
                    coin: "zzz:ABC".into(),
                    is_halted: false,
                })),
            ),
            (
                "perpDeploy/setOracle",
                Action::PerpDeploy(PerpDeployAction::SetOracle(SetOracle {
                    dex: "zzz".into(),
                    oracle_pxs: vec![("ABC".into(), "1.0".into())],
                    mark_pxs: vec![vec![("ABC".into(), "1.0".into())]],
                    external_perp_pxs: vec![("ABC".into(), "1.0".into())],
                })),
            ),
            (
                "perpDeploy/setFundingMultipliers",
                Action::PerpDeploy(PerpDeployAction::SetFundingMultipliers(vec![(
                    "ABC".into(),
                    "1.0".into(),
                )])),
            ),
            (
                "perpDeploy/setOpenInterestCaps",
                Action::PerpDeploy(PerpDeployAction::SetOpenInterestCaps(vec![(
                    "ABC".into(),
                    1_000_000u64,
                )])),
            ),
            (
                "perpDeploy/setMarginModes",
                Action::PerpDeploy(PerpDeployAction::SetMarginModes(vec![(
                    "ABC".into(),
                    MarginMode::NoCross,
                )])),
            ),
            (
                "perpDeploy/setDeployerFees",
                Action::PerpDeploy(PerpDeployAction::SetDeployerFees(vec![(
                    "ABC".into(),
                    DeployerFee {
                        scale: dec!(1),
                        growth_mode: false,
                    },
                )])),
            ),
            (
                "perpDeploy/insertMarginTable",
                Action::PerpDeploy(PerpDeployAction::InsertMarginTable(InsertMarginTable {
                    dex: "zzz".into(),
                    margin_table: RawMarginTable {
                        description: "d".into(),
                        margin_tiers: vec![RawMarginTier {
                            lower_bound: 0,
                            max_leverage: 3,
                        }],
                    },
                })),
            ),
            (
                "perpDeploy/setFeeRecipient",
                Action::PerpDeploy(PerpDeployAction::SetFeeRecipient(SetFeeRecipient {
                    dex: "zzz".into(),
                    fee_recipient: USER,
                })),
            ),
            (
                "perpDeploy/setSubDeployers",
                Action::PerpDeploy(PerpDeployAction::SetSubDeployers(SetSubDeployers {
                    dex: "zzz".into(),
                    sub_deployers: vec![SubDeployerInput {
                        variant: "setOracle".into(),
                        user: USER,
                        allowed: true,
                    }],
                })),
            ),
            (
                "perpDeploy/setPerpAnnotation",
                Action::PerpDeploy(PerpDeployAction::SetPerpAnnotation(SetPerpAnnotation {
                    coin: "zzz:ABC".into(),
                    category: "crypto".into(),
                    description: "d".into(),
                    display_name: None,
                    keywords: vec![],
                })),
            ),
            (
                "perpDeploy/registerAsset2",
                Action::PerpDeploy(PerpDeployAction::RegisterAsset2(RegisterAsset2 {
                    max_gas: None,
                    asset_request: RegisterAssetRequest2 {
                        coin: "ABC".into(),
                        sz_decimals: 2,
                        oracle_px: "1.0".into(),
                        margin_table_id: 50,
                        margin_mode: MarginMode::Normal,
                    },
                    dex: "zzz".into(),
                    schema: None,
                })),
            ),
            (
                "perpDeploy/registerAsset2 with schema",
                Action::PerpDeploy(PerpDeployAction::RegisterAsset2(RegisterAsset2 {
                    max_gas: Some(1),
                    asset_request: RegisterAssetRequest2 {
                        coin: "ABC".into(),
                        sz_decimals: 2,
                        oracle_px: "1.0".into(),
                        margin_table_id: 50,
                        margin_mode: MarginMode::Normal,
                    },
                    dex: "zzz".into(),
                    schema: Some(PerpDexSchemaInput {
                        full_name: "zzz".into(),
                        collateral_token: 0,
                        oracle_updater: Some(USER),
                        is_star: false,
                    }),
                })),
            ),
            (
                "perpDeploy/registerAsset",
                Action::PerpDeploy(PerpDeployAction::RegisterAsset(RegisterAsset {
                    max_gas: None,
                    asset_request: RegisterAssetRequest {
                        coin: "ABC".into(),
                        sz_decimals: 2,
                        oracle_px: "1.0".into(),
                        margin_table_id: 50,
                        only_isolated: false,
                    },
                    dex: "zzz".into(),
                    schema: None,
                })),
            ),
            (
                "perpDeploy/disableDex",
                Action::PerpDeploy(PerpDeployAction::DisableDex("zzz".into())),
            ),
            (
                "perpDeploy/registerAsset2 isStar",
                Action::PerpDeploy(PerpDeployAction::RegisterAsset2(RegisterAsset2 {
                    max_gas: None,
                    asset_request: RegisterAssetRequest2 {
                        coin: "ABC".into(),
                        sz_decimals: 2,
                        oracle_px: "1.0".into(),
                        margin_table_id: 50,
                        margin_mode: MarginMode::Normal,
                    },
                    dex: "zzz".into(),
                    schema: Some(PerpDexSchemaInput {
                        full_name: "zzz".into(),
                        collateral_token: 0,
                        oracle_updater: None,
                        is_star: true,
                    }),
                })),
            ),
            (
                "perpDeploy/setSubDeployers hip3Star",
                Action::PerpDeploy(PerpDeployAction::SetSubDeployers(SetSubDeployers {
                    dex: "zzz".into(),
                    sub_deployers: vec![SubDeployerInput {
                        variant: SubDeployerPermission::Hip3Star {
                            action: "order".into(),
                        },
                        user: USER,
                        allowed: true,
                    }],
                })),
            ),
            (
                "perpDeploy/star setOracle",
                Action::PerpDeploy(PerpDeployAction::Star(Hip3StarAction {
                    dex: "zzz".into(),
                    operation: Hip3StarOperation::SetOracle(Hip3StarSetOracle {
                        oracle_pxs: vec![("zzz:BTC".into(), "100000.0".into())],
                    }),
                })),
            ),
            (
                "perpDeploy/star modifyApproval",
                star(Hip3StarProxyOperation::ModifyApproval(true)),
            ),
            (
                "perpDeploy/star modifyBackstopLiquidatorApproval",
                star(Hip3StarProxyOperation::ModifyBackstopLiquidatorApproval(
                    true,
                )),
            ),
            (
                "perpDeploy/star setReduceOnly",
                star(Hip3StarProxyOperation::SetReduceOnly(true)),
            ),
            (
                "perpDeploy/star cancel",
                star(Hip3StarProxyOperation::Cancel(BatchCancel {
                    cancels: vec![super::super::Cancel {
                        asset: 110000,
                        oid: 1,
                    }],
                    fast: false,
                })),
            ),
            (
                "perpDeploy/star cancelAll",
                star(Hip3StarProxyOperation::CancelAll(Hip3StarCancelAll {
                    assets: None,
                })),
            ),
            (
                "perpDeploy/star cancelAll assets",
                star(Hip3StarProxyOperation::CancelAll(Hip3StarCancelAll {
                    assets: Some(vec![110000]),
                })),
            ),
            (
                "perpDeploy/star order",
                star(Hip3StarProxyOperation::Order(BatchOrder {
                    orders: vec![super::super::OrderRequest {
                        asset: 110000,
                        is_buy: false,
                        limit_px: dec!(100),
                        sz: dec!(0.5),
                        reduce_only: true,
                        order_type: super::super::OrderTypePlacement::Limit {
                            tif: super::super::TimeInForce::Ioc,
                        },
                        cloid: Default::default(),
                    }],
                    grouping: super::super::OrderGrouping::Na,
                    builder: None,
                })),
            ),
            (
                "perpDeploy/star sendAsset",
                star(Hip3StarProxyOperation::SendAsset(Hip3StarSendAsset {
                    destination: USER,
                    amount: dec!(100.5),
                })),
            ),
            (
                "spotDeploy/registerToken2",
                Action::SpotDeploy(SpotDeployAction::RegisterToken2(RegisterToken2 {
                    spec: TokenSpec {
                        name: "ZZTEST".into(),
                        sz_decimals: 2,
                        wei_decimals: 8,
                    },
                    max_gas: 1,
                    full_name: None,
                })),
            ),
            (
                "spotDeploy/userGenesis",
                Action::SpotDeploy(SpotDeployAction::UserGenesis(UserGenesis {
                    token: 99999,
                    user_and_wei: vec![(USER, "1".into())],
                    existing_token_and_wei: vec![],
                    blacklist_users: Some(vec![(USER, true)]),
                })),
            ),
            (
                "spotDeploy/genesis",
                Action::SpotDeploy(SpotDeployAction::Genesis(Genesis {
                    token: 99999,
                    max_supply: "1000".into(),
                    no_hyperliquidity: None,
                })),
            ),
            (
                "spotDeploy/registerSpot",
                Action::SpotDeploy(SpotDeployAction::RegisterSpot(RegisterSpot {
                    tokens: [99999, 0],
                })),
            ),
            (
                "spotDeploy/registerHyperliquidity",
                Action::SpotDeploy(SpotDeployAction::RegisterHyperliquidity(
                    RegisterHyperliquidity {
                        spot: 99999,
                        start_px: "1.0".into(),
                        order_sz: "1.0".into(),
                        n_orders: 10,
                        n_seeded_levels: None,
                    },
                )),
            ),
            (
                "spotDeploy/setDeployerTradingFeeShare",
                Action::SpotDeploy(SpotDeployAction::SetDeployerTradingFeeShare(
                    SetDeployerTradingFeeShare {
                        token: 99999,
                        share: "50%".into(),
                    },
                )),
            ),
            (
                "spotDeploy/enableQuoteToken",
                Action::SpotDeploy(SpotDeployAction::EnableQuoteToken(TokenRef {
                    token: 99999,
                })),
            ),
            (
                "spotDeploy/disableQuoteToken",
                Action::SpotDeploy(SpotDeployAction::DisableQuoteToken(TokenRef {
                    token: 99999,
                })),
            ),
            (
                "spotDeploy/setTokenAnnotation",
                Action::SpotDeploy(SpotDeployAction::SetTokenAnnotation(SetTokenAnnotation {
                    token: 99999,
                    annotation: TokenAnnotation {
                        category: "test".into(),
                        description: "test".into(),
                        display_name: None,
                        keywords: vec![],
                    },
                })),
            ),
            (
                "spotDeploy/setDeployerLabel",
                Action::SpotDeploy(SpotDeployAction::SetDeployerLabel(SetDeployerLabel {
                    label: "zzzz".into(),
                })),
            ),
            (
                "outcomeDeploy/registerStandalone",
                outcome(OutcomeDeployAction::RegisterStandaloneOutcomeFromTemplate(
                    TemplateInstance::new(
                        "abc",
                        [("expiry".to_string(), "20260801-0600".to_string())],
                        dec!(1),
                    ),
                )),
            ),
            (
                "outcomeDeploy/registerQuestion",
                outcome(OutcomeDeployAction::RegisterQuestionFromTemplate(
                    RegisterQuestionFromTemplate {
                        question_template_instance: TemplateInstance::new(
                            "abc",
                            [("expiry".to_string(), "20260801-1830".to_string())],
                            dec!(1),
                        ),
                        named_outcome_template_instances: vec![NamedOutcomeTemplateInstance::new(
                            "abc-outcome",
                            [("choice".to_string(), "A".to_string())],
                        )],
                    },
                )),
            ),
            (
                "outcomeDeploy/registerAndAssociate",
                outcome(
                    OutcomeDeployAction::RegisterAndAssociateNamedOutcomeFromTemplate(
                        RegisterAndAssociateNamedOutcome {
                            question: 3,
                            named_outcome_template_instance: NamedOutcomeTemplateInstance::new(
                                "abc-outcome",
                                [("choice".to_string(), "C".to_string())],
                            ),
                        },
                    ),
                ),
            ),
            (
                "outcomeDeploy/settleOutcome",
                outcome(OutcomeDeployAction::SettleOutcome(OutcomeSettlement {
                    outcome: 7,
                    settle_fraction: dec!(1),
                    details: String::new(),
                    name_and_description: ["template:abc".into(), "expiry:20260801-0600".into()],
                    side_names: ["template:Over".into(), "template:Under".into()],
                })),
            ),
            (
                "outcomeDeploy/settleQuestion2",
                outcome(OutcomeDeployAction::SettleQuestion2(SettleQuestion2 {
                    question: 3,
                    outcome_settlements: vec![OutcomeSettlement {
                        outcome: 11,
                        settle_fraction: dec!(1),
                        details: String::new(),
                        name_and_description: ["template:abc-outcome".into(), "choice:A".into()],
                        side_names: ["Yes".into(), "No".into()],
                    }],
                    name_and_description: ["template:abc".into(), "expiry:20260801-1830".into()],
                })),
            ),
            (
                "outcomeDeploy/setSubDeployers",
                outcome(OutcomeDeployAction::SetSubDeployers(vec![
                    SubDeployerInput {
                        variant: "settleOutcome".into(),
                        user: Address::ZERO,
                        allowed: true,
                    },
                ])),
            ),
            (
                "activateOutcomeDeployer/activate",
                Action::ActivateOutcomeDeployer(ActivateOutcomeDeployer::Activate(OutcomeVenue {
                    venue_name: "zz".into(),
                })),
            ),
            (
                "activateOutcomeDeployer/deactivate",
                Action::ActivateOutcomeDeployer(ActivateOutcomeDeployer::Deactivate(())),
            ),
        ];

        let mut failures = Vec::new();
        for (i, (label, action)) in cases.into_iter().enumerate() {
            let nonce = base + i as u64;
            let req = action
                .sign_sync(&signer, nonce, None, None, hypercore::Chain::Testnet)
                .unwrap();
            let out = match client.send(req).await {
                Ok(resp) => format!("{resp:?}"),
                Err(err) => format!("ERR {err}"),
            };
            println!(
                "{label:45} => {}",
                out.chars().take(150).collect::<String>()
            );

            if out.contains("Failed to deserialize") {
                failures.push(format!("{label}: the exchange no longer parses this shape"));
            }
            // Parsing is not enough: the signature covers the msgpack encoding, so a field
            // encoded differently from how the exchange hashes it recovers another address.
            if let Some(recovered) =
                crate::hypercore::api::recovered_other_signer(&out, signer.address())
            {
                failures.push(format!("{label}: signature recovered to {recovered}"));
            }

            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }

        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Sorted tuple lists must survive the round trip through msgpack untouched, since the
    /// signature covers that encoding.
    #[test]
    fn set_oracle_round_trips() {
        let action = Action::PerpDeploy(PerpDeployAction::SetOracle(SetOracle {
            dex: "abc".to_string(),
            oracle_pxs: vec![
                ("AAA".to_string(), "1.5".to_string()),
                ("BBB".to_string(), "2.5".to_string()),
            ],
            mark_pxs: vec![vec![("AAA".to_string(), "1.4".to_string())]],
            external_perp_pxs: vec![("AAA".to_string(), "1.45".to_string())],
        }));

        let bytes = rmp_serde::to_vec_named(&action).unwrap();
        let back: Action = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(
            serde_json::to_value(&back).unwrap(),
            serde_json::to_value(&action).unwrap()
        );
    }

    /// HIP-3* actions serialize as the documented examples.
    #[test]
    fn hip3_star_actions_match_the_documented_shapes() {
        let user: Address = "0x0000000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let star = |operation| {
            serde_json::to_value(Action::PerpDeploy(PerpDeployAction::Star(Hip3StarAction {
                dex: "test".into(),
                operation: Hip3StarOperation::Proxy(user, operation),
            })))
            .unwrap()
        };

        assert_eq!(
            star(Hip3StarProxyOperation::ModifyApproval(true)),
            json!({
                "type": "perpDeploy",
                "star": {
                    "dex": "test",
                    "operation": {"proxy": ["0x0000000000000000000000000000000000000001", {"modifyApproval": true}]}
                }
            })
        );
        assert_eq!(
            star(Hip3StarProxyOperation::CancelAll(Hip3StarCancelAll {
                assets: Some(vec![100001, 100002]),
            }))["star"]["operation"]["proxy"][1],
            json!({"cancelAll": {"assets": [100001, 100002]}})
        );
        assert_eq!(
            star(Hip3StarProxyOperation::CancelAll(
                Hip3StarCancelAll::default()
            ))["star"]["operation"]["proxy"][1],
            json!({"cancelAll": {"assets": null}})
        );
        assert_eq!(
            star(Hip3StarProxyOperation::SendAsset(Hip3StarSendAsset {
                destination: user,
                amount: dec!(100.0),
            }))["star"]["operation"]["proxy"][1],
            json!({"sendAsset": {"destination": "0x0000000000000000000000000000000000000001", "amount": "100.0"}})
        );

        assert_eq!(
            serde_json::to_value(Action::PerpDeploy(PerpDeployAction::Star(Hip3StarAction {
                dex: "test".into(),
                operation: Hip3StarOperation::SetOracle(Hip3StarSetOracle {
                    oracle_pxs: vec![
                        ("test:BTC".into(), "100000.0".into()),
                        ("test:ETH".into(), "4000.0".into()),
                    ],
                }),
            })))
            .unwrap(),
            json!({
                "type": "perpDeploy",
                "star": {
                    "dex": "test",
                    "operation": {"setOracle": {"oraclePxs": [["test:BTC", "100000.0"], ["test:ETH", "4000.0"]]}}
                }
            })
        );

        let grant = Action::PerpDeploy(PerpDeployAction::SetSubDeployers(SetSubDeployers {
            dex: "test".into(),
            sub_deployers: vec![SubDeployerInput {
                variant: SubDeployerPermission::Hip3Star {
                    action: "modifyApproval".into(),
                },
                user,
                allowed: true,
            }],
        }));
        assert_eq!(
            serde_json::to_value(&grant).unwrap()["setSubDeployers"]["subDeployers"][0]["variant"],
            json!({"hip3Star": "modifyApproval"})
        );

        let schema = |is_star| {
            serde_json::to_value(PerpDexSchemaInput {
                full_name: "Test".into(),
                collateral_token: 0,
                oracle_updater: None,
                is_star,
            })
            .unwrap()
        };
        assert_eq!(schema(true)["isStar"], json!(true));
        assert!(schema(false).get("isStar").is_none());
    }

    /// The exchange hashes an unset `maxGas` and `schema` as `null`, as the Python SDK sends
    /// them. Omitting either makes the signature recover to a different address.
    #[test]
    fn unset_register_asset_fields_are_sent_as_null() {
        let action = Action::PerpDeploy(PerpDeployAction::RegisterAsset2(RegisterAsset2 {
            max_gas: None,
            asset_request: RegisterAssetRequest2 {
                coin: "ABC".to_string(),
                sz_decimals: 2,
                oracle_px: "10.0".to_string(),
                margin_table_id: 50,
                margin_mode: MarginMode::Normal,
            },
            dex: "abc".to_string(),
            schema: None,
        }));

        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            json!({
                "type": "perpDeploy",
                "registerAsset2": {
                    "maxGas": null,
                    "assetRequest": {
                        "coin": "ABC",
                        "szDecimals": 2,
                        "oraclePx": "10.0",
                        "marginTableId": 50,
                        "marginMode": "normal"
                    },
                    "dex": "abc",
                    "schema": null
                }
            })
        );
    }
}
