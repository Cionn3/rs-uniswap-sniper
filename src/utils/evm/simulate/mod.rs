use ethers::prelude::*;
use std::str::FromStr;
use ethers::abi::parse_abi;
use ethers::abi::Tokenizable;
use ethabi::{ RawLog, Event };
use revm::EVM;
use crate::forked_db::fork_db::ForkDB;
use crate::oracles::block_oracle::BlockInfo;
use revm::primitives::{ ExecutionResult, Output, TransactTo, Log };
use crate::utils::{ helpers::*, types::structs::pool::Pool };
use revm::primitives::{ Address as rAddress, U256 as rU256 };
use ethers::types::transaction::eip2930::AccessList;
use crate::utils::evm::insp::access_list::AccessListInspector;
use anyhow::anyhow;

use super::insp::access_list::convert_access_list;

pub mod sim;

// helper function for generate tx data
// creates calldata whether we buy or sell

pub fn generate_call_data(
    pool: &Pool,
    amount_in: U256,
    minimum_received: U256,
    do_we_buy: bool,
    next_block: &BlockInfo,
    evm: &mut EVM<ForkDB>
) -> Result<Vec<u8>, anyhow::Error> {
    let call_data;

    // if we buy
    if do_we_buy {
        call_data = encode_swap(
            pool.token_0, // weth
            pool.token_1, // shitcoin
            pool.address,
            amount_in,
            minimum_received // expected amount
        );
    } else {
        // we sell

        // get the token balance in the contract
        let token_balance = get_balance_of_evm(
            pool.token_1,
            get_snipe_contract_address(),
            next_block,
            evm
        )?;
        log::info!(
            "Sell triggered, token balance: {:?} for token {:?}",
            token_balance,
            pool.token_1
        );

        call_data = encode_swap(
            pool.token_1, // shitcoin
            pool.token_0, // weth
            pool.address,
            token_balance,
            minimum_received // expected amount
        );
    }

    Ok(call_data)
}

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

// disable checks for easier simulations
pub fn disable_checks(evm: &mut EVM<ForkDB>) {
    evm.env.cfg.disable_base_fee = true;
    evm.env.cfg.disable_block_gas_limit = true;
    evm.env.cfg.disable_balance_check = true;
}

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

    let result = evm.transact_ref().unwrap().result;

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

    let result = evm.transact_ref().unwrap().result;

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

// commits a tx without any inspectors
// returns a bool to indicate if the tx was reverted
// returns a vector of logs
// returns gas used
#[allow(unused_variables)]
fn commit_tx(
    evm: &mut EVM<ForkDB>,
    call_data: Bytes,
    caller: Address,
    transact_to: Address,
    commit_to_db: bool,
    next_block: &BlockInfo
) -> Result<(bool, Vec<Log>, u64), anyhow::Error> {
    // setup evm for swap
    evm.env.tx.caller = caller.into();
    evm.env.tx.transact_to = TransactTo::Call(transact_to.0.into());
    evm.env.tx.data = call_data.0;
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 1000000;
    evm.env.tx.gas_price = next_block.base_fee.into();

    let result;

    if commit_to_db {
        // apply state changes to db
        result = evm.transact_commit()?;
    } else {
        // dont apply state changes to db
        result = evm.transact_ref().unwrap().result;
    }

    let logs = result.logs();
    let gas_used = result.gas_used();

    // define a bool if the tx is reverted
    let is_tx_reverted = match result {
        ExecutionResult::Success { .. } => false,
        ExecutionResult::Revert { output, .. } => {
            true
        }
        ExecutionResult::Halt { .. } => true,
    };

    Ok((is_tx_reverted, logs, gas_used))
}

// commit tx with access list inspector
// returns access list, logs, and gas used
fn commit_tx_with_access_list(
    evm: &mut EVM<ForkDB>,
    call_data: Bytes,
    caller: Address,
    transact_to: Address,
    commit_to_db: bool,
    next_block: &BlockInfo
) -> Result<(AccessList, Vec<Log>, u64), anyhow::Error> {
    // setup evm for swap
    evm.env.tx.caller = caller.into();
    evm.env.tx.transact_to = TransactTo::Call(transact_to.0.into());
    evm.env.tx.data = call_data.0;
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 1000000;
    evm.env.tx.gas_price = next_block.base_fee.into();

    let result;

    let mut access_list_inspector = AccessListInspector::new(
        get_my_address(),
        get_snipe_contract_address().0.into()
    );

    // sim tx to get access list
    evm.inspect_ref(&mut access_list_inspector)
        .map_err(|e| anyhow!("Error when getting Access List: {:?}", e))
        .unwrap();

        // get access list
        let access_list = access_list_inspector.into_access_list();
        // set access list to evm
        evm.env.tx.access_list = access_list.clone();

    // commit tx
    if commit_to_db {
        // apply state changes to db
        result = evm.transact_commit()?;
    } else {
        // dont apply state changes to db
        result = evm.transact_ref().unwrap().result;
    }

    let logs = result.logs();
    let gas_used = result.gas_used();

    Ok((convert_access_list(access_list), logs, gas_used))
}







// commits a pending tx and apply state changes to db
fn commit_pending_tx(evm: &mut EVM<ForkDB>, tx: &Transaction) -> Result<(), anyhow::Error> {
    evm.env.tx.caller = rAddress::from_slice(&tx.from.0);
    evm.env.tx.transact_to = TransactTo::Call(rAddress::from_slice(&tx.to.unwrap_or_default().0));
    evm.env.tx.data = tx.input.0.clone();
    evm.env.tx.value = tx.value.into();
    evm.env.tx.gas_limit = 5000000;

    let res = evm.transact_commit()?;

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
    let token_amount_from_swap = if amount_0_out == U256::zero() {
        amount_1_out
    } else {
        amount_0_out
    };
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