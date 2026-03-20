use axiom_primitives::GenesisConfig;
use axiom_state::{Account, State, Validator};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub fn load_genesis_state(path: &Path) -> Result<State, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let dto: GenesisConfig = serde_json::from_str(&content)?;

    // Validate Total Supply
    let calculated_supply: u64 = dto
        .accounts
        .iter()
        .try_fold(0u64, |acc, a| acc.checked_add(a.balance))
        .ok_or("Genesis supply overflow during summation")?;
    if calculated_supply != dto.total_supply {
        return Err(format!(
            "Genesis total supply mismatch: declared {}, calculated {}",
            dto.total_supply, calculated_supply
        )
        .into());
    }

    // Convert to State
    let mut accounts = BTreeMap::new();

    for acc in dto.accounts {
        accounts.insert(
            acc.id,
            Account {
                balance: acc.balance,
                nonce: acc.nonce,
            },
        );
    }

    let mut validators = BTreeMap::new();
    for v in dto.validators {
        // Validate validator account exists
        if !accounts.contains_key(&v.account_id) {
            return Err(format!(
                "Validator {} associated account {} does not exist",
                v.id, v.account_id
            )
            .into());
        }

        validators.insert(
            v.id,
            Validator {
                voting_power: v.voting_power,
                account_id: v.account_id,
                active: v.active,
            },
        );
    }

    Ok(State {
        total_supply: dto.total_supply,
        block_reward: dto.block_reward,
        accounts,
        validators,
    })
}
