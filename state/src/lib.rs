use axiom_primitives::{
    serialize_string, serialize_u64, to_hex, AccountId, GenesisConfig, StakeAmount, UnbondingEntry,
    ValidatorId, MIN_VALIDATOR_STAKE, UNBONDING_PERIOD,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

// -----------------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub total_supply: u64,
    pub block_reward: u64,
    pub accounts: BTreeMap<AccountId, Account>,
    pub validators: BTreeMap<ValidatorId, Validator>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub balance: u64,
    pub nonce: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validator {
    pub voting_power: u64,
    pub account_id: AccountId,
    pub active: bool,
}

// -----------------------------------------------------------------------------
// v2 Staking State (scaffolding only — inert during v1)
// -----------------------------------------------------------------------------

/// Staking state for Protocol v2. This structure exists alongside the core State
/// but is NOT included in v1 state hash computation or any v1 execution path.
/// It remains empty/default until the v2 activation height is reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StakingState {
    pub stakes: BTreeMap<ValidatorId, StakeAmount>,
    pub minimum_stake: u64,
    pub unbonding_period: u64,
    pub unbonding_queue: Vec<UnbondingEntry>,
    #[serde(default)]
    pub epoch: u64,
    #[serde(default)]
    pub jailed_validators: BTreeSet<ValidatorId>,
    #[serde(default)]
    pub processed_evidence: BTreeSet<[u8; 32]>,
}

impl StakingState {
    /// Creates an empty staking state (inert for v1).
    pub fn empty() -> Self {
        Self {
            stakes: BTreeMap::new(),
            minimum_stake: 0,
            unbonding_period: 0,
            unbonding_queue: Vec::new(),
            epoch: 0,
            jailed_validators: BTreeSet::new(),
            processed_evidence: BTreeSet::new(),
        }
    }

    /// Creates an active staking state for v2 with protocol constants.
    pub fn new_active() -> Self {
        Self {
            stakes: BTreeMap::new(),
            minimum_stake: MIN_VALIDATOR_STAKE,
            unbonding_period: UNBONDING_PERIOD,
            unbonding_queue: Vec::new(),
            epoch: 0,
            jailed_validators: BTreeSet::new(),
            processed_evidence: BTreeSet::new(),
        }
    }

    /// Returns true if the staking state has no data (v1 default).
    pub fn is_empty(&self) -> bool {
        self.stakes.is_empty()
            && self.unbonding_queue.is_empty()
            && self.epoch == 0
            && self.jailed_validators.is_empty()
            && self.processed_evidence.is_empty()
    }

    pub fn serialize_staking_canonical(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        serialize_u64(self.epoch, &mut buf);
        serialize_u64(self.minimum_stake, &mut buf);
        serialize_u64(self.unbonding_period, &mut buf);

        let stakes_len = self.stakes.len() as u32;
        buf.extend_from_slice(&stakes_len.to_be_bytes());
        for (vid, amount) in &self.stakes {
            serialize_string(&to_hex(&vid.0), &mut buf);
            serialize_u64(amount.0, &mut buf);
        }

        let queue_len = self.unbonding_queue.len() as u32;
        buf.extend_from_slice(&queue_len.to_be_bytes());
        for entry in &self.unbonding_queue {
            serialize_string(&to_hex(&entry.validator_id.0), &mut buf);
            serialize_u64(entry.amount.0, &mut buf);
            serialize_u64(entry.release_height, &mut buf);
        }

        let jailed_len = self.jailed_validators.len() as u32;
        buf.extend_from_slice(&jailed_len.to_be_bytes());
        for vid in &self.jailed_validators {
            serialize_string(&to_hex(&vid.0), &mut buf);
        }

        let processed_len = self.processed_evidence.len() as u32;
        buf.extend_from_slice(&processed_len.to_be_bytes());
        for h in &self.processed_evidence {
            serialize_string(&to_hex(h), &mut buf);
        }

        buf
    }

    /// Applies a stake operation. The caller must have already validated that
    /// the account has sufficient balance and deducted it.
    pub fn apply_stake(
        &mut self,
        validator_id: ValidatorId,
        amount: StakeAmount,
    ) -> Result<(), StateError> {
        let existing = self.stakes.get(&validator_id).map(|a| a.0).unwrap_or(0);
        let new_amount = existing
            .checked_add(amount.0)
            .ok_or(StateError::Overflow)?;
        self.stakes.insert(validator_id, StakeAmount(new_amount));
        Ok(())
    }

    /// Applies an unstake operation. Removes the stake and enqueues an
    /// unbonding entry. The caller must NOT return funds to the account yet;
    /// funds are released when the unbonding period expires.
    pub fn apply_unstake(
        &mut self,
        validator_id: ValidatorId,
        amount: u64,
        current_height: u64,
    ) -> Result<(), StateError> {
        let staked = self
            .stakes
            .get(&validator_id)
            .ok_or(StateError::NoActiveStake {
                account: validator_id,
            })?;

        if amount > staked.0 {
            return Err(StateError::InsufficientStake {
                requested: amount,
                available: staked.0,
            });
        }

        let remaining = staked
            .0
            .checked_sub(amount)
            .ok_or(StateError::Overflow)?;

        if remaining == 0 {
            self.stakes.remove(&validator_id);
        } else {
            self.stakes.insert(validator_id, StakeAmount(remaining));
        }

        let release_height = current_height
            .checked_add(self.unbonding_period)
            .ok_or(StateError::Overflow)?;

        self.unbonding_queue.push(UnbondingEntry {
            validator_id,
            amount: StakeAmount(amount),
            release_height,
        });

        Ok(())
    }

    /// Releases all unbonding entries whose release_height <= current_height.
    /// Returns a list of (ValidatorId, amount) pairs to credit back to accounts.
    /// The returned list is sorted by validator_id for deterministic processing.
    pub fn release_unbonded(&mut self, current_height: u64) -> Vec<(ValidatorId, u64)> {
        let mut released = Vec::new();
        let mut remaining = Vec::new();

        for entry in self.unbonding_queue.drain(..) {
            if entry.release_height <= current_height {
                released.push((entry.validator_id, entry.amount.0));
            } else {
                remaining.push(entry);
            }
        }

        self.unbonding_queue = remaining;

        released.sort_by(|a, b| a.0.cmp(&b.0));
        released
    }

    /// Returns the total amount currently staked across all validators.
    pub fn total_staked(&self) -> Result<u64, StateError> {
        self.stakes
            .values()
            .map(|s| s.0)
            .try_fold(0u64, |acc, x| acc.checked_add(x).ok_or(StateError::Overflow))
    }

    /// Returns the total amount currently in the unbonding queue.
    pub fn total_unbonding(&self) -> Result<u64, StateError> {
        self.unbonding_queue
            .iter()
            .map(|e| e.amount.0)
            .try_fold(0u64, |acc, x| acc.checked_add(x).ok_or(StateError::Overflow))
    }
}

/// Verifies that the combined economic invariants hold for v2 state.
/// total_supply == sum(balances) + sum(staked) + sum(unbonding)
pub fn verify_staking_invariants(
    state: &State,
    staking: &StakingState,
) -> Result<(), StateError> {
    let balance_sum: u64 = state
        .accounts
        .values()
        .map(|a| a.balance)
        .try_fold(0u64, |acc, x| acc.checked_add(x).ok_or(StateError::Overflow))?;

    let staked_sum = staking.total_staked()?;
    let unbonding_sum = staking.total_unbonding()?;

    let total = balance_sum
        .checked_add(staked_sum)
        .ok_or(StateError::Overflow)?
        .checked_add(unbonding_sum)
        .ok_or(StateError::Overflow)?;

    if total != state.total_supply {
        return Err(StateError::BalanceMismatch {
            expected_supply: state.total_supply,
            actual_sum: total,
        });
    }

    Ok(())
}

// -----------------------------------------------------------------------------
// Errors
// -----------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum StateError {
    #[error("Balance mismatch: expected supply {expected_supply}, actual sum {actual_sum}")]
    BalanceMismatch {
        expected_supply: u64,
        actual_sum: u64,
    },

    #[error("Duplicate account: {id}")]
    DuplicateAccount { id: AccountId },

    #[error("Duplicate validator: {id}")]
    DuplicateValidator { id: ValidatorId },

    #[error("Validator {validator_id} linked to missing account {account_id}")]
    ValidatorAccountMissing {
        validator_id: ValidatorId,
        account_id: AccountId,
    },

    #[error("Genesis supply mismatch: declared {declared}, actual {actual}")]
    GenesisSupplyMismatch { declared: u64, actual: u64 },

    #[error("Arithmetic overflow")]
    Overflow,

    #[error("No active stake for account {account}")]
    NoActiveStake { account: ValidatorId },

    #[error("Unstake amount {requested} exceeds staked amount {available}")]
    InsufficientStake { requested: u64, available: u64 },
}

// -----------------------------------------------------------------------------
// Implementation
// -----------------------------------------------------------------------------

impl State {
    /// Creates a new State from a GenesisConfig
    pub fn from_genesis(genesis: &GenesisConfig) -> Result<Self, StateError> {
        let mut accounts = BTreeMap::new();
        let mut validators = BTreeMap::new();
        let mut actual_supply: u64 = 0;

        // Process accounts
        for gen_acc in &genesis.accounts {
            if accounts.contains_key(&gen_acc.id) {
                return Err(StateError::DuplicateAccount { id: gen_acc.id });
            }

            let account = Account {
                balance: gen_acc.balance,
                nonce: gen_acc.nonce,
            };

            accounts.insert(gen_acc.id, account);

            actual_supply = actual_supply
                .checked_add(gen_acc.balance)
                .ok_or(StateError::Overflow)?;
        }

        // Verify total supply matches sum of balances
        if actual_supply != genesis.total_supply {
            return Err(StateError::GenesisSupplyMismatch {
                declared: genesis.total_supply,
                actual: actual_supply,
            });
        }

        // Process validators
        for gen_val in &genesis.validators {
            if validators.contains_key(&gen_val.id) {
                return Err(StateError::DuplicateValidator { id: gen_val.id });
            }

            // Verify validator's account exists
            if !accounts.contains_key(&gen_val.account_id) {
                return Err(StateError::ValidatorAccountMissing {
                    validator_id: gen_val.id,
                    account_id: gen_val.account_id,
                });
            }

            let validator = Validator {
                voting_power: gen_val.voting_power,
                account_id: gen_val.account_id,
                active: gen_val.active,
            };

            validators.insert(gen_val.id, validator);
        }

        let state = State {
            total_supply: genesis.total_supply,
            block_reward: genesis.block_reward,
            accounts,
            validators,
        };

        // Final invariant check (redundant but safe)
        state.verify_invariants()?;

        Ok(state)
    }

    // Queries

    pub fn get_account(&self, id: &AccountId) -> Option<&Account> {
        self.accounts.get(id)
    }

    pub fn get_validator(&self, id: &ValidatorId) -> Option<&Validator> {
        self.validators.get(id)
    }

    /// Returns active validators sorted by ID
    pub fn active_validators(&self) -> Vec<(&ValidatorId, &Validator)> {
        self.validators.iter().filter(|(_, v)| v.active).collect()
    }

    pub fn total_voting_power(&self) -> Result<u64, StateError> {
        self.validators
            .values()
            .filter(|v| v.active)
            .map(|v| v.voting_power)
            .try_fold(0u64, |acc, x| {
                acc.checked_add(x).ok_or(StateError::Overflow)
            })
    }

    // Mutation methods for Phase 4

    pub fn get_account_mut(&mut self, id: &AccountId) -> Option<&mut Account> {
        self.accounts.get_mut(id)
    }

    pub fn create_account(&mut self, id: AccountId, account: Account) -> Option<Account> {
        self.accounts.insert(id, account)
    }

    pub fn apply_reward(&mut self, account_id: &AccountId, amount: u64) -> Result<(), StateError> {
        self.total_supply = self
            .total_supply
            .checked_add(amount)
            .ok_or(StateError::Overflow)?;

        if let Some(acc) = self.accounts.get_mut(account_id) {
            acc.balance = acc
                .balance
                .checked_add(amount)
                .ok_or(StateError::Overflow)?;
        } else {
            // Create new account if it doesn't exist (rewards might be initial funds)
            let acc = Account {
                balance: amount,
                nonce: 0,
            };
            self.accounts.insert(*account_id, acc);
        }
        Ok(())
    }

    // Serialization

    pub fn serialize_state_canonical(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        serialize_u64(self.total_supply, &mut buf);
        serialize_u64(self.block_reward, &mut buf);

        // Accounts
        let accounts_len = self.accounts.len() as u32;
        buf.extend_from_slice(&accounts_len.to_be_bytes());
        for (id, acc) in &self.accounts {
            serialize_string(&to_hex(&id.0), &mut buf);
            serialize_u64(acc.balance, &mut buf);
            serialize_u64(acc.nonce, &mut buf);
        }

        // Validators
        let validators_len = self.validators.len() as u32;
        buf.extend_from_slice(&validators_len.to_be_bytes());
        for (id, val) in &self.validators {
            serialize_string(&to_hex(&id.0), &mut buf);
            serialize_u64(val.voting_power, &mut buf);
            serialize_string(&to_hex(&val.account_id.0), &mut buf);
            buf.push(if val.active { 1 } else { 0 });
        }

        buf
    }

    // Validation

    pub fn verify_invariants(&self) -> Result<(), StateError> {
        let mut sum: u64 = 0;
        for acc in self.accounts.values() {
            sum = sum.checked_add(acc.balance).ok_or(StateError::Overflow)?;
        }

        if sum != self.total_supply {
            return Err(StateError::BalanceMismatch {
                expected_supply: self.total_supply,
                actual_sum: sum,
            });
        }

        // Ensure all validators point to existing accounts
        for (vid, val) in &self.validators {
            if !self.accounts.contains_key(&val.account_id) {
                return Err(StateError::ValidatorAccountMissing {
                    validator_id: *vid,
                    account_id: val.account_id,
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_primitives::{GenesisAccount, GenesisValidator};

    fn mock_account_id(byte: u8) -> AccountId {
        AccountId([byte; 32])
    }

    fn mock_validator_id(byte: u8) -> ValidatorId {
        ValidatorId([byte; 32])
    }

    #[test]
    fn test_genesis_construction_valid() {
        let acc_id = mock_account_id(1);
        let val_id = mock_validator_id(1);

        let genesis = GenesisConfig {
            total_supply: 100,
            block_reward: 10,
            accounts: vec![GenesisAccount {
                id: acc_id,
                balance: 100,
                nonce: 0,
            }],
            validators: vec![GenesisValidator {
                id: val_id,
                voting_power: 10,
                account_id: acc_id,
                active: true,
            }],
        };

        let state = State::from_genesis(&genesis).unwrap();
        assert_eq!(state.total_supply, 100);
        assert!(state.get_account(&acc_id).is_some());
        assert!(state.get_validator(&val_id).is_some());
    }

    #[test]
    fn test_genesis_supply_mismatch() {
        let acc_id = mock_account_id(1);
        let genesis = GenesisConfig {
            total_supply: 200, // Declared 200
            block_reward: 10,
            accounts: vec![
                GenesisAccount {
                    id: acc_id,
                    balance: 100,
                    nonce: 0,
                }, // Actual 100
            ],
            validators: vec![],
        };

        let res = State::from_genesis(&genesis);
        assert!(matches!(res, Err(StateError::GenesisSupplyMismatch { .. })));
    }

    #[test]
    fn test_genesis_duplicate_account() {
        let acc_id = mock_account_id(1);
        let genesis = GenesisConfig {
            total_supply: 200,
            block_reward: 10,
            accounts: vec![
                GenesisAccount {
                    id: acc_id,
                    balance: 100,
                    nonce: 0,
                },
                GenesisAccount {
                    id: acc_id,
                    balance: 100,
                    nonce: 0,
                }, // Duplicate
            ],
            validators: vec![],
        };

        let res = State::from_genesis(&genesis);
        assert!(matches!(res, Err(StateError::DuplicateAccount { .. })));
    }

    #[test]
    fn test_genesis_validator_account_missing() {
        let val_id = mock_validator_id(1);
        let acc_id = mock_account_id(1); // Not in accounts list

        let genesis = GenesisConfig {
            total_supply: 0,
            block_reward: 10,
            accounts: vec![],
            validators: vec![GenesisValidator {
                id: val_id,
                voting_power: 10,
                account_id: acc_id,
                active: true,
            }],
        };

        let res = State::from_genesis(&genesis);
        assert!(matches!(
            res,
            Err(StateError::ValidatorAccountMissing { .. })
        ));
    }

    #[test]
    fn test_verify_invariants_valid() {
        let acc_id = mock_account_id(1);
        let mut accounts = BTreeMap::new();
        accounts.insert(
            acc_id,
            Account {
                balance: 100,
                nonce: 0,
            },
        );

        let state = State {
            total_supply: 100,
            block_reward: 10,
            accounts,
            validators: BTreeMap::new(),
        };

        assert!(state.verify_invariants().is_ok());
    }

    #[test]
    fn test_verify_invariants_balance_mismatch() {
        let acc_id = mock_account_id(1);
        let mut accounts = BTreeMap::new();
        accounts.insert(
            acc_id,
            Account {
                balance: 50,
                nonce: 0,
            },
        ); // Sum 50

        let state = State {
            total_supply: 100, // Expected 100
            block_reward: 10,
            accounts,
            validators: BTreeMap::new(),
        };

        assert!(matches!(
            state.verify_invariants(),
            Err(StateError::BalanceMismatch { .. })
        ));
    }

    #[test]
    fn test_ordering_is_deterministic() {
        // BTreeMap should sort keys
        let acc1 = mock_account_id(1);
        let acc2 = mock_account_id(2);

        let mut accounts = BTreeMap::new();
        accounts.insert(
            acc2,
            Account {
                balance: 50,
                nonce: 0,
            },
        );
        accounts.insert(
            acc1,
            Account {
                balance: 50,
                nonce: 0,
            },
        );

        // Iteration should be 1 then 2
        let keys: Vec<_> = accounts.keys().collect();
        assert_eq!(keys[0], &acc1);
        assert_eq!(keys[1], &acc2);
    }

    #[test]
    fn test_get_account_none() {
        let genesis = GenesisConfig {
            total_supply: 0,
            block_reward: 10,
            accounts: vec![],
            validators: vec![],
        };
        let state = State::from_genesis(&genesis).unwrap();
        assert!(state.get_account(&mock_account_id(99)).is_none());
    }

    #[test]
    fn test_get_validator_none() {
        let genesis = GenesisConfig {
            total_supply: 0,
            block_reward: 10,
            accounts: vec![],
            validators: vec![],
        };
        let state = State::from_genesis(&genesis).unwrap();
        assert!(state.get_validator(&mock_validator_id(99)).is_none());
    }

    #[test]
    fn test_active_validators_filters_inactive() {
        let acc_id = mock_account_id(1);
        let val_active = mock_validator_id(1);
        let val_inactive = mock_validator_id(2);

        let genesis = GenesisConfig {
            total_supply: 100,
            block_reward: 10,
            accounts: vec![GenesisAccount {
                id: acc_id,
                balance: 100,
                nonce: 0,
            }],
            validators: vec![
                GenesisValidator {
                    id: val_active,
                    voting_power: 10,
                    account_id: acc_id,
                    active: true,
                },
                GenesisValidator {
                    id: val_inactive,
                    voting_power: 10,
                    account_id: acc_id,
                    active: false,
                },
            ],
        };
        let state = State::from_genesis(&genesis).unwrap();
        let active = state.active_validators();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].0, &val_active);
    }

    #[test]
    fn test_total_voting_power_excludes_inactive() {
        let acc_id = mock_account_id(1);
        let val_active = mock_validator_id(1);
        let val_inactive = mock_validator_id(2);

        let genesis = GenesisConfig {
            total_supply: 100,
            block_reward: 10,
            accounts: vec![GenesisAccount {
                id: acc_id,
                balance: 100,
                nonce: 0,
            }],
            validators: vec![
                GenesisValidator {
                    id: val_active,
                    voting_power: 10,
                    account_id: acc_id,
                    active: true,
                },
                GenesisValidator {
                    id: val_inactive,
                    voting_power: 20,
                    account_id: acc_id,
                    active: false,
                },
            ],
        };
        let state = State::from_genesis(&genesis).unwrap();
        assert_eq!(state.total_voting_power().unwrap(), 10);
    }

    #[test]
    fn test_serialize_state_canonical_correctness() {
        let acc_id = mock_account_id(0xaa);
        let val_id = mock_validator_id(0xbb);

        let genesis = GenesisConfig {
            total_supply: 100,
            block_reward: 10,
            accounts: vec![GenesisAccount {
                id: acc_id,
                balance: 100,
                nonce: 5,
            }],
            validators: vec![GenesisValidator {
                id: val_id,
                voting_power: 50,
                account_id: acc_id,
                active: true,
            }],
        };
        let state = State::from_genesis(&genesis).unwrap();
        let bytes = state.serialize_state_canonical();

        // Check manually:
        // total_supply (8) + block_reward (8) = 16
        // accounts_len (4)
        // account: id (4+64) + balance (8) + nonce (8) = 84
        // validators_len (4)
        // validator: id (4+64) + voting_power (8) + account_id (4+64) + active (1) = 145
        // Total = 16 + 4 + 84 + 4 + 145 = 253 bytes
        assert_eq!(bytes.len(), 253);
    }

    #[test]
    fn test_serialize_state_canonical_determinism() {
        let acc1 = mock_account_id(1);
        let acc2 = mock_account_id(2);

        // Create state in different insertion order if possible, or just same state twice
        let mut state = State {
            total_supply: 0,
            block_reward: 0,
            accounts: BTreeMap::new(),
            validators: BTreeMap::new(),
        };

        // BTreeMap sorts automatically, so we test if re-serializing gives same result
        let bytes1 = state.serialize_state_canonical();
        let bytes2 = state.serialize_state_canonical();
        assert_eq!(bytes1, bytes2);

        // Add items out of order
        state.accounts.insert(
            acc2,
            Account {
                balance: 0,
                nonce: 0,
            },
        );
        state.accounts.insert(
            acc1,
            Account {
                balance: 0,
                nonce: 0,
            },
        );

        let bytes3 = state.serialize_state_canonical();

        // Create another state with items inserted in order
        let mut state2 = State {
            total_supply: 0,
            block_reward: 0,
            accounts: BTreeMap::new(),
            validators: BTreeMap::new(),
        };
        state2.accounts.insert(
            acc1,
            Account {
                balance: 0,
                nonce: 0,
            },
        );
        state2.accounts.insert(
            acc2,
            Account {
                balance: 0,
                nonce: 0,
            },
        );

        let bytes4 = state2.serialize_state_canonical();

        assert_eq!(bytes3, bytes4);
    }

    #[test]
    fn test_staking_state_empty() {
        let ss = StakingState::empty();
        assert!(ss.is_empty());
        assert!(ss.stakes.is_empty());
        assert!(ss.unbonding_queue.is_empty());
        assert_eq!(ss.minimum_stake, 0);
        assert_eq!(ss.unbonding_period, 0);
    }

    #[test]
    fn test_staking_state_serde_roundtrip() {
        let ss = StakingState::empty();
        let json = serde_json::to_string(&ss).unwrap();
        let deserialized: StakingState = serde_json::from_str(&json).unwrap();
        assert_eq!(ss, deserialized);
    }

    #[test]
    fn test_staking_state_not_empty_with_stake() {
        let mut ss = StakingState::empty();
        ss.stakes
            .insert(mock_validator_id(1), StakeAmount(100_000));
        assert!(!ss.is_empty());
    }

    #[test]
    fn test_v1_state_hash_unchanged_with_v2_scaffolding() {
        let acc_id = mock_account_id(0xaa);
        let val_id = mock_validator_id(0xbb);

        let genesis = GenesisConfig {
            total_supply: 100,
            block_reward: 10,
            accounts: vec![GenesisAccount {
                id: acc_id,
                balance: 100,
                nonce: 5,
            }],
            validators: vec![GenesisValidator {
                id: val_id,
                voting_power: 50,
                account_id: acc_id,
                active: true,
            }],
        };
        let state = State::from_genesis(&genesis).unwrap();
        let bytes = state.serialize_state_canonical();
        assert_eq!(bytes.len(), 253);
    }

    #[test]
    fn test_staking_state_new_active() {
        let ss = StakingState::new_active();
        assert_eq!(ss.minimum_stake, MIN_VALIDATOR_STAKE);
        assert_eq!(ss.unbonding_period, UNBONDING_PERIOD);
        assert!(ss.stakes.is_empty());
        assert!(ss.unbonding_queue.is_empty());
    }

    #[test]
    fn test_apply_stake_success() {
        let mut ss = StakingState::new_active();
        let vid = mock_validator_id(1);
        ss.apply_stake(vid, StakeAmount(1)).unwrap();
        assert_eq!(ss.stakes.get(&vid).unwrap().0, 1);
    }

    #[test]
    fn test_apply_stake_additive() {
        let mut ss = StakingState::new_active();
        let vid = mock_validator_id(1);
        ss.apply_stake(vid, StakeAmount(1)).unwrap();
        ss.apply_stake(vid, StakeAmount(2)).unwrap();
        assert_eq!(ss.stakes.get(&vid).unwrap().0, 3);
    }

    #[test]
    fn test_apply_unstake_success() {
        let mut ss = StakingState::new_active();
        let vid = mock_validator_id(1);
        ss.apply_stake(vid, StakeAmount(100_000)).unwrap();
        ss.apply_unstake(vid, 100_000, 10).unwrap();
        assert!(!ss.stakes.contains_key(&vid));
        assert_eq!(ss.unbonding_queue.len(), 1);
        assert_eq!(ss.unbonding_queue[0].amount.0, 100_000);
        assert_eq!(ss.unbonding_queue[0].release_height, 10 + UNBONDING_PERIOD);
    }

    #[test]
    fn test_apply_unstake_no_stake() {
        let mut ss = StakingState::new_active();
        let vid = mock_validator_id(1);
        let res = ss.apply_unstake(vid, 100_000, 10);
        assert!(matches!(res, Err(StateError::NoActiveStake { .. })));
    }

    #[test]
    fn test_apply_unstake_exceeds_stake() {
        let mut ss = StakingState::new_active();
        let vid = mock_validator_id(1);
        ss.apply_stake(vid, StakeAmount(100_000)).unwrap();
        let res = ss.apply_unstake(vid, 200_000, 10);
        assert!(matches!(res, Err(StateError::InsufficientStake { .. })));
    }

    #[test]
    fn test_apply_unstake_partial() {
        let mut ss = StakingState::new_active();
        let vid = mock_validator_id(1);
        ss.apply_stake(vid, StakeAmount(200_000)).unwrap();
        ss.apply_unstake(vid, 100_000, 10).unwrap();
        assert_eq!(ss.stakes.get(&vid).unwrap().0, 100_000);
        assert_eq!(ss.unbonding_queue.len(), 1);
        assert_eq!(ss.unbonding_queue[0].amount.0, 100_000);
        assert_eq!(ss.unbonding_queue[0].release_height, 10 + UNBONDING_PERIOD);
    }

    #[test]
    fn test_release_unbonded() {
        let mut ss = StakingState::new_active();
        let vid1 = mock_validator_id(2);
        let vid2 = mock_validator_id(1);
        ss.unbonding_queue.push(UnbondingEntry {
            validator_id: vid1,
            amount: StakeAmount(50_000),
            release_height: 100,
        });
        ss.unbonding_queue.push(UnbondingEntry {
            validator_id: vid2,
            amount: StakeAmount(30_000),
            release_height: 90,
        });
        let released = ss.release_unbonded(100);
        assert_eq!(released.len(), 2);
        assert_eq!(released[0].0, vid2);
        assert_eq!(released[0].1, 30_000);
        assert_eq!(released[1].0, vid1);
        assert_eq!(released[1].1, 50_000);
        assert!(ss.unbonding_queue.is_empty());
    }

    #[test]
    fn test_release_unbonded_none_ready() {
        let mut ss = StakingState::new_active();
        let vid = mock_validator_id(1);
        ss.unbonding_queue.push(UnbondingEntry {
            validator_id: vid,
            amount: StakeAmount(50_000),
            release_height: 200,
        });
        let released = ss.release_unbonded(100);
        assert!(released.is_empty());
        assert_eq!(ss.unbonding_queue.len(), 1);
    }

    #[test]
    fn test_release_unbonded_mixed() {
        let mut ss = StakingState::new_active();
        let vid1 = mock_validator_id(1);
        let vid2 = mock_validator_id(2);
        ss.unbonding_queue.push(UnbondingEntry {
            validator_id: vid1,
            amount: StakeAmount(50_000),
            release_height: 100,
        });
        ss.unbonding_queue.push(UnbondingEntry {
            validator_id: vid2,
            amount: StakeAmount(30_000),
            release_height: 200,
        });
        let released = ss.release_unbonded(100);
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].0, vid1);
        assert_eq!(released[0].1, 50_000);
        assert_eq!(ss.unbonding_queue.len(), 1);
        assert_eq!(ss.unbonding_queue[0].validator_id, vid2);
    }

    #[test]
    fn test_verify_staking_invariants_valid() {
        let acc_id = mock_account_id(1);
        let val_id = mock_validator_id(1);
        let mut accounts = BTreeMap::new();
        accounts.insert(acc_id, Account { balance: 700_000, nonce: 0 });
        let mut validators = BTreeMap::new();
        validators.insert(val_id, Validator {
            voting_power: 10,
            account_id: acc_id,
            active: true,
        });
        let state = State {
            total_supply: 1_000_000,
            block_reward: 10,
            accounts,
            validators,
        };
        let mut ss = StakingState::new_active();
        ss.stakes.insert(val_id, StakeAmount(200_000));
        ss.unbonding_queue.push(UnbondingEntry {
            validator_id: val_id,
            amount: StakeAmount(100_000),
            release_height: 500,
        });
        assert!(verify_staking_invariants(&state, &ss).is_ok());
    }

    #[test]
    fn test_verify_staking_invariants_mismatch() {
        let acc_id = mock_account_id(1);
        let val_id = mock_validator_id(1);
        let mut accounts = BTreeMap::new();
        accounts.insert(acc_id, Account { balance: 700_000, nonce: 0 });
        let mut validators = BTreeMap::new();
        validators.insert(val_id, Validator {
            voting_power: 10,
            account_id: acc_id,
            active: true,
        });
        let state = State {
            total_supply: 999_999,
            block_reward: 10,
            accounts,
            validators,
        };
        let mut ss = StakingState::new_active();
        ss.stakes.insert(val_id, StakeAmount(200_000));
        ss.unbonding_queue.push(UnbondingEntry {
            validator_id: val_id,
            amount: StakeAmount(100_000),
            release_height: 500,
        });
        assert!(matches!(
            verify_staking_invariants(&state, &ss),
            Err(StateError::BalanceMismatch { .. })
        ));
    }

    #[test]
    fn test_total_staked_and_unbonding() {
        let mut ss = StakingState::new_active();
        let vid1 = mock_validator_id(1);
        let vid2 = mock_validator_id(2);
        ss.stakes.insert(vid1, StakeAmount(100_000));
        ss.stakes.insert(vid2, StakeAmount(200_000));
        ss.unbonding_queue.push(UnbondingEntry {
            validator_id: vid1,
            amount: StakeAmount(50_000),
            release_height: 500,
        });
        ss.unbonding_queue.push(UnbondingEntry {
            validator_id: vid2,
            amount: StakeAmount(30_000),
            release_height: 600,
        });
        assert_eq!(ss.total_staked().unwrap(), 300_000);
        assert_eq!(ss.total_unbonding().unwrap(), 80_000);
    }
}
