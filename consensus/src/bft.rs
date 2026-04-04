use axiom_crypto::{compute_block_hash, sign_consensus_vote, sign_proposal, verify_consensus_vote, verify_proposal, PrivateKey};
use axiom_execution::{execute_proposal_v2, select_proposer_v2, ExecutionError};
use axiom_primitives::{Block, BlockHash, Evidence, LockState, Proposal, ProtocolVersion, Transaction, ValidatorId, ValidatorSignature, Vote, VotePhase, V2_ACTIVATION_HEIGHT, V2_MIGRATION_STAKE_PER_VALIDATOR};
use axiom_state::{StakingState, State};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Proposal,
    Prevote,
    Precommit,
}

#[derive(Debug, Clone)]
pub enum Outbound {
    Proposal(Proposal),
    Vote(Vote),
    Evidence(Evidence),
    CommittedBlock(Block),
    AdvanceRound { height: u64, round: u64 },
}

pub struct ProposalInputs<'a> {
    pub state: &'a State,
    pub staking: &'a StakingState,
    pub parent_hash: BlockHash,
    pub transactions: Vec<Transaction>,
    pub proposer_key: &'a PrivateKey,
    pub proposer_id: &'a ValidatorId,
    pub timestamp: u64,
}

#[derive(Debug)]
pub struct Engine {
    pub height: u64,
    pub round: u64,
    pub step: Step,
    lock: LockState,
    proposals_by_round: HashMap<u64, Proposal>,
    proposal_hash_by_round: HashMap<u64, BlockHash>,
    /// Inverse map: block hash → block, populated alongside proposals_by_round.
    /// Enables O(1) lookup in block_for_hash instead of O(rounds) linear scan.
    proposal_block_by_hash: HashMap<BlockHash, Block>,
    own_votes_sent: HashSet<(u64, u64, VotePhase)>,
    prevotes: HashMap<u64, HashMap<ValidatorId, Vote>>,
    precommits: HashMap<u64, HashMap<ValidatorId, Vote>>,
    double_vote_evidence: Vec<Evidence>,
    double_propose_evidence: Vec<Evidence>,
}

impl Engine {
    pub fn new(height: u64, lock: LockState) -> Self {
        Self {
            height,
            round: 0,
            step: Step::Proposal,
            lock,
            proposals_by_round: HashMap::new(),
            proposal_hash_by_round: HashMap::new(),
            proposal_block_by_hash: HashMap::new(),
            own_votes_sent: HashSet::new(),
            prevotes: HashMap::new(),
            precommits: HashMap::new(),
            double_vote_evidence: Vec::new(),
            double_propose_evidence: Vec::new(),
        }
    }

    pub fn lock_state(&self) -> LockState {
        self.lock.clone()
    }

    pub fn set_lock_state(&mut self, lock: LockState) {
        self.lock = lock;
    }

    pub fn proposer_for_round(
        &self,
        state: &State,
        staking: &StakingState,
    ) -> Result<ValidatorId, ExecutionError> {
        select_proposer_v2(state, staking, self.height, self.round)
    }

    pub fn make_proposal(&mut self, input: ProposalInputs<'_>) -> Result<Outbound, ExecutionError> {
        let expected = self.proposer_for_round(input.state, input.staking)?;
        if *input.proposer_id != expected {
            return Err(ExecutionError::InvalidProposer {
                expected,
                got: *input.proposer_id,
            });
        }

        let (_, _, state_hash) =
            execute_proposal_v2(
                input.state,
                input.staking,
                &input.transactions,
                input.proposer_id,
                self.height,
            )?;

        let block = Block {
            parent_hash: input.parent_hash,
            height: self.height,
            epoch: input.staking.epoch,
            protocol_version: ProtocolVersion::V2.as_u64(),
            round: self.round,
            proposer_id: *input.proposer_id,
            transactions: input.transactions,
            signatures: vec![],
            state_hash,
            timestamp: input.timestamp,
        };

        let proposal = Proposal {
            height: self.height,
            round: self.round,
            block: block.clone(),
            proposer_id: *input.proposer_id,
            signature: axiom_primitives::Signature([0u8; 64]),
        };

        let mut signed = proposal;
        signed.signature = sign_proposal(input.proposer_key, &signed);

        let block_hash = compute_block_hash(&block);
        self.store_proposal(signed.clone(), block_hash);
        self.step = Step::Prevote;

        Ok(Outbound::Proposal(signed))
    }

    pub fn on_proposal(
        &mut self,
        state: &State,
        staking: &StakingState,
        proposal: Proposal,
    ) -> Result<Vec<Outbound>, ExecutionError> {
        if proposal.height != self.height {
            return Ok(vec![]);
        }

        if proposal.round < self.round {
            return Ok(vec![]);
        }

        if proposal.round > self.round {
            // Advance to the proposer's round so we can participate.
            self.round = proposal.round;
            self.step = Step::Proposal;
        }

        if proposal.round == self.round {
            self.step = Step::Prevote;
        }

        verify_proposal(&proposal).map_err(ExecutionError::CryptoError)?;

        let expected = select_proposer_v2(state, staking, proposal.height, proposal.round)?;
        if proposal.proposer_id != expected {
            return Err(ExecutionError::InvalidProposer {
                expected,
                got: proposal.proposer_id,
            });
        }

        if proposal.block.height != proposal.height {
            return Err(ExecutionError::InvalidHeight {
                expected: proposal.height,
                got: proposal.block.height,
            });
        }
        if proposal.block.round != proposal.round {
            return Err(ExecutionError::InvalidRound {
                expected: proposal.round,
                got: proposal.block.round,
            });
        }
        if proposal.block.proposer_id != proposal.proposer_id {
            return Err(ExecutionError::InvalidProposer {
                expected: proposal.proposer_id,
                got: proposal.block.proposer_id,
            });
        }

        if ProtocolVersion::for_height(proposal.block.height) != ProtocolVersion::V2
            || proposal.block.protocol_version != ProtocolVersion::V2.as_u64()
        {
            return Err(ExecutionError::InvalidProtocolVersion {
                expected: ProtocolVersion::V2.as_u64(),
                got: proposal.block.protocol_version,
            });
        }

        let expected_epoch = if proposal.height == V2_ACTIVATION_HEIGHT && staking.is_empty() {
            0
        } else {
            staking.epoch
        };
        if proposal.block.epoch != expected_epoch {
            return Err(ExecutionError::InvalidEpoch {
                expected: expected_epoch,
                got: proposal.block.epoch,
            });
        }

        let (_, _, expected_hash) = execute_proposal_v2(
            state,
            staking,
            &proposal.block.transactions,
            &proposal.proposer_id,
            proposal.height,
        )?;

        if expected_hash != proposal.block.state_hash {
            return Err(ExecutionError::StateHashMismatch {
                expected: proposal.block.state_hash,
                computed: expected_hash,
            });
        }

        let block_hash = compute_block_hash(&proposal.block);

        if let Some(existing) = self.proposals_by_round.get(&proposal.round) {
            let existing_hash = self
                .proposal_hash_by_round
                .get(&proposal.round)
                .copied()
                .unwrap_or_else(|| compute_block_hash(&existing.block));
            if existing_hash != block_hash && existing.proposer_id == proposal.proposer_id {
                let evidence = Evidence::DoublePropose {
                    proposal_a: Box::new(existing.clone()),
                    proposal_b: Box::new(proposal.clone()),
                };
                self.double_propose_evidence.push(evidence.clone());
                self.store_proposal(proposal.clone(), block_hash);
                return Ok(vec![Outbound::Evidence(evidence)]);
            }
        }

        self.store_proposal(proposal, block_hash);
        Ok(vec![])
    }

    pub fn make_prevote(
        &mut self,
        _state: &State,
        _staking: &StakingState,
        validator_key: &PrivateKey,
        validator_id: &ValidatorId,
    ) -> Result<Option<Outbound>, ExecutionError> {
        if self.step == Step::Proposal {
            return Ok(None);
        }

        if !self.own_votes_sent.insert((self.height, self.round, VotePhase::Prevote)) {
            return Ok(None);
        }

        if self.lock.height == self.height {
            if let Some(h) = self.lock.block_hash {
                return Ok(Some(Outbound::Vote(self.sign_vote(
                    validator_key,
                    validator_id,
                    VotePhase::Prevote,
                    Some(h),
                ))));
            }
        }

        if let Some(h) = self.proposal_hash_by_round.get(&self.round).copied() {
            return Ok(Some(Outbound::Vote(self.sign_vote(
                validator_key,
                validator_id,
                VotePhase::Prevote,
                Some(h),
            ))));
        }

        Ok(Some(Outbound::Vote(self.sign_vote(
            validator_key,
            validator_id,
            VotePhase::Prevote,
            None,
        ))))
    }

    pub fn on_vote(
        &mut self,
        state: &State,
        staking: &StakingState,
        vote: Vote,
    ) -> Result<Vec<Outbound>, ExecutionError> {
        if vote.height != self.height {
            return Ok(vec![]);
        }

        verify_consensus_vote(&vote).map_err(ExecutionError::CryptoError)?;
        let validator = state
            .get_validator(&vote.validator_id)
            .ok_or(ExecutionError::UnknownValidator {
                id: vote.validator_id,
            })?;
        if !validator.active {
            return Err(ExecutionError::InactiveValidator {
                id: vote.validator_id,
            });
        }

        if vote.round < self.round {
            return Ok(vec![]);
        }

        if vote.round > self.round {
            return Ok(vec![]);
        }

        let mut outs = Vec::new();

        match vote.phase {
            VotePhase::Prevote => {
                let round_votes = self.prevotes.entry(vote.round).or_default();
                if let Some(existing) = round_votes.get(&vote.validator_id) {
                    if existing.block_hash != vote.block_hash {
                        let evidence = Evidence::DoubleVote {
                            vote_a: Box::new(existing.clone()),
                            vote_b: Box::new(vote.clone()),
                        };
                        self.double_vote_evidence.push(evidence.clone());
                        outs.push(Outbound::Evidence(evidence));
                        return Ok(outs);
                    }
                }
                round_votes.insert(vote.validator_id, vote);

                if let Some((value, _power)) =
                    self.find_supermajority(state, staking, VotePhase::Prevote, self.round)
                {
                    self.apply_polka(value);
                    self.step = Step::Precommit;
                }
            }
            VotePhase::Precommit => {
                let round_votes = self.precommits.entry(vote.round).or_default();
                if let Some(existing) = round_votes.get(&vote.validator_id) {
                    if existing.block_hash != vote.block_hash {
                        let evidence = Evidence::DoubleVote {
                            vote_a: Box::new(existing.clone()),
                            vote_b: Box::new(vote.clone()),
                        };
                        self.double_vote_evidence.push(evidence.clone());
                        outs.push(Outbound::Evidence(evidence));
                        return Ok(outs);
                    }
                }
                round_votes.insert(vote.validator_id, vote);

                // Call find_supermajority once and branch on the result.
                match self.find_supermajority(state, staking, VotePhase::Precommit, self.round) {
                    Some((Some(hash), _)) => {
                        if let Some(block) = self.block_for_hash(hash) {
                            let mut committed = block.clone();
                            committed.signatures = self
                                .precommits
                                .get(&self.round)
                                .into_iter()
                                .flat_map(|m| m.values())
                                .filter(|v| v.block_hash == Some(hash))
                                .map(|v| ValidatorSignature {
                                    validator_id: v.validator_id,
                                    signature: v.signature,
                                })
                                .collect();
                            outs.push(Outbound::CommittedBlock(committed));
                        }
                    }
                    Some((None, _)) => {
                        self.round = self.round.saturating_add(1);
                        self.step = Step::Proposal;
                        // Prune vote maps for rounds more than 1 behind current.
                        let cur = self.round;
                        self.prevotes.retain(|r, _| *r + 1 >= cur);
                        self.precommits.retain(|r, _| *r + 1 >= cur);
                        outs.push(Outbound::AdvanceRound {
                            height: self.height,
                            round: self.round,
                        });
                    }
                    None => {}
                }
            }
        }

        Ok(outs)
    }

    pub fn make_precommit(
        &mut self,
        state: &State,
        staking: &StakingState,
        validator_key: &PrivateKey,
        validator_id: &ValidatorId,
    ) -> Result<Option<Outbound>, ExecutionError> {
        if self.step != Step::Precommit {
            return Ok(None);
        }

        if !self
            .own_votes_sent
            .insert((self.height, self.round, VotePhase::Precommit))
        {
            return Ok(None);
        }

        if let Some((value, _)) =
            self.find_supermajority(state, staking, VotePhase::Prevote, self.round)
        {
            return Ok(Some(Outbound::Vote(self.sign_vote(
                validator_key,
                validator_id,
                VotePhase::Precommit,
                value,
            ))));
        }

        Ok(Some(Outbound::Vote(self.sign_vote(
            validator_key,
            validator_id,
            VotePhase::Precommit,
            None,
        ))))
    }

    fn store_proposal(&mut self, proposal: Proposal, hash: BlockHash) {
        let round = proposal.round;
        self.proposal_block_by_hash
            .insert(hash, proposal.block.clone());
        self.proposals_by_round.insert(round, proposal);
        self.proposal_hash_by_round.insert(round, hash);
    }

    fn sign_vote(
        &self,
        validator_key: &PrivateKey,
        validator_id: &ValidatorId,
        phase: VotePhase,
        block_hash: Option<BlockHash>,
    ) -> Vote {
        let vote = Vote {
            height: self.height,
            round: self.round,
            phase,
            block_hash,
            validator_id: *validator_id,
            signature: axiom_primitives::Signature([0u8; 64]),
        };
        let mut signed = vote;
        signed.signature = sign_consensus_vote(validator_key, &signed);
        signed
    }

    fn effective_stake(&self, state: &State, staking: &StakingState, validator_id: &ValidatorId) -> u64 {
        if let Some(v) = state.get_validator(validator_id) {
            if !v.active {
                return 0;
            }
        } else {
            return 0;
        }

        if let Some(a) = staking.stakes.get(validator_id) {
            return a.0;
        }
        if self.height == V2_ACTIVATION_HEIGHT && staking.is_empty() {
            return V2_MIGRATION_STAKE_PER_VALIDATOR;
        }
        0
    }

    fn total_active_stake(
        &self,
        state: &State,
        staking: &StakingState,
    ) -> Result<u128, ExecutionError> {
        let active = state.active_validators();
        if active.is_empty() {
            return Err(ExecutionError::NoActiveValidators);
        }
        let mut total: u128 = 0;
        for (vid, _) in active {
            total = total
                .checked_add(self.effective_stake(state, staking, vid) as u128)
                .ok_or(ExecutionError::Overflow)?;
        }
        if total == 0 {
            return Err(ExecutionError::NoActiveValidators);
        }
        Ok(total)
    }

    fn vote_power_by_value<'a>(
        &self,
        state: &State,
        staking: &StakingState,
        votes: impl Iterator<Item = &'a Vote>,
    ) -> BTreeMap<Option<BlockHash>, u128> {
        let mut map: BTreeMap<Option<BlockHash>, u128> = BTreeMap::new();
        for v in votes {
            let w = self.effective_stake(state, staking, &v.validator_id) as u128;
            *map.entry(v.block_hash).or_default() += w;
        }
        map
    }

    fn find_supermajority(
        &self,
        state: &State,
        staking: &StakingState,
        phase: VotePhase,
        round: u64,
    ) -> Option<(Option<BlockHash>, u128)> {
        let votes_map = match phase {
            VotePhase::Prevote => self.prevotes.get(&round),
            VotePhase::Precommit => self.precommits.get(&round),
        }?;

        let total_active = self.total_active_stake(state, staking).ok()?;
        let totals = self.vote_power_by_value(state, staking, votes_map.values());

        for (value, power) in totals {
            if power * 3 > total_active * 2 {
                return Some((value, power));
            }
        }

        None
    }

    fn apply_polka(&mut self, value: Option<BlockHash>) {
        if self.lock.height != self.height {
            self.lock.height = self.height;
            self.lock.round = self.round;
            self.lock.block_hash = value;
            return;
        }

        if self.round > self.lock.round {
            self.lock.round = self.round;
            self.lock.block_hash = value;
            return;
        }

        if self.round == self.lock.round && self.lock.block_hash.is_none() {
            self.lock.block_hash = value;
        }
    }

    fn block_for_hash(&self, hash: BlockHash) -> Option<&Block> {
        self.proposal_block_by_hash.get(&hash)
    }
}
