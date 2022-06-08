use crate::bellatrix as spec;

use crate::primitives::{Gwei, Hash32, GENESIS_EPOCH};
use crate::state_transition::{Context, Result};
use spec::{
    get_next_sync_committee, process_deposit, BeaconBlockBody, BeaconBlockHeader, BeaconState,
    Deposit, DepositData, Eth1Data, ExecutionPayloadHeader, Fork, DEPOSIT_CONTRACT_TREE_DEPTH,
};
use ssz_rs::prelude::*;

// @dev consider making `phase0` public to reduce duplicate definition
const DEPOSIT_DATA_LIST_BOUND: usize = 2usize.pow(DEPOSIT_CONTRACT_TREE_DEPTH as u32);

pub fn initialize_beacon_state_from_eth1<
    const SLOTS_PER_HISTORICAL_ROOT: usize,
    const HISTORICAL_ROOTS_LIMIT: usize,
    const ETH1_DATA_VOTES_BOUND: usize,
    const VALIDATOR_REGISTRY_LIMIT: usize,
    const EPOCHS_PER_HISTORICAL_VECTOR: usize,
    const EPOCHS_PER_SLASHINGS_VECTOR: usize,
    const MAX_VALIDATORS_PER_COMMITTEE: usize,
    const SYNC_COMMITTEE_SIZE: usize,
    const MAX_PROPOSER_SLASHINGS: usize,
    const MAX_ATTESTER_SLASHINGS: usize,
    const MAX_ATTESTATIONS: usize,
    const MAX_DEPOSITS: usize,
    const MAX_VOLUNTARY_EXITS: usize,
    const BYTES_PER_LOGS_BLOOM: usize,
    const MAX_EXTRA_DATA_BYTES: usize,
    const MAX_BYTES_PER_TRANSACTION: usize,
    const MAX_TRANSACTIONS_PER_PAYLOAD: usize,
>(
    eth1_block_hash: Hash32,
    eth1_timestamp: u64,
    deposits: &mut [Deposit],
    execution_payload_header: ExecutionPayloadHeader<
        BYTES_PER_LOGS_BLOOM,
        MAX_EXTRA_DATA_BYTES,
        MAX_BYTES_PER_TRANSACTION,
        MAX_TRANSACTIONS_PER_PAYLOAD,
    >,
    context: &Context,
) -> Result<
    BeaconState<
        SLOTS_PER_HISTORICAL_ROOT,
        HISTORICAL_ROOTS_LIMIT,
        ETH1_DATA_VOTES_BOUND,
        VALIDATOR_REGISTRY_LIMIT,
        EPOCHS_PER_HISTORICAL_VECTOR,
        EPOCHS_PER_SLASHINGS_VECTOR,
        MAX_VALIDATORS_PER_COMMITTEE,
        SYNC_COMMITTEE_SIZE,
        BYTES_PER_LOGS_BLOOM,
        MAX_EXTRA_DATA_BYTES,
        MAX_BYTES_PER_TRANSACTION,
        MAX_TRANSACTIONS_PER_PAYLOAD,
    >,
> {
    let fork = Fork {
        previous_version: context.altair_fork_version,
        current_version: context.altair_fork_version,
        epoch: GENESIS_EPOCH,
    };
    let eth1_data = Eth1Data {
        block_hash: eth1_block_hash.clone(),
        deposit_count: deposits.len() as u64,
        ..Default::default()
    };
    let mut latest_block_body = BeaconBlockBody::<
        MAX_PROPOSER_SLASHINGS,
        MAX_VALIDATORS_PER_COMMITTEE,
        MAX_ATTESTER_SLASHINGS,
        MAX_ATTESTATIONS,
        MAX_DEPOSITS,
        MAX_VOLUNTARY_EXITS,
        SYNC_COMMITTEE_SIZE,
        BYTES_PER_LOGS_BLOOM,
        MAX_EXTRA_DATA_BYTES,
        MAX_BYTES_PER_TRANSACTION,
        MAX_TRANSACTIONS_PER_PAYLOAD,
    >::default();
    let body_root = latest_block_body.hash_tree_root()?;
    let latest_block_header = BeaconBlockHeader {
        body_root,
        ..Default::default()
    };
    let randao_mixes = Vector::from_iter(
        std::iter::repeat(eth1_block_hash).take(context.epochs_per_historical_vector as usize),
    );
    let mut state = BeaconState {
        genesis_time: eth1_timestamp + context.genesis_delay,
        fork,
        eth1_data,
        latest_block_header,
        randao_mixes,
        ..Default::default()
    };
    let mut leaves = List::<DepositData, DEPOSIT_DATA_LIST_BOUND>::default();
    for deposit in deposits.iter_mut() {
        leaves.push(deposit.data.clone());
        state.eth1_data.deposit_root = leaves.hash_tree_root()?;
        process_deposit(&mut state, deposit, context)?;
    }
    for i in 0..state.validators.len() {
        let validator = &mut state.validators[i];
        let balance = state.balances[i];
        let effective_balance = Gwei::min(
            balance - balance % context.effective_balance_increment,
            context.max_effective_balance,
        );
        validator.effective_balance = effective_balance;
        if validator.effective_balance == context.max_effective_balance {
            validator.activation_eligibility_epoch = GENESIS_EPOCH;
            validator.activation_epoch = GENESIS_EPOCH;
        }
    }

    // Set genesis validators root for domain separation and chain versioning
    state.genesis_validators_root = state.validators.hash_tree_root()?;

    // Fill in sync committees
    state.current_sync_committee = get_next_sync_committee(&state, context)?;
    state.next_sync_committee = get_next_sync_committee(&state, context)?;

    Ok(state)
}
