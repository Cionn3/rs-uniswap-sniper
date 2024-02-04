use ethers::prelude::*;
use revm::primitives::{
    TransactTo,
    U256 as rU256,
    B160 as rAddress
};
use anyhow::anyhow;
use super::*;
use crate::{ forked_db::fork_db::ForkDB, utils::types::structs::tx_data::TxData };
use crate::oracles::block_oracle::BlockInfo;
use ethers::abi::Tokenizable;
use ethabi::RawLog;

use crate::utils::abi::*;
use crate::utils::types::structs::snipe_tx::SnipeTx;
use crate::utils::types::structs::pool::Pool;

// finds the amount in weth to buy the token
// ** A lot of tokens have min and max buy size

pub fn find_amount_in(
    pool: &Pool,
    next_block: &BlockInfo,
    pending_tx: Option<Transaction>,
    fork_db: ForkDB
) -> Result<U256, anyhow::Error> {
    let mut amount_in = *MAX_BUY_SIZE;
    let decrease_by = U256::from(1000000000000000u128); // 0.001 ETH
    let mut attempts = 0;
    let max_attempts: usize = 100;
    let mut got_amount = false;

    // by default we assume the token has a max buy size
    let mut is_reverted = true;

    // call data
    let mut call_data = encode_swap(
        pool.token_0, // weth
        pool.token_1, // shitcoin
        pool.address,
        amount_in,
        U256::from(0u128)
    );

    // ** a simple while loop to find the amount in

    while is_reverted {
        // setup a new evm instance
        let mut evm = revm::EVM::new();
        evm.database(fork_db.clone());

        // setup the next block state
        setup_evm(&mut evm, next_block);

        if let Some(ref tx) = pending_tx {
            // first simulate and commit the pending tx so we can buy the token
            evm.env.tx.value = tx.value.into();
            let _ = sim_call(tx.from, tx.to.unwrap_or_default(), tx.input.clone(), true, &mut evm)?;
        }

        let result = sim_call(
            *CALLER_ADDRESS,
            *CONTRACT_ADDRESS,
            call_data.clone().into(),
            false,
            &mut evm
        )?;

        if result.is_reverted {
            amount_in = amount_in.saturating_sub(decrease_by);

            if amount_in < *MIN_BUY_SIZE || attempts >= max_attempts {
                break;
            }
            call_data = encode_swap(
                pool.token_0,
                pool.token_1,
                pool.address,
                amount_in,
                U256::from(0u128)
            );
            attempts += 1;
        } else {
            is_reverted = false;
            got_amount = true;
        }
    }

    if !got_amount {
        return Ok(U256::zero());
    }

    return Ok(amount_in);
}

// Checks if the token has taxes
// we use a resonable amount of weth cause of the price impact
// ** We also do HoneyPot checks **
pub fn tax_check(
    pool: &Pool,
    amount_in_weth: U256,
    next_block: &BlockInfo,
    pending_tx: Option<Transaction>,
    fork_db: ForkDB
) -> Result<bool, anyhow::Error> {
    let mut evm = revm::EVM::new();
    evm.database(fork_db.clone());

    // setup the next block state
    setup_evm(&mut evm, next_block);

    // if we have a pending tx simulate it
    if let Some(tx) = pending_tx.clone() {
        // commit the pending tx so we can buy the token
        evm.env.tx.value = tx.value.into();
        let _ = sim_call(tx.from, tx.to.unwrap_or_default(), tx.input.clone(), true, &mut evm)?;
    }

    // ** create the call_data for the swap
    let call_data = encode_swap(
        pool.token_0, // weth
        pool.token_1, // shitcoin
        pool.address,
        amount_in_weth,
        U256::from(0u128)
    );

    let result = sim_call(
        *CALLER_ADDRESS,
        *CONTRACT_ADDRESS,
        call_data.clone().into(),
        false,
        &mut evm
    )?;

    // if the swap is reverted usually there is 2 reasons
    // 1. Trading is not open yet
    // 2. The token has a maximum or minimum buy size which we may not met
    // we return false so we can push it to retry oracle
    if result.is_reverted {
        log::warn!("Buy reverted {:?}", pool.token_1);
        return Ok(false);
    }

    // ** we check the logs to see the actual amount of tokens the pool is gonna send us
    let (real_amount, amount_from_swap) = get_real_amount_from_logs(result.logs, pool.address)?;

    // if the actual amount of tokens is less than 70% of the amount we should receive
    // then we skip the token
    if real_amount < (amount_from_swap * 7) / 10 {
        log::error!("Amount From Swap {:?}", amount_from_swap);
        log::error!("Real Amount {:?}", real_amount);
        return Ok(false);
    }

    // ** Do the sell Transaction **

    // ** create the call_data for the swap
    let call_data = encode_swap(
        pool.token_1, // shitcoin
        pool.token_0, // weth
        pool.address,
        real_amount,
        U256::from(0u128)
    );

    // try to avoid the transfer delay error buy setting the block 1 number further
    evm.env.block.number = rU256::from(next_block.number.as_u64() + 1);
    evm.env.block.timestamp = (next_block.timestamp + U256::from(12u128)).into();

    // ** Simulate sell
    let result = sim_call(
        *CALLER_ADDRESS,
        *CONTRACT_ADDRESS,
        call_data.clone().into(),
        false,
        &mut evm
    )?;

    // see if the tx is revrted
    if result.is_reverted {
        log::warn!("Sell reverted {:?}", pool.token_1);
        return Ok(false);
    }

    // ** check the amount of weth we are going to receive
    let (real_weth_amount, _) = get_real_amount_from_logs(result.logs, pool.address)?;

    // if the actual amount of weth is less than 70% of the amount in weth
    // then we skip the token
    if real_weth_amount < (amount_in_weth * 7) / 10 {
        log::error!("Amount In Weth {}", convert_wei_to_ether(amount_in_weth));
        log::error!("Real Weth Amount out {}", convert_wei_to_ether(real_weth_amount));
        return Ok(false);
    }

    // ** Passed All Checks **
    Ok(true)
}

// ** Generate Call Data **
pub fn generate_tx_data(
    pool: &Pool,
    amount_in_weth: U256,
    next_block: &BlockInfo,
    pending_tx: Option<Transaction>,
    miner_tip: U256,
    frontrun_or_backrun: u8,
    do_we_buy: bool,
    fork_db: ForkDB
) -> Result<(SnipeTx, TxData), anyhow::Error> {
    let mut evm = revm::EVM::new();
    evm.database(fork_db.clone());

    // setup the next block state
    setup_evm(&mut evm, next_block);

    // if we have a pending tx simulate it
    if let Some(ref tx) = pending_tx {
        evm.env.tx.value = tx.value.into();
        let _ = sim_call(tx.from, tx.to.unwrap_or_default(), tx.input.clone(), true, &mut evm)?;
    }

    // generate call data based on whether we buy or sell
    let call_data = generate_call_data(
        pool,
        amount_in_weth,
        U256::zero(), // minimum received
        do_we_buy,
        &mut evm
    )?;

    // ** Generate Access List
    let mut access_list_inspector = AccessListInspector::new(*CALLER_ADDRESS, *CONTRACT_ADDRESS);

    // setup fields
    evm.env.tx.caller = rAddress::from(CALLER_ADDRESS.0);
    evm.env.tx.transact_to = TransactTo::Call(rAddress::from(CONTRACT_ADDRESS.0));
    evm.env.tx.data = call_data.clone().into();

    // sim tx to get access list
    evm
        .inspect_ref(&mut access_list_inspector)
        .map_err(|e| anyhow!("Error generating Access List: {:?}", e))?;

    let access_list = access_list_inspector.into_access_list();

    // set access list to evm
    evm.env.tx.access_list = access_list.clone();
    // simulate call
    let result = sim_call(
        *CALLER_ADDRESS,
        *CONTRACT_ADDRESS,
        call_data.into(),
        false,
        &mut evm
    )?;

    // calculate total gas cost for the transaction
    let gas_cost = (next_block.base_fee + miner_tip) * result.gas_used;

    // get the real amount of tokens received
    let (amount_received, _) = get_real_amount_from_logs(result.logs, pool.address)?;

    let minimum_received =
        (amount_received * U256::from(*BUY_NUMERATOR)) / U256::from(*BUY_DENOMINATOR);

    // encode the call data again with the minimum received
    let call_data = generate_call_data(
        pool,
        amount_in_weth,
        minimum_received,
        do_we_buy,
        &mut evm
    )?;

    // ** Generate SnipeTx and TxData

    let snipe_tx = SnipeTx::new(
        result.gas_used,
        gas_cost,
        *pool,
        amount_in_weth,
        minimum_received,
        *TARGET_AMOUNT_TO_SELL,
        next_block.number
    );

    let tx_data = TxData::new(
        call_data.into(),
        result.gas_used,
        minimum_received,
        pending_tx.unwrap_or(Transaction::default()),
        U256::from(frontrun_or_backrun),
        convert_access_list(access_list)
    );

    Ok((snipe_tx, tx_data))
}

// ** Simulate a sell transactions to get the current amount out of weth
// ** We also use the same function here to simulate a sell after a pending tx which may have changed the state of the contract
pub fn simulate_sell(
    tx: Option<Transaction>,
    pool: Pool,
    next_block: BlockInfo,
    fork_db: ForkDB
) -> Result<U256, anyhow::Error> {
    // setup an evm instance
    let mut evm = revm::EVM::new();
    evm.database(fork_db);

    // setup the next block state
    setup_evm(&mut evm, &next_block);

    // if we have a pending tx simulate it
    if let Some(tx) = tx.clone() {
        evm.env.tx.value = tx.value.into();
        let _ = sim_call(tx.from, tx.to.unwrap_or_default(), tx.input.clone(), true, &mut evm)?;
    }

    // ** get the token balance for the amount_in to sell
    let amount_in = get_erc20_balance(
        pool.token_1, // shitcoin
        *CONTRACT_ADDRESS,
        &mut evm
    )?;

    // ** create the call_data for the swap
    let call_data = encode_swap(
        pool.token_1, // shitcoin
        pool.token_0, // weth
        pool.address,
        amount_in,
        U256::from(0u128)
    );

    // ** Simulate the Sell Transaction
    let result = sim_call(
        *CALLER_ADDRESS,
        *CONTRACT_ADDRESS,
        call_data.clone().into(),
        false,
        &mut evm
    )?;
    

    // if the tx is reverted we return 0
    // cause it will produce no logs
    if result.is_reverted {
        log::warn!("Our tx is reverted, returning 0");
        return Ok(U256::zero());
    }

    // ** get the actual amount of weth we are going to receive from the logs
    let (weth_amount, _) = get_real_amount_from_logs(
        result.logs,
        pool.address,
    )?;

    return Ok(weth_amount);
}

// Profit Taker

pub fn profit_taker(
    next_block: &BlockInfo,
    pool: Pool,
    amount_in: U256,
    fork_db: ForkDB
) -> Result<TxData, anyhow::Error> {
    // setup an evm instance
    let mut evm = revm::EVM::new();
    evm.database(fork_db.clone());

    // setup the next block state
    setup_evm(&mut evm, &next_block);

    // ** a simple way to find out how much tokens to sell
    // ** is to simulate a buy transaction at the current state
    // ** to see how much tokens we get and we will use that amount to sell

    // ** create the call_data for the swap
    let call_data = encode_swap(
        *WETH, // input
        pool.token_1, // output
        pool.address,
        amount_in,
        U256::from(0u128)
    );

    // ** Simulate the Buy Transaction
    let result = sim_call(
        *CALLER_ADDRESS,
        *CONTRACT_ADDRESS,
        call_data.clone().into(),
        false,
        &mut evm
    )?;

    // get the real amount of tokens we are going to receive
    let (mut amount_of_tokens_to_sell, _) = get_real_amount_from_logs(
        result.logs,
        pool.address
    )?;

    // encode the sell call data
    let call_data = encode_swap(
        pool.token_1, // input
        *WETH, // output
        pool.address,
        amount_of_tokens_to_sell,
        U256::from(0u128)
    );

    // ** Generate Access List
    let mut access_list_inspector = AccessListInspector::new(*CALLER_ADDRESS, *CONTRACT_ADDRESS);

    // setup fields
    evm.env.tx.caller = rAddress::from(CALLER_ADDRESS.0);
    evm.env.tx.transact_to = TransactTo::Call(rAddress::from(CONTRACT_ADDRESS.0));
    evm.env.tx.data = call_data.clone().into();

    // sim tx to get access list
    evm
        .inspect_ref(&mut access_list_inspector)
        .map_err(|e| anyhow!("Error generating Access List: {:?}", e))?;

    let access_list = access_list_inspector.into_access_list();

    // set access list to evm
    evm.env.tx.access_list = access_list.clone();

    // ** simulate the sell tx
    let result = sim_call(
        *CALLER_ADDRESS,
        *CONTRACT_ADDRESS,
        call_data.clone().into(),
        false,
        &mut evm
    )?;

    // ** get the amount of weth we are going to receive
    let (real_amount_weth, _) = get_real_amount_from_logs(
        result.logs,
        pool.address
    )?;

    // make sure the real_amount_weth is not less than the initial amount
    // TODO implement a while loop and run the simulation again
    if real_amount_weth < amount_in {
        // increase the amount of tokens to sell by 5%
        amount_of_tokens_to_sell = (amount_of_tokens_to_sell * 105) / 100;
    }

    let minimum_received =
        (real_amount_weth * U256::from(*BUY_NUMERATOR)) / U256::from(*BUY_DENOMINATOR);

    // encode the final call data
    let call_data = encode_swap(
        pool.token_1, // input
        *WETH, // output
        pool.address,
        amount_of_tokens_to_sell,
        minimum_received
    );

    // ** Generate TxData
    let tx_data = TxData::new(
        call_data.into(),
        result.gas_used,
        minimum_received,
        Transaction::default(),
        U256::from(2u128), // 2 because we dont frontrun or back run
        convert_access_list(access_list)
    );

    Ok(tx_data)
}

// Get touched pools from a pending transaction
pub fn get_touched_pools(
    tx: &Transaction,
    next_block: &BlockInfo,
    pools: Vec<Pool>,
    fork_db: ForkDB
) -> Result<Option<Vec<Pool>>, anyhow::Error> {
    // setup an evm instance
    let mut evm = revm::EVM::new();
    evm.database(fork_db);

    // setup the next block state
    setup_evm(&mut evm, next_block);

    // simulate the pending tx
    evm.env.tx.caller = rAddress::from_slice(&tx.from.0);
    evm.env.tx.transact_to = TransactTo::Call(rAddress::from_slice(&tx.to.unwrap_or_default().0));
    evm.env.tx.data = tx.input.0.clone();
    evm.env.tx.value = tx.value.into();

    let res = evm.transact_ref()?;

    // get the touched accs
    let touched_accs = res.state.keys();

    // get the touched_pools from the touched_accs
    let touched_pools: Vec<Pool> = touched_accs
        .filter_map(|acc| {
            pools
                .iter()
                .find(|pool| pool.address == H160::from(*acc))
                .cloned()
        })
        .collect();

    // if the touched_pools vector is empty return None
    if touched_pools.is_empty() {
        return Ok(None);
    }

    // else return the touched_pools
    Ok(Some(touched_pools))
}

// Gets a new pair from a pending transaction
pub fn get_pair(
    next_block: BlockInfo,
    tx: &Transaction,
    fork_db: ForkDB
) -> Result<(Address, Address, Address, U256), anyhow::Error> {
    // setup an evm instance
    let mut evm = revm::EVM::new();
    evm.database(fork_db);

    // setup block state
    setup_evm(&mut evm, &next_block);

    // simulate pending tx
    evm.env.tx.value = tx.value.into();
    let result = sim_call(
        tx.from,
        tx.to.unwrap_or_default(),
        tx.input.clone(),
        true,
        &mut evm,
    )?;

    // ** define empty addresses
    let mut token_0 = Address::zero();
    let mut token_1 = Address::zero();
    let mut pool_address = Address::zero();
    let mut mint_pool_address = Address::zero();
    let mut reserve_0 = U256::zero();
    let mut reserve_1 = U256::zero();
    let sync_reserve_0;

    // Structures to hold decoded events
    let mut pair_created_opt = None;
    let mut mint_opt = None;
    let mut sync_opt = None;

    // Collect events
    for log in &result.logs {
        let converted_topics: Vec<_> = log.topics
            .iter()
            .map(|b256| H256::from_slice(b256.as_bytes()))
            .collect();

        // Check for PairCreated event
        if
            let Ok(decoded_log) = PAIR_CREATED_EVENT.parse_log(RawLog {
                topics: converted_topics.clone(),
                data: log.data.clone().to_vec(),
            })
        {
            pair_created_opt = Some(decoded_log);
        }

        // Check for Mint event
        if
            let Ok(decoded_log) = MINT_EVENT.parse_log(RawLog {
                topics: converted_topics.clone(),
                data: log.data.clone().to_vec(),
            })
        {
            mint_opt = Some(decoded_log);
            mint_pool_address = H160::from(log.address);
        }

        // Check for Sync event
        if
            let Ok(decoded_log) = SYNC_EVENT.parse_log(RawLog {
                topics: converted_topics.clone(),
                data: log.data.clone().to_vec(),
            })
        {
            sync_opt = Some(decoded_log);
        }
    }

    // Process PairCreated (if found)
    if let Some(pair_created) = pair_created_opt {
        // decode the log
        token_0 = pair_created.params[0].value.clone().into_token().into_address().unwrap();

        token_1 = pair_created.params[1].value.clone().into_token().into_address().unwrap();

        pool_address = pair_created.params[2].value.clone().into_token().into_address().unwrap();

        // get the reserves from the sync event
        match sync_opt {
            Some(sync) => {
                reserve_0 = sync.params[0].value.clone().into_token().into_uint().unwrap();
                reserve_1 = sync.params[1].value.clone().into_token().into_uint().unwrap();
            }
            None => {
                return Err(anyhow!("Sync event not found"));
            }
        }
    } else if
        // If no PairCreated, process Mint (if found)
        let Some(mint) = mint_opt
    {
        // get the reserves from the mint event
        reserve_0 = mint.params[1].value.clone().into_token().into_uint().unwrap();
        reserve_1 = mint.params[2].value.clone().into_token().into_uint().unwrap();

        // get the reserves from the sync event
        match sync_opt {
            Some(sync) => {
                // get the reserve_0 for comparison
                sync_reserve_0 = sync.params[0].value.clone().into_token().into_uint().unwrap();
            }
            None => {
                return Err(anyhow!("Sync event not found"));
            }
        }

        // check if the mint and sync reserves match
        // if they match we found a new pool, if they dont then we found a pool that was already existed

        // if reserves dont match return a zero pool address
        if reserve_0 != sync_reserve_0 {
            pool_address = Address::zero();
        } else {
            // return the pool address
            pool_address = mint_pool_address;
        }

        // if we got the pool address we can get the tokens by simulating a call to the pool contract
        (token_0, token_1) = get_tokens_from_pool(pool_address, &mut evm.clone())?;
    }

    // ** determine which token is weth and its corrospending reserve
    // ** we want to return the weth token address as token_0
    let (weth, token_1, weth_reserve) = if token_0 == *WETH {
        (token_0, token_1, reserve_0)
    } else {
        (token_1, token_0, reserve_1)
    };

    Ok((pool_address, weth, token_1, weth_reserve))
}
