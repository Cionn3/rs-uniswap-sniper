pub mod simulate;
pub use simulate::*;

pub mod access_list;
pub use access_list::*;

use ethers::prelude::*;
use std::str::FromStr;
use ethers::abi::parse_abi;
use ethers::abi::Tokenizable;
use ethabi::{ RawLog, Event };
use ethers::types::transaction::eip2930::AccessList;
use revm::EVM;
use crate::forked_db::fork_db::ForkDB;
use crate::oracles::block_oracle::BlockInfo;
use revm::primitives::{ ExecutionResult, Output, TransactTo, AccountInfo, Log };
use revm::db::{ CacheDB, EmptyDB };
use crate::utils::{ helpers::*, types::structs::Pool };
use std::sync::Arc;
use revm::primitives::{ Address as rAddress, Bytecode, U256 as rU256, B256 };
use anyhow::anyhow;

// Setup evm blockstate
//
// Arguments:
// * `&mut evm`: mutable refernece to `EVM<ForkDB>` instance which we want to modify
// * `&next_block`: reference to `BlockInfo` of next block to set values against
//
// Returns: This function returns nothing
pub fn setup_block_state(evm: &mut EVM<ForkDB>, next_block: &BlockInfo) {
    evm.env.block.number = rU256::from(next_block.number.as_u64());
    evm.env.block.timestamp = next_block.timestamp.into();
    evm.env.block.basefee = next_block.base_fee.into();
    // use something other than default
    evm.env.block.coinbase = rAddress
        ::from_str("0xDecafC0FFEe15BAD000000000000000000000000")
        .unwrap();
}

// simulate a token call to the pool address
// returns token0 and token1
fn simulate_token_call(
    pool_address: H160,
    evm: &mut EVM<ForkDB>
) -> Result<(H160, H160), anyhow::Error> {
    let token_0_call = BaseContract::from(
        parse_abi(&["function token0() external view returns (address)"]).unwrap()
    );
    let token_1_call = BaseContract::from(
        parse_abi(&["function token1() external view returns (address)"]).unwrap()
    );

    // get the token0 address

    evm.env.tx.caller = crate::utils::helpers::get_my_address().0.into();
    evm.env.tx.transact_to = TransactTo::Call(pool_address.0.into());
    evm.env.tx.data = token_0_call.encode("token0", ()).unwrap().0;
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 5000000;

    let result = match evm.transact_ref() {
        Ok(result) => result.result,
        Err(e) => {
            return Err(anyhow!("Error when getting balance: {:?}", e));
        }
    };

    let output: Bytes = match result {
        ExecutionResult::Success { output, .. } =>
            match output {
                Output::Call(o) => o.into(),
                Output::Create(o, _) => o.into(),
            }
        ExecutionResult::Revert { output, .. } => {
            return Err(anyhow!("Revert when getting token0: {:?}", output));
        }
        ExecutionResult::Halt { reason, .. } => {
            return Err(anyhow!("Halted when getting token0: {:?}", reason));
        }
    };

    let token_0 = match token_0_call.decode_output("token0", &output) {
        Ok(token) => token,
        Err(e) => {
            return Err(anyhow!("Failed to decode token0: {:?}", e));
        }
    };

    // now get the token1

    evm.env.tx.caller = crate::utils::helpers::get_my_address().0.into();
    evm.env.tx.transact_to = TransactTo::Call(pool_address.0.into());
    evm.env.tx.data = token_1_call.encode("token1", ()).unwrap().0;
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 5000000;

    let result = match evm.transact_ref() {
        Ok(result) => result.result,
        Err(e) => {
            return Err(anyhow!("Error when getting balance: {:?}", e));
        }
    };

    let output: Bytes = match result {
        ExecutionResult::Success { output, .. } =>
            match output {
                Output::Call(o) => o.into(),
                Output::Create(o, _) => o.into(),
            }
        ExecutionResult::Revert { output, .. } => {
            return Err(anyhow!("Revert when getting token1: {:?}", output));
        }
        ExecutionResult::Halt { reason, .. } => {
            return Err(anyhow!("Halted when getting token1: {:?}", reason));
        }
    };

    let token_1 = match token_1_call.decode_output("token1", &output) {
        Ok(token) => token,
        Err(e) => {
            return Err(anyhow!("Failed to decode token1: {:?}", e));
        }
    };

    Ok((token_0, token_1))
}

pub fn get_token_balance(
    token: Address,
    owner: Address,
    next_block: &BlockInfo,
    fork_db: ForkDB
) -> Result<U256, anyhow::Error> {

    let mut evm = revm::EVM::new();
    evm.database(fork_db.clone());

    // setup the next block state
    setup_block_state(&mut evm, next_block);

   let balance = get_balance_of_evm(token, owner, next_block, &mut evm)?;
   Ok(balance)
}

// Get token balance
//
// Arguments:
// * `token`: erc20 token to query
// * `owner`: address to find balance of
// * `next_block`: block to query balance at
// * `evm`: evm instance to run query on
//
// Returns:
// `Ok(balance: U256)` if successful, Err(SimulationError) otherwise
pub fn get_balance_of_evm(
    token: Address,
    owner: Address,
    next_block: &BlockInfo,
    evm: &mut EVM<ForkDB>
) -> Result<U256, anyhow::Error> {
    let erc20 = BaseContract::from(
        parse_abi(&["function balanceOf(address) external returns (uint)"]).unwrap()
    );

    evm.env.tx.transact_to = TransactTo::Call(token.0.into());
    evm.env.tx.data = erc20.encode("balanceOf", owner).unwrap().0;
    evm.env.tx.caller = crate::utils::helpers::get_my_address().0.into();
    evm.env.tx.gas_price = next_block.base_fee.into();
    evm.env.tx.gas_limit = 1000000;
    evm.env.tx.value = rU256::ZERO;

    let result = match evm.transact_ref() {
        Ok(result) => result.result,
        Err(e) => {
            return Err(anyhow!("Error when getting balance: {:?}", e));
        }
    };

    let output: Bytes = match result {
        ExecutionResult::Success { output, .. } =>
            match output {
                Output::Call(o) => o.into(),
                Output::Create(o, _) => o.into(),
            }
        ExecutionResult::Revert { output, .. } => {
            return Err(anyhow!("Err when getting balance: {:?}", output));
        }
        ExecutionResult::Halt { reason, .. } => {
            return Err(anyhow!("Halted when getting balance: {:?}", reason));
        }
    };

    match erc20.decode_output("balanceOf", &output) {
        Ok(tokens) => {
            return Ok(tokens);
        }
        Err(e) => {
            return Err(anyhow!("Failed to decode balanceOf: {:?}", e));
        }
    }
}

// commits a pending tx
fn commit_pending_tx(evm: &mut EVM<ForkDB>, tx: &Transaction) -> Result<(), anyhow::Error> {
    // disable checks
    evm.env.cfg.disable_base_fee = true;
    evm.env.cfg.disable_block_gas_limit = true;
    evm.env.cfg.disable_balance_check = true;

    evm.env.tx.caller = rAddress::from_slice(&tx.from.0);
    evm.env.tx.transact_to = TransactTo::Call(rAddress::from_slice(&tx.to.unwrap_or_default().0));
    evm.env.tx.data = tx.input.0.clone();
    evm.env.tx.value = tx.value.into();
    evm.env.tx.gas_limit = 5000000;

    let res = match evm.transact_commit() {
        Ok(result) => result,
        Err(e) => {
            return Err(anyhow!("Failed to commit pending tx: {:?}", e));
        }
    };

    let _output: Bytes = match res {
        ExecutionResult::Success { output, .. } =>
            match output {
                Output::Call(o) => o.into(),
                Output::Create(o, _) => o.into(),
            }
        ExecutionResult::Revert { output, .. } => {
            return Err(anyhow!("Pending tx reverted: {:?}", output));
        }
        ExecutionResult::Halt { reason, .. } => {
            return Err(anyhow!("Pending tx halted: {:?}", reason));
        }
    };

    Ok(())
}

// commit our tx
fn commit_tx(
    evm: &mut EVM<ForkDB>,
    call_data: Vec<u8>,
    next_block: &BlockInfo
) -> Result<(), anyhow::Error> {
    // setup evm for swap
    evm.env.tx.caller = get_my_address().into();
    evm.env.tx.transact_to = TransactTo::Call(get_snipe_contract_address().0.into());
    evm.env.tx.data = call_data.into();
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 1000000;
    evm.env.tx.gas_price = next_block.base_fee.into();

    let result = match evm.transact_commit() {
        Ok(res) => res,
        Err(e) => {
            return Err(anyhow!("Error Commiting tx: {:?}", e));
        }
    };

    let _output: Bytes = match result {
        ExecutionResult::Success { output, .. } =>
            match output {
                Output::Call(o) => o.into(),
                Output::Create(o, _) => o.into(),
            }
        ExecutionResult::Revert { output, .. } => {
            return Err(anyhow!("Sell Tx Reverted: {:?}", output));
        }
        ExecutionResult::Halt { reason, .. } => {
            return Err(anyhow!("Sell Tx Halted: {:?}", reason));
        }
    };

    Ok(())
}

// commit tx and return logs
// returns a bool, true if tx is reverted, false otherwise
// returns a vector of logs
fn commit_tx_and_return_logs(
    evm: &mut EVM<ForkDB>,
    call_data: Vec<u8>,
    next_block: &BlockInfo,
    token: &Address,
    caller: Address,
    commit_to_db: bool
) -> Result<(bool, Vec<Log>), anyhow::Error> {
    evm.env.tx.caller = caller.into();
    evm.env.tx.transact_to = TransactTo::Call(get_snipe_contract_address().0.into());
    evm.env.tx.data = call_data.into();
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 1000000;
    evm.env.tx.gas_price = next_block.base_fee.into();

    let result;
    
    if commit_to_db {
    // simulate tx and write to db
    result = match evm.transact_commit() {
        Ok(result) => result,
        Err(e) => {
            return Err(anyhow!("Failed to commit Tax Check: {:?}", e));
        }
    };
} else {
    // simulate tx without writing to db
    result = match evm.transact_ref() {
        Ok(result) => result.result,
        Err(e) => {
            return Err(anyhow!("Failed to commit Tax Check: {:?}", e));
        }
    };
}

    let logs = result.logs();

    // define a bool if the tx is reverted
    let is_tx_reverted = match result {
        ExecutionResult::Success { .. } => false,
        ExecutionResult::Revert { output, .. } => {
            log::error!("Token {:?} Tx Reverted: {:?}", token, output);
            true
        }
        ExecutionResult::Halt { .. } => true,
    };

    Ok((is_tx_reverted, logs))
}

// commit tx with access list inspector
// returns access list and gas used
fn commit_tx_with_access_list(
    evm: &mut EVM<ForkDB>,
    call_data: Vec<u8>,
    next_block: &BlockInfo
) -> Result<(AccessList, u64, Vec<Log>), anyhow::Error> {
    // setup evm for swap
    evm.env.tx.caller = get_my_address().into();
    evm.env.tx.transact_to = TransactTo::Call(get_snipe_contract_address().0.into());
    evm.env.tx.data = call_data.clone().into();
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 1000000;
    evm.env.tx.gas_price = next_block.base_fee.into();

    // get access list
    let mut access_list_inspector = AccessListInspector::new(
        get_my_address(),
        get_snipe_contract_address().0.into()
    );

    // sim tx without writing to db
    evm.inspect_ref(&mut access_list_inspector)
        .map_err(|e| anyhow!("Error when getting Access List: {:?}", e))
        .unwrap();

    // get access list
    let access_list = access_list_inspector.into_access_list();

    evm.env.tx.access_list = access_list.clone();

    // run once again and commit changes to db
    let tx_result = match evm.transact_commit() {
        Ok(result) => result,
        Err(e) => {
            return Err(anyhow!("Error when commiting Final Simulation: {:?}", e));
        }
    };

    let logs = tx_result.logs();
    let gas_used = tx_result.gas_used();

    Ok((convert_access_list(access_list), gas_used, logs))
}

// get the real amount of tokens we are going to receive from the swap
// returns real amount and amount from swap
pub fn get_real_amount_from_logs(
    logs: Vec<Log>,
    pool_address: H160,
    swap_event: Event,
    transfer_event: Event
) -> Result<(U256, U256), anyhow::Error> {
    // hold decoded events
    let mut swap_opt = None;
    let mut transfer_logs = Vec::new();

    for log in &logs {
        // convert logs topics to H256
        let converted_topics: Vec<_> = log.topics
            .iter()
            .map(|b256| H256::from_slice(b256.as_bytes()))
            .collect();

        // check for the swap event
        if
            let Ok(decoded_log) = swap_event.parse_log(RawLog {
                topics: converted_topics.clone(),
                data: log.data.clone().to_vec(),
            })
        {
            swap_opt = Some(decoded_log);
        }

        // push all transfer logs to the vector
        if
            let Ok(decoded_log) = transfer_event.parse_log(RawLog {
                topics: converted_topics.clone(),
                data: log.data.clone().to_vec(),
            })
        {
            transfer_logs.push(decoded_log);
        }
    }

    // if for some reason we dont get the swap log (unlikely) return err
    let swap_log = match swap_opt {
        Some(swap) => swap,
        None => {
            return Err(anyhow!("Swap event not found"));
        }
    };

    // same for transfer
    if transfer_logs.is_empty() {
        return Err(anyhow!("Transfer events not found"));
    }

    // get the amount of tokens we are going to receive from the swap_log
    let amount_0_out = swap_log.params[3].value.clone().into_token().into_uint().unwrap();
    let amount_1_out = swap_log.params[4].value.clone().into_token().into_uint().unwrap();

    // the amount of tokens should be either amount 0 out or amount 1 out
    // which ever is not zero is the tokens we receive
    let token_amount_from_swap = if amount_0_out == U256::zero() { amount_1_out } else { amount_0_out };
    let mut got_amount = false;
    let mut real_amount = U256::zero();

    // now we find the transfer log that sends the tokens to our contract address
    for log in transfer_logs {
        // from address must be the pool address
        let from = log.params[0].value.clone().into_token().into_address().unwrap();
        // to address must be our contract address
        let to = log.params[1].value.clone().into_token().into_address().unwrap();

        if from == pool_address && to == get_snipe_contract_address() {
            // get the amount of tokens
            real_amount = log.params[2].value.clone().into_token().into_uint().unwrap();
            got_amount = true;
        }
    }

    if !got_amount {
        return Err(anyhow!("Something broke! We didnt find the token amount from transfer logs"));
    }

    Ok((real_amount, token_amount_from_swap))
}



// inserts pool storage into cache db
pub async fn insert_pool_storage(
    client: Arc<Provider<Ws>>,
    pool: Pool,
    fork_block: Option<BlockId>
) -> Result<CacheDB<EmptyDB>, ProviderError> {
    let mut cache_db = CacheDB::new(EmptyDB::default());

    let slot_8 = rU256::from(8);

    // fetch the acc info of pool
    let pool_acc_info = get_acc_info(client.clone(), pool.address, fork_block).await?;

    cache_db.insert_account_info(pool.address.into(), pool_acc_info);

    let pool_value = get_storage(client.clone(), pool.address, slot_8, fork_block).await?;

    cache_db.insert_account_storage(pool.address.into(), slot_8, pool_value).unwrap();

    Ok(cache_db)
}

pub async fn get_acc_info(
    client: Arc<Provider<Ws>>,
    address: Address,
    fork_block: Option<BlockId>
) -> Result<AccountInfo, ProviderError> {
    let nonce = client.get_transaction_count(address, fork_block).await?;
    let balance = client.get_balance(address, fork_block).await?;
    let code = client.get_code(address, fork_block).await?;

    let acc_info = AccountInfo::new(balance.into(), nonce.as_u64(), B256::default(), Bytecode::new_raw(code.0));

    Ok(acc_info)
}

pub async fn get_storage(
    client: Arc<Provider<Ws>>,
    address: Address,
    slot: rU256,
    fork_block: Option<BlockId>
) -> Result<rU256, ProviderError> {
    let slot = H256::from(slot.to_be_bytes());
    let storage = client.get_storage_at(address, slot, fork_block).await.unwrap();
    Ok(rU256::from_be_bytes(storage.to_fixed_bytes()))
}
