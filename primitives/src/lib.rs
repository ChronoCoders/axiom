use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::{Hash, Hasher};
use subtle::ConstantTimeEq;

// Protocol version supported by this node implementation (network identity).
pub const PROTOCOL_VERSION: u64 = 2;
pub const MAX_TRANSACTIONS_PER_BLOCK: usize = 1000;
pub const MAX_BLOCK_SIZE_BYTES: usize = 1_048_576; // 1 MB

// Protocol v2 constants (compile-time, deterministic, integer-only)
pub const PROTOCOL_V1_VERSION: u64 = 1;
pub const PROTOCOL_V2_VERSION: u64 = 2;
pub const PROTOCOL_VERSION_V1: u64 = PROTOCOL_V1_VERSION;
pub const PROTOCOL_VERSION_V2: u64 = PROTOCOL_V2_VERSION;
pub const V2_ACTIVATION_HEIGHT: u64 = 10_000;
pub const MIN_VALIDATOR_STAKE: u64 = 100_000;
pub const UNBONDING_PERIOD: u64 = 1_000;
pub const SLASH_PERCENTAGE: u64 = 10;
pub const V2_MIGRATION_STAKE_PER_VALIDATOR: u64 = MIN_VALIDATOR_STAKE;

/// Protocol version derived deterministically from block height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolVersion {
    V1,
    V2,
}

impl ProtocolVersion {
    /// Returns the protocol version for a given block height.
    /// height < V2_ACTIVATION_HEIGHT → V1
    /// height >= V2_ACTIVATION_HEIGHT → V2
    pub fn for_height(height: u64) -> Self {
        if height < V2_ACTIVATION_HEIGHT {
            ProtocolVersion::V1
        } else {
            ProtocolVersion::V2
        }
    }

    /// Returns the numeric version identifier.
    pub fn as_u64(&self) -> u64 {
        match self {
            ProtocolVersion::V1 => PROTOCOL_V1_VERSION,
            ProtocolVersion::V2 => PROTOCOL_V2_VERSION,
        }
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolVersion::V1 => write!(f, "v1"),
            ProtocolVersion::V2 => write!(f, "v2"),
        }
    }
}

/// Transaction type discriminator for v2 protocol.
/// Transfer is the only type valid in v1. Stake, Unstake, and SlashEvidence
/// are v2-only and must be rejected below the activation height.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum TransactionType {
    #[default]
    Transfer = 0,
    Stake = 1,
    Unstake = 2,
    SlashEvidence = 3,
}

impl TransactionType {
    /// Returns true if this transaction type is valid under the given protocol version.
    pub fn is_valid_for(&self, version: ProtocolVersion) -> bool {
        match version {
            ProtocolVersion::V1 => matches!(self, TransactionType::Transfer),
            ProtocolVersion::V2 => true,
        }
    }

    /// Converts a u8 tag to a TransactionType, returning None for unknown tags.
    pub fn from_u8(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(TransactionType::Transfer),
            1 => Some(TransactionType::Stake),
            2 => Some(TransactionType::Unstake),
            3 => Some(TransactionType::SlashEvidence),
            _ => None,
        }
    }
}

impl fmt::Display for TransactionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransactionType::Transfer => write!(f, "Transfer"),
            TransactionType::Stake => write!(f, "Stake"),
            TransactionType::Unstake => write!(f, "Unstake"),
            TransactionType::SlashEvidence => write!(f, "SlashEvidence"),
        }
    }
}

/// Staked amount in AXM (u64 wrapper for type safety in v2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StakeAmount(pub u64);

/// A single staking entry for a validator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StakeEntry {
    pub validator_id: ValidatorId,
    pub amount: StakeAmount,
}

/// An entry in the unbonding queue (validator unstaking funds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnbondingEntry {
    pub validator_id: ValidatorId,
    pub amount: StakeAmount,
    pub release_height: u64,
}

/// Round number within a height (v2 consensus scaffolding).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Round(pub u64);

impl fmt::Display for Round {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Vote type in round-based BFT consensus (v2 scaffolding).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum VotePhase {
    Prevote,
    Precommit,
}

pub type VoteType = VotePhase;

// Core identifiers

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId(pub [u8; 32]); // Ed25519 public key

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValidatorId(pub [u8; 32]); // Same as AccountId

#[derive(Clone, Copy, PartialOrd, Ord)]
pub struct BlockHash(pub [u8; 32]); // SHA-256 output

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockState {
    pub height: u64,
    pub round: u64,
    pub block_hash: Option<BlockHash>,
}

#[derive(Clone, Copy, PartialOrd, Ord)]
pub struct StateHash(pub [u8; 32]); // SHA-256 output

#[derive(Clone, Copy, PartialOrd, Ord)]
pub struct TransactionHash(pub [u8; 32]); // SHA-256 output

#[derive(Clone, Copy, PartialOrd, Ord)]
pub struct Signature(pub [u8; 64]); // Ed25519 signature

#[derive(Clone, Copy, PartialOrd, Ord)]
pub struct PublicKey(pub [u8; 32]); // Ed25519 public key

impl PartialEq for BlockHash {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}
impl Eq for BlockHash {}
impl Hash for BlockHash {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialEq for StateHash {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}
impl Eq for StateHash {}
impl Hash for StateHash {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialEq for TransactionHash {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}
impl Eq for TransactionHash {}
impl Hash for TransactionHash {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}
impl Eq for Signature {}
impl Hash for Signature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialEq for PublicKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}
impl Eq for PublicKey {}
impl Hash for PublicKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

// Display implementation for hex encoding
impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_hex(&self.0))
    }
}
impl fmt::Debug for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AccountId({})", to_hex(&self.0))
    }
}

impl fmt::Display for ValidatorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_hex(&self.0))
    }
}
impl fmt::Debug for ValidatorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ValidatorId({})", to_hex(&self.0))
    }
}

impl fmt::Display for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_hex(&self.0))
    }
}
impl fmt::Debug for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlockHash({})", to_hex(&self.0))
    }
}

impl fmt::Display for StateHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_hex(&self.0))
    }
}
impl fmt::Debug for StateHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StateHash({})", to_hex(&self.0))
    }
}

impl fmt::Display for TransactionHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_hex(&self.0))
    }
}
impl fmt::Debug for TransactionHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TransactionHash({})", to_hex(&self.0))
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_hex(&self.0))
    }
}
impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({})", to_hex(&self.0))
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_hex(&self.0))
    }
}
impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", to_hex(&self.0))
    }
}

// Implement conversion from ValidatorId to PublicKey
impl ValidatorId {
    pub fn as_public_key(&self) -> PublicKey {
        PublicKey(self.0)
    }
}

// Implement Serde for core identifiers (as hex strings)
impl Serialize for AccountId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&to_hex(&self.0))
    }
}

impl<'de> Deserialize<'de> for AccountId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = from_hex(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("invalid length for AccountId"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(AccountId(arr))
    }
}

impl Serialize for ValidatorId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&to_hex(&self.0))
    }
}

impl<'de> Deserialize<'de> for ValidatorId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = from_hex(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("invalid length for ValidatorId"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(ValidatorId(arr))
    }
}

impl Serialize for BlockHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&to_hex(&self.0))
    }
}

impl<'de> Deserialize<'de> for BlockHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = from_hex(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("invalid length for BlockHash"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(BlockHash(arr))
    }
}

impl Serialize for StateHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&to_hex(&self.0))
    }
}

impl<'de> Deserialize<'de> for StateHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = from_hex(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("invalid length for StateHash"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(StateHash(arr))
    }
}

impl Serialize for TransactionHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&to_hex(&self.0))
    }
}

impl<'de> Deserialize<'de> for TransactionHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = from_hex(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom(
                "invalid length for TransactionHash",
            ));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(TransactionHash(arr))
    }
}

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&to_hex(&self.0))
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = from_hex(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 64 {
            return Err(serde::de::Error::custom("invalid length for Signature"));
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&bytes);
        Ok(Signature(arr))
    }
}

impl Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&to_hex(&self.0))
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = from_hex(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("invalid length for PublicKey"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(PublicKey(arr))
    }
}

// Block structure
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub parent_hash: BlockHash,
    pub height: u64,
    pub epoch: u64,
    #[serde(default = "default_protocol_version")]
    pub protocol_version: u64,
    #[serde(default)]
    pub round: u64,
    pub proposer_id: ValidatorId,
    pub transactions: Vec<Transaction>,
    pub signatures: Vec<ValidatorSignature>,
    pub state_hash: StateHash,
    /// Unix timestamp (seconds since epoch) when the block was created.
    /// Not included in canonical serialization — treated as metadata only.
    #[serde(default)]
    pub timestamp: u64,
}

fn default_protocol_version() -> u64 {
    PROTOCOL_VERSION_V1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatorSignature {
    pub validator_id: ValidatorId,
    pub signature: Signature,
}

// Transaction structure
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub sender: AccountId,
    pub recipient: AccountId,
    pub amount: u64,
    pub nonce: u64,
    pub signature: Signature,
    #[serde(default)]
    pub tx_type: TransactionType,
    #[serde(default)]
    pub evidence: Option<Evidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vote {
    pub height: u64,
    pub round: u64,
    pub phase: VotePhase,
    pub block_hash: Option<BlockHash>,
    pub validator_id: ValidatorId,
    pub signature: Signature,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub height: u64,
    pub round: u64,
    pub block: Block,
    pub proposer_id: ValidatorId,
    pub signature: Signature,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Evidence {
    DoublePropose {
        proposal_a: Box<Proposal>,
        proposal_b: Box<Proposal>,
    },
    DoubleVote {
        vote_a: Box<Vote>,
        vote_b: Box<Vote>,
    },
}

// Genesis structure (for JSON deserialization)
// Note: Fields are ordered alphabetically to ensure deterministic JSON if serialized by field order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub accounts: Vec<GenesisAccount>,
    pub block_reward: u64,
    pub total_supply: u64,
    pub validators: Vec<GenesisValidator>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisAccount {
    pub balance: u64,
    pub id: AccountId,
    pub nonce: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisValidator {
    pub account_id: AccountId,
    pub active: bool,
    pub id: ValidatorId,
    pub voting_power: u64,
}

// Error type for primitives
#[derive(Debug)]
pub enum PrimitivesError {
    InvalidHex,
    SerializationError,
}

impl fmt::Display for PrimitivesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrimitivesError::InvalidHex => write!(f, "Invalid hex string"),
            PrimitivesError::SerializationError => write!(f, "Serialization error"),
        }
    }
}

// Serialization Functions

pub fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub fn from_hex(hex: &str) -> Result<Vec<u8>, PrimitivesError> {
    if !hex.len().is_multiple_of(2) {
        return Err(PrimitivesError::InvalidHex);
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte_str = &hex[i..i + 2];
        let byte = u8::from_str_radix(byte_str, 16).map_err(|_| PrimitivesError::InvalidHex)?;
        bytes.push(byte);
    }
    Ok(bytes)
}

pub fn serialize_u64(v: u64, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&v.to_be_bytes());
}

pub fn serialize_string(s: &str, buf: &mut Vec<u8>) {
    let len = s.len() as u32; // Assuming string fits in u32
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(s.as_bytes());
}

pub fn serialize_block_canonical(block: &Block) -> Vec<u8> {
    let mut buf = Vec::new();
    // 1. parent_hash (32 bytes raw)
    buf.extend_from_slice(&block.parent_hash.0);
    // 2. height (u64 big-endian)
    serialize_u64(block.height, &mut buf);
    // 3. epoch (u64 big-endian)
    serialize_u64(block.epoch, &mut buf);
    if block.protocol_version == PROTOCOL_VERSION_V2 {
        // v2 block header extension
        serialize_u64(block.protocol_version, &mut buf);
        serialize_u64(block.round, &mut buf);
    }
    // 4. proposer_id (length-prefixed hex string)
    serialize_string(&to_hex(&block.proposer_id.0), &mut buf);
    // 5. transactions (count-prefixed list of canonical transactions)
    let tx_count = block.transactions.len() as u32;
    buf.extend_from_slice(&tx_count.to_be_bytes());
    for tx in &block.transactions {
        if block.protocol_version == PROTOCOL_VERSION_V2 {
            buf.extend(serialize_transaction_canonical_v2(tx));
        } else {
            buf.extend(serialize_transaction_canonical_v1(tx));
        }
    }
    // 6. state_hash (32 bytes raw)
    buf.extend_from_slice(&block.state_hash.0);

    buf
}

pub fn serialize_transaction_canonical(tx: &Transaction) -> Vec<u8> {
    serialize_transaction_canonical_v1(tx)
}

pub fn serialize_transaction_canonical_v1(tx: &Transaction) -> Vec<u8> {
    let mut buf = Vec::new();
    // 1. sender (length-prefixed hex string)
    serialize_string(&to_hex(&tx.sender.0), &mut buf);
    // 2. recipient (length-prefixed hex string)
    serialize_string(&to_hex(&tx.recipient.0), &mut buf);
    // 3. amount (u64 big-endian)
    serialize_u64(tx.amount, &mut buf);
    // 4. nonce (u64 big-endian)
    serialize_u64(tx.nonce, &mut buf);
    // Note: Signature is EXCLUDED from canonical serialization for hashing
    buf
}

pub fn serialize_transaction_canonical_v2(tx: &Transaction) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(tx.tx_type as u8);
    serialize_string(&to_hex(&tx.sender.0), &mut buf);
    serialize_string(&to_hex(&tx.recipient.0), &mut buf);
    serialize_u64(tx.amount, &mut buf);
    serialize_u64(tx.nonce, &mut buf);
    if tx.tx_type == TransactionType::SlashEvidence {
        if let Some(evidence) = tx.evidence.as_ref() {
            let ev_bytes = serialize_evidence_canonical(evidence);
            let len = ev_bytes.len() as u32;
            buf.extend_from_slice(&len.to_be_bytes());
            buf.extend_from_slice(&ev_bytes);
        } else {
            buf.extend_from_slice(&0u32.to_be_bytes());
        }
    }
    buf
}

pub fn serialize_evidence_canonical(evidence: &Evidence) -> Vec<u8> {
    let mut buf = Vec::new();
    match evidence {
        Evidence::DoublePropose {
            proposal_a,
            proposal_b,
        } => {
            buf.push(0);
            let a = serialize_signed_proposal_canonical(proposal_a);
            let b = serialize_signed_proposal_canonical(proposal_b);
            let (first, second) = if a <= b { (a, b) } else { (b, a) };
            let len1 = first.len() as u32;
            buf.extend_from_slice(&len1.to_be_bytes());
            buf.extend_from_slice(&first);
            let len2 = second.len() as u32;
            buf.extend_from_slice(&len2.to_be_bytes());
            buf.extend_from_slice(&second);
        }
        Evidence::DoubleVote { vote_a, vote_b } => {
            buf.push(1);
            let a = serialize_signed_vote_canonical(vote_a);
            let b = serialize_signed_vote_canonical(vote_b);
            let (first, second) = if a <= b { (a, b) } else { (b, a) };
            let len1 = first.len() as u32;
            buf.extend_from_slice(&len1.to_be_bytes());
            buf.extend_from_slice(&first);
            let len2 = second.len() as u32;
            buf.extend_from_slice(&len2.to_be_bytes());
            buf.extend_from_slice(&second);
        }
    }
    buf
}

fn serialize_signed_vote_canonical(vote: &Vote) -> Vec<u8> {
    let mut buf = serialize_vote_canonical(vote);
    buf.extend_from_slice(&vote.signature.0);
    buf
}

fn serialize_signed_proposal_canonical(proposal: &Proposal) -> Vec<u8> {
    let mut buf = serialize_proposal_canonical(proposal);
    buf.extend_from_slice(&proposal.signature.0);
    buf
}

pub fn serialize_vote_canonical(vote: &Vote) -> Vec<u8> {
    let mut buf = Vec::new();
    serialize_u64(vote.height, &mut buf);
    serialize_u64(vote.round, &mut buf);
    buf.push(vote.phase as u8);
    match vote.block_hash {
        Some(h) => {
            buf.push(1);
            buf.extend_from_slice(&h.0);
        }
        None => {
            buf.push(0);
        }
    }
    buf.extend_from_slice(&vote.validator_id.0);
    buf
}

pub fn serialize_proposal_canonical(proposal: &Proposal) -> Vec<u8> {
    let mut buf = Vec::new();
    serialize_u64(proposal.height, &mut buf);
    serialize_u64(proposal.round, &mut buf);
    buf.extend_from_slice(&proposal.proposer_id.0);
    buf.extend_from_slice(&serialize_block_canonical(&proposal.block));
    buf
}

pub fn serialize_genesis_json(genesis: &GenesisConfig) -> Result<String, PrimitivesError> {
    serde_json::to_string(genesis).map_err(|_| PrimitivesError::SerializationError)
}

pub fn deserialize_genesis_json(json: &str) -> Result<GenesisConfig, PrimitivesError> {
    serde_json::from_str(json).map_err(|_| PrimitivesError::SerializationError)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1. AccountId / ValidatorId
    #[test]
    fn test_account_id_display() {
        let bytes = [0xaa; 32];
        let id = AccountId(bytes);
        let s = format!("{id}");
        assert_eq!(s.len(), 64);
        assert_eq!(
            s,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
    }

    #[test]
    fn test_validator_id_display() {
        let bytes = [0xbb; 32];
        let id = ValidatorId(bytes);
        let s = format!("{id}");
        assert_eq!(s.len(), 64);
        assert_eq!(
            s,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
    }

    // 2. BlockHash / StateHash / TransactionHash
    #[test]
    fn test_block_hash_display() {
        let bytes = [0x11; 32];
        let h = BlockHash(bytes);
        let s = format!("{h}");
        assert_eq!(s.len(), 64);
        assert_eq!(
            s,
            "1111111111111111111111111111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn test_state_hash_display() {
        let bytes = [0x22; 32];
        let h = StateHash(bytes);
        let s = format!("{h}");
        assert_eq!(s.len(), 64);
        assert_eq!(
            s,
            "2222222222222222222222222222222222222222222222222222222222222222"
        );
    }

    #[test]
    fn test_transaction_hash_display() {
        let bytes = [0x33; 32];
        let h = TransactionHash(bytes);
        let s = format!("{h}");
        assert_eq!(s.len(), 64);
        assert_eq!(
            s,
            "3333333333333333333333333333333333333333333333333333333333333333"
        );
    }

    // 3. Signature
    #[test]
    fn test_signature_display() {
        let bytes = [0xcc; 64];
        let sig = Signature(bytes);
        let s = format!("{sig}");
        assert_eq!(s.len(), 128);
        assert!(s.chars().all(|c| c == 'c'));
    }

    // 4. from_hex
    #[test]
    fn test_from_hex_valid() {
        let res = from_hex("deadbeef");
        assert_eq!(res.unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn test_from_hex_odd_length() {
        assert!(matches!(from_hex("abc"), Err(PrimitivesError::InvalidHex)));
    }

    #[test]
    fn test_from_hex_invalid_char() {
        assert!(matches!(from_hex("gg"), Err(PrimitivesError::InvalidHex)));
    }

    #[test]
    fn test_from_hex_empty() {
        let res = from_hex("");
        assert_eq!(res.unwrap(), Vec::<u8>::new());
    }

    // 5. to_hex
    #[test]
    fn test_to_hex_empty() {
        assert_eq!(to_hex(&[]), "");
    }

    #[test]
    fn test_to_hex_known() {
        assert_eq!(to_hex(&[0x01, 0x02, 0xff]), "0102ff");
    }

    // 6. serialize_block_canonical
    #[test]
    fn test_serialize_block_canonical_exact_length() {
        let block = Block {
            parent_hash: BlockHash([0u8; 32]),
            height: 1,
            epoch: 2,
            protocol_version: PROTOCOL_VERSION_V1,
            round: 0,
            proposer_id: ValidatorId([0xaa; 32]),
            transactions: vec![],
            signatures: vec![],
            state_hash: StateHash([0xbb; 32]),
            timestamp: 0,
        };
        let bytes = serialize_block_canonical(&block);

        // Expected:
        // parent_hash: 32 bytes
        // height: 8 bytes
        // epoch: 8 bytes
        // proposer_id: 4 (len) + 64 (hex string) = 68 bytes
        // tx_count: 4 bytes
        // state_hash: 32 bytes
        // Total: 32 + 8 + 8 + 68 + 4 + 32 = 152 bytes
        assert_eq!(bytes.len(), 152);
    }

    #[test]
    fn test_serialize_block_determinism() {
        let block = Block {
            parent_hash: BlockHash([0x01; 32]),
            height: 100,
            epoch: 5,
            protocol_version: PROTOCOL_VERSION_V1,
            round: 0,
            proposer_id: ValidatorId([0x02; 32]),
            transactions: vec![],
            signatures: vec![],
            state_hash: StateHash([0x03; 32]),
            timestamp: 0,
        };
        let b1 = serialize_block_canonical(&block);
        let b2 = serialize_block_canonical(&block);
        assert_eq!(b1, b2);
    }

    // 7. serialize_transaction_canonical
    #[test]
    fn test_serialize_transaction_canonical_exact_length() {
        let tx = Transaction {
            sender: AccountId([0x11; 32]),
            recipient: AccountId([0x22; 32]),
            amount: 500,
            nonce: 10,
            signature: Signature([0x33; 64]),
            tx_type: TransactionType::Transfer,
            evidence: None,
        };
        let bytes = serialize_transaction_canonical(&tx);

        // Expected:
        // sender: 4 + 64 = 68 bytes
        // recipient: 4 + 64 = 68 bytes
        // amount: 8 bytes
        // nonce: 8 bytes
        // Total: 68 + 68 + 8 + 8 = 152 bytes
        assert_eq!(bytes.len(), 152);
    }

    #[test]
    fn test_serialize_transaction_determinism() {
        let tx = Transaction {
            sender: AccountId([0x11; 32]),
            recipient: AccountId([0x22; 32]),
            amount: 500,
            nonce: 10,
            signature: Signature([0x33; 64]),
            tx_type: TransactionType::Transfer,
            evidence: None,
        };
        let b1 = serialize_transaction_canonical(&tx);
        let b2 = serialize_transaction_canonical(&tx);
        assert_eq!(b1, b2);
    }

    // 8. serialize_genesis_json
    #[test]
    fn test_serialize_genesis_json_format() {
        let genesis = GenesisConfig {
            accounts: vec![],
            block_reward: 10,
            total_supply: 1000,
            validators: vec![],
        };
        let json = serialize_genesis_json(&genesis).unwrap();

        // Check sorting: "accounts" < "block_reward" < "total_supply" < "validators"
        let idx_acc = json.find("accounts").unwrap();
        let idx_br = json.find("block_reward").unwrap();
        let idx_ts = json.find("total_supply").unwrap();
        let idx_val = json.find("validators").unwrap();

        assert!(idx_acc < idx_br);
        assert!(idx_br < idx_ts);
        assert!(idx_ts < idx_val);

        // Check no whitespace
        assert!(!json.contains(" "));
        assert!(!json.contains("\n"));
    }

    #[test]
    fn test_serialize_genesis_json_roundtrip() {
        let genesis = GenesisConfig {
            accounts: vec![GenesisAccount {
                id: AccountId([1u8; 32]),
                balance: 100,
                nonce: 1,
            }],
            block_reward: 10,
            total_supply: 1000,
            validators: vec![],
        };
        let json = serialize_genesis_json(&genesis).unwrap();
        let deserialized = deserialize_genesis_json(&json).unwrap();
        assert_eq!(genesis, deserialized);
    }

    // 9. deserialize_genesis_json
    #[test]
    fn test_deserialize_genesis_json_invalid_json() {
        assert!(matches!(
            deserialize_genesis_json("{invalid"),
            Err(PrimitivesError::SerializationError)
        ));
    }

    #[test]
    fn test_deserialize_genesis_json_missing_fields() {
        assert!(matches!(
            deserialize_genesis_json("{}"),
            Err(PrimitivesError::SerializationError)
        ));
    }

    #[test]
    fn test_deserialize_genesis_json_wrong_types() {
        // total_supply should be u64 (number), not string
        let json = r#"{"accounts":[],"block_reward":10,"total_supply":"1000","validators":[]}"#;
        assert!(matches!(
            deserialize_genesis_json(json),
            Err(PrimitivesError::SerializationError)
        ));
    }

    // 10. GenesisConfig sorting (manual verification of test logic, as Vec order is preserved)
    // Array order in serialized JSON matches input Vec order; callers must sort before constructing GenesisConfig.

    #[test]
    fn test_genesis_config_array_order_preserved() {
        let acc1 = GenesisAccount {
            id: AccountId([1u8; 32]),
            balance: 0,
            nonce: 0,
        };
        let acc2 = GenesisAccount {
            id: AccountId([2u8; 32]),
            balance: 0,
            nonce: 0,
        };

        let genesis = GenesisConfig {
            accounts: vec![acc2.clone(), acc1.clone()], // Unsorted input
            block_reward: 0,
            total_supply: 0,
            validators: vec![],
        };

        let json = serialize_genesis_json(&genesis).unwrap();
        // We expect order to be preserved: acc2 then acc1
        let idx1 = json.find(&to_hex(&acc1.id.0)).unwrap();
        let idx2 = json.find(&to_hex(&acc2.id.0)).unwrap();

        // Since input was [acc2, acc1], idx2 should appear before idx1
        assert!(idx2 < idx1);
    }

    // 11. Protocol constants
    #[test]
    fn test_protocol_constants() {
        assert_eq!(PROTOCOL_VERSION, 2);
        assert_eq!(MAX_TRANSACTIONS_PER_BLOCK, 1000);
        assert_eq!(MAX_BLOCK_SIZE_BYTES, 1_048_576);
    }

    #[test]
    fn test_validator_id_hex() {
        let bytes = [1u8; 32];
        let val_id = ValidatorId(bytes);
        assert_eq!(
            val_id.to_string(),
            "0101010101010101010101010101010101010101010101010101010101010101"
        );

        let json = serde_json::to_string(&val_id).unwrap();
        assert_eq!(
            json,
            "\"0101010101010101010101010101010101010101010101010101010101010101\""
        );
    }

    #[test]
    fn test_block_proposer_persistence() {
        let val_id = ValidatorId([0xAA; 32]);
        let block = Block {
            parent_hash: BlockHash([0; 32]),
            height: 10,
            epoch: 0,
            protocol_version: PROTOCOL_VERSION_V1,
            round: 0,
            proposer_id: val_id,
            transactions: vec![],
            signatures: vec![],
            state_hash: StateHash([0; 32]),
            timestamp: 0,
        };

        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));

        let deserialized: Block = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.proposer_id, val_id);
    }

    // v2 scaffolding tests

    #[test]
    fn test_protocol_version_for_height_v1() {
        assert_eq!(ProtocolVersion::for_height(0), ProtocolVersion::V1);
        assert_eq!(ProtocolVersion::for_height(1), ProtocolVersion::V1);
        assert_eq!(ProtocolVersion::for_height(9_999), ProtocolVersion::V1);
    }

    #[test]
    fn test_protocol_version_for_height_v2() {
        assert_eq!(ProtocolVersion::for_height(10_000), ProtocolVersion::V2);
        assert_eq!(ProtocolVersion::for_height(10_001), ProtocolVersion::V2);
        assert_eq!(ProtocolVersion::for_height(u64::MAX), ProtocolVersion::V2);
    }

    #[test]
    fn test_protocol_version_as_u64() {
        assert_eq!(ProtocolVersion::V1.as_u64(), 1);
        assert_eq!(ProtocolVersion::V2.as_u64(), 2);
    }

    #[test]
    fn test_protocol_version_display() {
        assert_eq!(format!("{}", ProtocolVersion::V1), "v1");
        assert_eq!(format!("{}", ProtocolVersion::V2), "v2");
    }

    #[test]
    fn test_transaction_type_valid_for_v1() {
        assert!(TransactionType::Transfer.is_valid_for(ProtocolVersion::V1));
        assert!(!TransactionType::Stake.is_valid_for(ProtocolVersion::V1));
        assert!(!TransactionType::Unstake.is_valid_for(ProtocolVersion::V1));
        assert!(!TransactionType::SlashEvidence.is_valid_for(ProtocolVersion::V1));
    }

    #[test]
    fn test_transaction_type_valid_for_v2() {
        assert!(TransactionType::Transfer.is_valid_for(ProtocolVersion::V2));
        assert!(TransactionType::Stake.is_valid_for(ProtocolVersion::V2));
        assert!(TransactionType::Unstake.is_valid_for(ProtocolVersion::V2));
        assert!(TransactionType::SlashEvidence.is_valid_for(ProtocolVersion::V2));
    }

    #[test]
    fn test_transaction_type_from_u8() {
        assert_eq!(TransactionType::from_u8(0), Some(TransactionType::Transfer));
        assert_eq!(TransactionType::from_u8(1), Some(TransactionType::Stake));
        assert_eq!(TransactionType::from_u8(2), Some(TransactionType::Unstake));
        assert_eq!(
            TransactionType::from_u8(3),
            Some(TransactionType::SlashEvidence)
        );
        assert_eq!(TransactionType::from_u8(4), None);
        assert_eq!(TransactionType::from_u8(255), None);
    }

    #[test]
    fn test_transaction_type_display() {
        assert_eq!(format!("{}", TransactionType::Transfer), "Transfer");
        assert_eq!(format!("{}", TransactionType::Stake), "Stake");
        assert_eq!(format!("{}", TransactionType::Unstake), "Unstake");
        assert_eq!(format!("{}", TransactionType::SlashEvidence), "SlashEvidence");
    }

    #[test]
    fn test_stake_amount_ordering() {
        assert!(StakeAmount(100) < StakeAmount(200));
        assert_eq!(StakeAmount(50), StakeAmount(50));
    }

    #[test]
    fn test_unbonding_entry_serde_roundtrip() {
        let entry = UnbondingEntry {
            validator_id: ValidatorId([0xaa; 32]),
            amount: StakeAmount(50_000),
            release_height: 11_000,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: UnbondingEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, deserialized);
    }

    #[test]
    fn test_round_display() {
        assert_eq!(format!("{}", Round(0)), "0");
        assert_eq!(format!("{}", Round(42)), "42");
    }

    #[test]
    fn test_vote_type_serde_roundtrip() {
        let prevote = VoteType::Prevote;
        let precommit = VoteType::Precommit;
        let json_pv = serde_json::to_string(&prevote).unwrap();
        let json_pc = serde_json::to_string(&precommit).unwrap();
        assert_eq!(
            serde_json::from_str::<VoteType>(&json_pv).unwrap(),
            VoteType::Prevote
        );
        assert_eq!(
            serde_json::from_str::<VoteType>(&json_pc).unwrap(),
            VoteType::Precommit
        );
    }

    #[test]
    fn test_v2_constants_are_deterministic() {
        assert_eq!(V2_ACTIVATION_HEIGHT, 10_000);
        assert_eq!(MIN_VALIDATOR_STAKE, 100_000);
        assert_eq!(UNBONDING_PERIOD, 1_000);
        assert_eq!(SLASH_PERCENTAGE, 10);
    }

    #[test]
    fn test_v1_serialization_unchanged_with_v2_types() {
        let block = Block {
            parent_hash: BlockHash([0u8; 32]),
            height: 1,
            epoch: 0,
            protocol_version: PROTOCOL_VERSION_V1,
            round: 0,
            proposer_id: ValidatorId([0xaa; 32]),
            transactions: vec![],
            signatures: vec![],
            state_hash: StateHash([0xbb; 32]),
            timestamp: 0,
        };
        let bytes = serialize_block_canonical(&block);
        assert_eq!(bytes.len(), 152);

        let tx = Transaction {
            sender: AccountId([0x11; 32]),
            recipient: AccountId([0x22; 32]),
            amount: 500,
            nonce: 10,
            signature: Signature([0x33; 64]),
            tx_type: TransactionType::Transfer,
            evidence: None,
        };
        let tx_bytes = serialize_transaction_canonical(&tx);
        assert_eq!(tx_bytes.len(), 152);
    }
}
