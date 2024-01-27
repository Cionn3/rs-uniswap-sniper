use ethers::prelude::*;
use std::str::FromStr;
use ethers::abi::Tokenizable;
use ethabi::RawLog;
use revm::EVM;
use crate::forked_db::fork_db::ForkDB;
use crate::forked_db::{ match_output, match_output_reverted };
use crate::oracles::block_oracle::BlockInfo;
use revm::primitives::{ TransactTo, Log };
use crate::utils::{ helpers::*, types::structs::pool::Pool };
use revm::primitives::{ Address as rAddress, U256 as rU256 };

use crate::utils::evm::insp::access_list::AccessListInspector;
use crate::utils::abi::{ ERC20_BALANCE_OF, TOKEN0, TOKEN1, V2_SWAP_EVENT, TRANSFER_EVENT, encode_swap };
use crate::utils::constants::*;

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
        let token_balance = get_erc20_balance(
            pool.token_1,
            *CONTRACT_ADDRESS,
            evm
        )?;

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

pub fn setup_evm(evm: &mut EVM<ForkDB>, next_block: &BlockInfo) {
    evm.env.block.number = rU256::from(next_block.number.as_u64());
    evm.env.block.timestamp = next_block.timestamp.into();
    evm.env.block.basefee = next_block.base_fee.into();
    evm.env.block.coinbase = rAddress
        ::from_str("0xDecafC0FFEe15BAD000000000000000000000000")
        .unwrap();
    // disable some checks
    evm.env.cfg.disable_base_fee = true;
    evm.env.cfg.disable_block_gas_limit = true;
    evm.env.cfg.disable_balance_check = true;

    // some fields that are the same for all calls
    evm.env.tx.gas_price = next_block.base_fee.into();
    evm.env.tx.gas_limit = 1000000;
    evm.env.tx.value = rU256::ZERO;
}

pub fn get_erc20_balance(
    token: Address,
    owner: Address,
    evm: &mut EVM<ForkDB>
) -> Result<U256, anyhow::Error> {
    evm.env.tx.caller = CALLER_ADDRESS.0.into();
    evm.env.tx.transact_to = TransactTo::Call(token.0.into());
    evm.env.tx.data = ERC20_BALANCE_OF.encode("balanceOf", owner).unwrap().0;

    let result = evm.transact_ref()?.result;

    let output = match_output(result)?;

    let bal = ERC20_BALANCE_OF.decode_output("balanceOf", &output)?;

    Ok(bal)
}

// simulate a token call to the pool address
// returns token0 and token1
fn get_tokens_from_pool(
    pool_address: H160,
    evm: &mut EVM<ForkDB>
) -> Result<(H160, H160), anyhow::Error> {
    evm.env.tx.caller = CALLER_ADDRESS.0.into();
    evm.env.tx.transact_to = TransactTo::Call(pool_address.0.into());
    evm.env.tx.data = TOKEN0.encode("token0", ()).unwrap().0;
    evm.env.tx.value = rU256::ZERO;

    let result = evm.transact_ref()?.result;

    let output = match_output(result)?;

    let token0 = TOKEN0.decode_output("token0", &output)?;

    evm.env.tx.data = TOKEN1.encode("token1", ()).unwrap().0;

    let result = evm.transact_ref()?.result;

    let output = match_output(result)?;

    let token1 = TOKEN1.decode_output("token1", &output)?;

    Ok((token0, token1))
}


/// Simulates a call without any inspectors
/// Returns 'is_reverted, logs, gas_used'
pub fn sim_call(
    caller: Address,
    transact_to: Address,
    call_data: Bytes,
    apply_changes: bool,
    evm: &mut EVM<ForkDB>
) -> Result<(bool, Vec<Log>, u64), anyhow::Error> {
    evm.env.tx.caller = caller.0.into();
    evm.env.tx.transact_to = TransactTo::Call(transact_to.0.into());
    evm.env.tx.data = call_data.0;

    let result;

    if apply_changes {
        result = evm.transact_commit()?;
    } else {
        result = evm.transact_ref()?.result;
    }

    let logs = result.logs();

    let gas_used = result.gas_used();

    let is_reverted = match_output_reverted(result);

    Ok((is_reverted, logs, gas_used))
}





// get the real amount of tokens we are going to receive from the swap
// returns real amount and amount from swap
pub fn get_real_amount_from_logs(
    logs: Vec<Log>,
    pool_address: H160,
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
            let Ok(decoded_log) = V2_SWAP_EVENT.parse_log(RawLog {
                topics: converted_topics.clone(),
                data: log.data.clone().to_vec(),
            })
        {
            swap_opt = Some(decoded_log);
        }

        // get all transfer logs
        if
            let Ok(decoded_log) = TRANSFER_EVENT.parse_log(RawLog {
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

        if from == pool_address && to == *CONTRACT_ADDRESS {
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
