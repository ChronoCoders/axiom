use axiom_primitives::{
    serialize_block_canonical, serialize_string, serialize_transaction_canonical_v1,
    serialize_transaction_canonical_v2, serialize_u64, serialize_vote_canonical,
    serialize_proposal_canonical, to_hex, Block, BlockHash, GenesisConfig, ProtocolVersion,
    Proposal, PublicKey, Signature, StateHash, Transaction, TransactionHash, ValidatorId, Vote,
    VotePhase,
};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use thiserror::Error;

// Error type for crypto operations
#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Invalid public key")]
    InvalidPublicKey,
    #[error("Invalid private key")]
    InvalidPrivateKey,
    #[error("Hash mismatch: expected {expected}, got {got}")]
    HashMismatch { expected: String, got: String },
}

// Re-export PrivateKey as ed25519_dalek::SigningKey for convenience,
// or wrap it if we want strict typing. The guide uses "PrivateKey".
// ed25519-dalek 2.x uses SigningKey / VerifyingKey.
// Let's define aliases or wrappers.
pub type PrivateKey = SigningKey;
// PublicKey is already defined in primitives as a wrapper around [u8; 32].
// We need to convert between primitives::PublicKey and ed25519_dalek::VerifyingKey.

// Hashing

/// Computes SHA-256 hash of data
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Computes hash of a block
pub fn compute_block_hash(block: &Block) -> BlockHash {
    let bytes = serialize_block_canonical(block);
    BlockHash(sha256(&bytes))
}

/// Computes hash of a transaction
pub fn compute_transaction_hash(tx: &Transaction) -> TransactionHash {
    let bytes = serialize_transaction_canonical_v1(tx);
    TransactionHash(sha256(&bytes))
}

pub fn compute_transaction_hash_v2(tx: &Transaction) -> TransactionHash {
    let bytes = serialize_transaction_canonical_v2(tx);
    TransactionHash(sha256(&bytes))
}

pub fn compute_transaction_hash_for_height(height: u64, tx: &Transaction) -> TransactionHash {
    match ProtocolVersion::for_height(height) {
        ProtocolVersion::V1 => compute_transaction_hash(tx),
        ProtocolVersion::V2 => compute_transaction_hash_v2(tx),
    }
}

/// Computes the initial state hash from genesis config
/// Simulates canonical state serialization:
/// total_supply (u64)
/// block_reward (u64)
/// accounts (map: count + sorted entries)
/// validators (map: count + sorted entries)
pub fn compute_genesis_hash(genesis: &GenesisConfig) -> StateHash {
    let mut buf = Vec::new();

    // 1. total_supply
    serialize_u64(genesis.total_supply, &mut buf);
    // 2. block_reward
    serialize_u64(genesis.block_reward, &mut buf);

    // 3. accounts (sorted by ID)
    let mut accounts = genesis.accounts.clone();
    accounts.sort_by(|a, b| a.id.cmp(&b.id));

    let accounts_count = accounts.len() as u32;
    buf.extend_from_slice(&accounts_count.to_be_bytes());

    for acc in accounts {
        // key: account_id (length-prefixed hex string)
        serialize_string(&to_hex(&acc.id.0), &mut buf);
        // value: balance (u64), nonce (u64)
        serialize_u64(acc.balance, &mut buf);
        serialize_u64(acc.nonce, &mut buf);
    }

    // 4. validators (sorted by ID)
    let mut validators = genesis.validators.clone();
    validators.sort_by(|a, b| a.id.cmp(&b.id));

    let validators_count = validators.len() as u32;
    buf.extend_from_slice(&validators_count.to_be_bytes());

    for val in validators {
        // key: validator_id (length-prefixed hex string)
        serialize_string(&to_hex(&val.id.0), &mut buf);
        // value: voting_power (u64), account_id (len-prefixed string), active (u8)
        serialize_u64(val.voting_power, &mut buf);
        serialize_string(&to_hex(&val.account_id.0), &mut buf);
        buf.push(if val.active { 1 } else { 0 });
    }

    StateHash(sha256(&buf))
}

// Signing

/// Signs a transaction
pub fn sign_transaction(private_key: &PrivateKey, tx: &Transaction) -> Signature {
    let bytes = serialize_transaction_canonical_v1(tx);
    // ed25519-dalek returns a Signature struct, we need [u8; 64]
    let sig = private_key.sign(&bytes);
    Signature(sig.to_bytes())
}

pub fn sign_transaction_v2(private_key: &PrivateKey, tx: &Transaction) -> Signature {
    let bytes = serialize_transaction_canonical_v2(tx);
    let sig = private_key.sign(&bytes);
    Signature(sig.to_bytes())
}

pub fn sign_transaction_for_height(height: u64, private_key: &PrivateKey, tx: &Transaction) -> Signature {
    match ProtocolVersion::for_height(height) {
        ProtocolVersion::V1 => sign_transaction(private_key, tx),
        ProtocolVersion::V2 => sign_transaction_v2(private_key, tx),
    }
}

/// Verifies a transaction signature
pub fn verify_transaction_signature(tx: &Transaction) -> Result<(), CryptoError> {
    let bytes = serialize_transaction_canonical_v1(tx);
    let public_key_bytes = tx.sender.0;

    let verifying_key =
        VerifyingKey::from_bytes(&public_key_bytes).map_err(|_| CryptoError::InvalidPublicKey)?;

    let signature = ed25519_dalek::Signature::from_bytes(&tx.signature.0);

    verifying_key
        .verify(&bytes, &signature)
        .map_err(|_| CryptoError::InvalidSignature)
}

pub fn verify_transaction_signature_v2(tx: &Transaction) -> Result<(), CryptoError> {
    let bytes = serialize_transaction_canonical_v2(tx);
    let public_key_bytes = tx.sender.0;

    let verifying_key =
        VerifyingKey::from_bytes(&public_key_bytes).map_err(|_| CryptoError::InvalidPublicKey)?;

    let signature = ed25519_dalek::Signature::from_bytes(&tx.signature.0);

    verifying_key
        .verify(&bytes, &signature)
        .map_err(|_| CryptoError::InvalidSignature)
}

pub fn verify_transaction_signature_for_height(height: u64, tx: &Transaction) -> Result<(), CryptoError> {
    match ProtocolVersion::for_height(height) {
        ProtocolVersion::V1 => verify_transaction_signature(tx),
        ProtocolVersion::V2 => verify_transaction_signature_v2(tx),
    }
}

// Vote signing (PROTOCOL.md Section 8.4)
// Vote message = SHA-256(block_hash || height as u64 big-endian)

pub fn sign_vote(private_key: &PrivateKey, block_hash: &BlockHash, height: u64) -> Signature {
    let mut msg = Vec::new();
    msg.extend_from_slice(&block_hash.0);
    msg.extend_from_slice(&height.to_be_bytes());
    let msg_hash = sha256(&msg);

    let sig = private_key.sign(&msg_hash);
    Signature(sig.to_bytes())
}

pub fn verify_vote(
    public_key: &PublicKey,
    block_hash: &BlockHash,
    height: u64,
    signature: &Signature,
) -> Result<(), CryptoError> {
    let mut msg = Vec::new();
    msg.extend_from_slice(&block_hash.0);
    msg.extend_from_slice(&height.to_be_bytes());
    let msg_hash = sha256(&msg);

    let verifying_key =
        VerifyingKey::from_bytes(&public_key.0).map_err(|_| CryptoError::InvalidPublicKey)?;

    let sig = ed25519_dalek::Signature::from_bytes(&signature.0);

    verifying_key
        .verify(&msg_hash, &sig)
        .map_err(|_| CryptoError::InvalidSignature)
}

pub fn sign_consensus_vote(private_key: &PrivateKey, vote: &Vote) -> Signature {
    let bytes = serialize_vote_canonical(vote);
    let msg_hash = sha256(&bytes);
    let sig = private_key.sign(&msg_hash);
    Signature(sig.to_bytes())
}

pub fn verify_consensus_vote(vote: &Vote) -> Result<(), CryptoError> {
    let bytes = serialize_vote_canonical(vote);
    let msg_hash = sha256(&bytes);

    let verifying_key = VerifyingKey::from_bytes(&vote.validator_id.0)
        .map_err(|_| CryptoError::InvalidPublicKey)?;

    let sig = ed25519_dalek::Signature::from_bytes(&vote.signature.0);

    verifying_key
        .verify(&msg_hash, &sig)
        .map_err(|_| CryptoError::InvalidSignature)
}

pub fn sign_proposal(private_key: &PrivateKey, proposal: &Proposal) -> Signature {
    let bytes = serialize_proposal_canonical(proposal);
    let msg_hash = sha256(&bytes);
    let sig = private_key.sign(&msg_hash);
    Signature(sig.to_bytes())
}

pub fn verify_proposal(proposal: &Proposal) -> Result<(), CryptoError> {
    let bytes = serialize_proposal_canonical(proposal);
    let msg_hash = sha256(&bytes);

    let verifying_key = VerifyingKey::from_bytes(&proposal.proposer_id.0)
        .map_err(|_| CryptoError::InvalidPublicKey)?;

    let sig = ed25519_dalek::Signature::from_bytes(&proposal.signature.0);

    verifying_key
        .verify(&msg_hash, &sig)
        .map_err(|_| CryptoError::InvalidSignature)
}

pub fn sign_precommit(
    private_key: &PrivateKey,
    validator_id: &ValidatorId,
    block_hash: &BlockHash,
    height: u64,
    round: u64,
) -> Signature {
    let vote = Vote {
        height,
        round,
        phase: VotePhase::Precommit,
        block_hash: Some(*block_hash),
        validator_id: *validator_id,
        signature: Signature([0u8; 64]),
    };
    sign_consensus_vote(private_key, &vote)
}

pub fn verify_precommit(
    validator_id: &ValidatorId,
    block_hash: &BlockHash,
    height: u64,
    round: u64,
    signature: &Signature,
) -> Result<(), CryptoError> {
    let vote = Vote {
        height,
        round,
        phase: VotePhase::Precommit,
        block_hash: Some(*block_hash),
        validator_id: *validator_id,
        signature: *signature,
    };
    verify_consensus_vote(&vote)
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    a.ct_eq(b).into()
}

/// Compares two byte slices in constant time, regardless of their lengths.
/// Both slices are hashed with SHA-256 first so the comparison always
/// operates on fixed-size 32-byte digests, preventing length timing leaks.
pub fn ct_compare(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    let ha = sha256(a);
    let hb = sha256(b);
    ha.ct_eq(&hb).into()
}

// Key handling helpers

pub fn generate_keypair_from_seed(seed: &[u8; 32]) -> (PrivateKey, PublicKey) {
    let signing_key = SigningKey::from_bytes(seed);
    let verifying_key = signing_key.verifying_key();
    (signing_key, PublicKey(verifying_key.to_bytes()))
}

pub fn test_keypair(identity: &str) -> (PrivateKey, PublicKey) {
    let seed = sha256(identity.as_bytes());
    generate_keypair_from_seed(&seed)
}
