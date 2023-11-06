use ethers::prelude::*;
use revm::primitives::{ ExecutionResult, Output, TransactTo, U256 as rU256, B160 as rAddress };
use anyhow::anyhow;
use super::*;
use crate::forked_db::fork_db::ForkDB;
use crate::oracles::block_oracle::BlockInfo;
use crate::utils::helpers::*;
use ethers::abi::Tokenizable;
use ethabi::{ RawLog, Event };
use crate::utils::types::structs::*;

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
    setup_block_state(&mut evm, next_block);

    // if we have a pending tx simulate it
    if let Some(tx) = pending_tx.clone() {
        // commit the pending tx so we can buy the token
        commit_pending_tx(&mut evm, &tx)?;
    }

    // enable checks
    evm.env.cfg.disable_base_fee = false;
    evm.env.cfg.disable_block_gas_limit = false;
    evm.env.cfg.disable_balance_check = false;

    // ** create the call_data for the swap
    let call_data = encode_swap(
        pool.token_0, // weth
        pool.token_1, // shitcoin
        pool.address,
        amount_in_weth,
        U256::from(0u128)
    );

    // ** simulate the tax check **
    // ** We do a buy/sell on the same block **
    // ** We could skip Salmonela Inspector **

    let is_buy_reverted = commit_tx_with_inspector(
        &mut evm,
        call_data,
        next_block,
        &pool.token_1,
        get_my_address() // caller
    )?;

    // ** Do the sell Transaction **

    // ** Get The Token Balance for the amount_in to sell **
    let amount_in_token = get_balance_of_evm(
        pool.token_1, // shitcoin
        get_snipe_contract_address(),
        next_block,
        &mut evm
    )?;

    // ** create the call_data for the swap
    let call_data = encode_swap(
        pool.token_1, // shitcoin
        pool.token_0, // weth
        pool.address,
        amount_in_token,
        U256::from(0u128)
    );

    let is_sell_reverted = commit_tx_with_inspector(
        &mut evm,
        call_data,
        next_block,
        &pool.token_1,
        get_my_address() // caller
    )?;

    // get the post balance of weth
    let final_amount_weth = get_balance_of_evm(
        pool.token_0, // weth
        get_snipe_contract_address(),
        next_block,
        &mut evm
    )?;

    if is_buy_reverted {
        log::warn!("Buy reverted {:?}", pool.token_1);
        return Ok(false);
    }

    if is_sell_reverted {
        log::error!("Sell reverted {:?}", pool.token_1);
        return Ok(false);
    }

    // Check if the final WETH amount is less than 80% of the expected amount
    // change this as you like
    if final_amount_weth < (amount_in_weth * 8) / 10 {
        return Err(
            anyhow!("Skipped Token, we lost more than 20% in a buy/sell: {:?}", pool.token_1)
        );
    }

    // ** SIMULATE SELL 200 BLOCKS FURTHER
    // Now Do one more check but this time we sell 200 blocks further

    // setup a new db
    let mut evm = revm::EVM::new();
    evm.database(fork_db);

    // ** setup the next block state
    setup_block_state(&mut evm, next_block);

    if let Some(tx) = pending_tx {
        // first simulate and commit the pending tx so we can buy the token
        commit_pending_tx(&mut evm, &tx)?;
    }

    // enable checks
    evm.env.cfg.disable_base_fee = false;
    evm.env.cfg.disable_block_gas_limit = false;
    evm.env.cfg.disable_balance_check = false;

    // ** create the call_data for the swap
    let call_data = encode_swap(
        pool.token_0, // weth
        pool.token_1, // shitcoin
        pool.address,
        amount_in_weth,
        U256::from(0u128)
    );

    // ** simulate the buy swap
    let is_buy_reverted = commit_tx_with_inspector(
        &mut evm,
        call_data,
        next_block,
        &pool.token_1,
        get_my_address() // caller
    )?;

    // ** Do the sell Transaction **

    // set the block number 200 blocks further
    evm.env.block.number = rU256::from(next_block.number.as_u64() + 200);
    evm.env.block.timestamp = (next_block.timestamp + U256::from(2400u128)).into();

    // ** Get The Token Balance for the amount_in to sell **
    let amount_in_token = get_balance_of_evm(
        pool.token_1, // shitcoin
        get_snipe_contract_address(),
        next_block,
        &mut evm
    )?;

    // ** create the call_data for the swap
    let call_data = encode_swap(
        pool.token_1, // shitcoin
        pool.token_0, // weth
        pool.address,
        amount_in_token,
        U256::from(0u128)
    );

    let is_sell_reverted = commit_tx_with_inspector(
        &mut evm,
        call_data,
        next_block,
        &pool.token_1,
        get_my_address() // caller
    )?;

    // get the post balance of weth
    let final_amount_weth = get_balance_of_evm(
        pool.token_0, // weth
        get_snipe_contract_address(),
        next_block,
        &mut evm
    )?;

    if is_buy_reverted {
        log::error!("Buy reverted after 200 blocks {:?}", pool.token_1);
        return Ok(false);
    }

    if is_sell_reverted {
        log::error!("Sell reverted after 200 blocks {:?}", pool.token_1);
        return Ok(false);
    }

    // Check if the final WETH amount is less than 80% of the expected amount
    if final_amount_weth < (amount_in_weth * 8) / 10 {
        return Err(
            anyhow!(
                "Skipped Token after 200 blocks, we lost more than 20% in a buy/sell: {:?}",
                pool.token_1
            )
        );
    }

    Ok(true)
}

pub fn transfer_check(
    pool: &Pool,
    amount_in_weth: U256,
    next_block: &BlockInfo,
    pending_tx: Option<Transaction>,
    fork_db: ForkDB
) -> Result<(), anyhow::Error> {
    let mut evm = revm::EVM::new();
    evm.database(fork_db);

    // setup the next block state
    setup_block_state(&mut evm, next_block);

    if let Some(tx) = pending_tx {
        // first simulate and commit the pending tx so we can buy the token
        commit_pending_tx(&mut evm, &tx)?;
    }
    // enable checks
    evm.env.cfg.disable_base_fee = false;
    evm.env.cfg.disable_block_gas_limit = false;
    evm.env.cfg.disable_balance_check = false;

    //** Buy The Token **
    // ** create the call_data for the swap
    let call_data = encode_swap(
        pool.token_0, // weth
        pool.token_1, // shitcoin
        pool.address,
        amount_in_weth,
        U256::from(0u128)
    );

    let _commit_tx = commit_tx_with_inspector(
        &mut evm,
        call_data,
        next_block,
        &pool.token_1,
        get_my_address() // caller
    )?;

    // ** Do Tranfer Check **
    //** A lot of honeypots tokens dont allow you to transfer the token at all

    //** set the block number 200 blocks further
    evm.env.block.number = rU256::from(next_block.number.as_u64() + 200);
    evm.env.block.timestamp = (next_block.timestamp + U256::from(2400u128)).into();

    // ** Get The Token Balance for the amount_in to tranfer **
    let amount_in_token = get_balance_of_evm(
        pool.token_1, // shitcoin
        get_snipe_contract_address(),
        next_block,
        &mut evm
    )?;

    // ** create the call_data for the swap
    let call_data = create_withdraw_data(
        pool.token_1, // shitcoin
        amount_in_token
    );

    let _withdraw = commit_tx_with_inspector(
        &mut evm,
        call_data,
        next_block,
        &pool.token_1,
        get_admin_address() // caller
    )?;

    // get the post balance of admin (the address we sent the tokens)
    let amount_token_in_admin = get_balance_of_evm(
        pool.token_1, // shitcoin
        get_admin_address(),
        next_block,
        &mut evm
    )?;

    //** check if we lost more than 20% on the transfer */
    if amount_token_in_admin < (amount_in_token * 8) / 10 {
        return Err(anyhow!("We lost more than 20% on the transfer"));
    }

    Ok(())
}

pub fn generate_buy_tx_data(
    pool: &Pool,
    amount_in_weth: U256,
    next_block: &BlockInfo,
    pending_tx: Option<Transaction>,
    fork_db: ForkDB
) -> Result<SnipeTx, anyhow::Error> {
    let mut evm = revm::EVM::new();
    evm.database(fork_db);

    // setup the next block state
    setup_block_state(&mut evm, next_block);

    if let Some(ref tx) = pending_tx {
        // first simulate and commit the pending tx so we can buy the token
        commit_pending_tx(&mut evm, tx)?;
    }

    // enable checks
    evm.env.cfg.disable_base_fee = false;
    evm.env.cfg.disable_block_gas_limit = false;
    evm.env.cfg.disable_balance_check = false;

    // create the call_data for the swap
    let call_data = encode_swap(
        pool.token_0, // weth
        pool.token_1, // shitcoin
        pool.address,
        amount_in_weth,
        U256::from(0u128)
    );

    // commit tx
    let (access_list, gas_used) = commit_tx_with_access_list(&mut evm, call_data, next_block)?;

    // get the balance of the token we bought
    let token_balance = get_balance_of_evm(
        pool.token_1, // shitcoin
        get_snipe_contract_address(),
        next_block,
        &mut evm
    )?;

    // 10% tolerance/slippage
    // adjust this as you like
    let expected_amount = (token_balance * 9) / 10;

    // create the call data again
    let call_data = encode_swap(
        pool.token_0, // weth
        pool.token_1, // shitcoin
        pool.address,
        amount_in_weth,
        expected_amount // expected amount
    );

    // ** Generate SnipeTx
    let tx = pending_tx.unwrap_or_default();
    Ok(
        SnipeTx::new(
            call_data.into(),
            get_snipe_contract_address(),
            access_list,
            gas_used,
            *pool,
            amount_in_weth,
            *TARGET_AMOUNT_TO_SELL,
            next_block.number,
            Some(tx.clone()),
            0, // zero attempts to sell
            0, // zero snipe retries
            false // pending tx is false
        )
    )
}

pub fn simulate_sell(
    pool: Pool,
    next_block: BlockInfo,
    fork_db: ForkDB
) -> Result<U256, anyhow::Error> {
    let mut evm = revm::EVM::new();
    evm.database(fork_db);

    // setup the next block state
    setup_block_state(&mut evm, &next_block);

    // ** get the token balance for the amount_in to sell
    let amount_in = get_balance_of_evm(
        pool.token_1, // token1 is always a shitcoin
        get_snipe_contract_address(),
        &next_block,
        &mut evm
    )?;

    // ** Get The initial WETH Balance
    let before_balance = get_balance_of_evm(
        pool.token_0, // weth
        get_snipe_contract_address(),
        &next_block,
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

    // commit tx
    let _commit_tx = commit_tx(&mut evm, call_data, &next_block)?;

    // get the post balance of weth
    let post_balance_weth = get_balance_of_evm(
        pool.token_0, // weth
        get_snipe_contract_address(),
        &next_block,
        &mut evm
    )?;

    // calculate the final amount of weth
    let final_amount_weth = post_balance_weth.checked_sub(before_balance).unwrap_or_default();

    Ok(final_amount_weth)
}

// ** Run a sell Simulation After a Tx that either touches the pool we hold or the contract address of the token
// ** If any of the tx reverts we try to panic sell by front running the tx
pub fn simulate_sell_after(
    tx: &Transaction,
    pool: Pool,
    next_block: BlockInfo,
    fork_db: ForkDB
) -> Result<U256, anyhow::Error> {
    // setup an evm instance
    let mut evm = revm::EVM::new();
    evm.database(fork_db);

    // setup the next block state
    setup_block_state(&mut evm, &next_block);

    // simulate pending tx
    let _commit_pending = commit_pending_tx(&mut evm, tx)?;

    // enable checks
    evm.env.cfg.disable_base_fee = false;
    evm.env.cfg.disable_block_gas_limit = false;
    evm.env.cfg.disable_balance_check = false;

    // ** Now Simulate The Sell Transaction

    // ** get the token balance for the amount_in to sell
    let amount_in = get_balance_of_evm(
        pool.token_1, // token1 is always a shitcoin
        get_snipe_contract_address(),
        &next_block,
        &mut evm
    )?;

    if amount_in == U256::zero() {
        log::error!("Anti-Honeypot ERROR contract doesnt have any balance for {:?}", pool.token_1);
    }

    // ** Get The initial WETH Balance
    let before_balance = get_balance_of_evm(
        pool.token_0, // weth
        get_snipe_contract_address(),
        &next_block,
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

    // ** Simulate The Sell Transaction

    // setup evm for swap
    evm.env.tx.caller = get_my_address().into();
    evm.env.tx.transact_to = TransactTo::Call(get_snipe_contract_address().0.into());
    evm.env.tx.data = call_data.clone().into();
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 1000000;
    evm.env.tx.gas_price = next_block.base_fee.into();

    let mut salmonella_inspector = SalmonellaInspectoooor::new();

    let result = match evm.inspect_commit(&mut salmonella_inspector) {
        Ok(res) => res,
        Err(e) => {
            return Err(anyhow!("Error when commiting sell tx: {:?}", e));
        }
    };

    // match the inspector to see if token is safu
    let mut is_safu = true;
    match salmonella_inspector.is_safu() {
        IsSafu::Safu => {/* continue */}
        IsSafu::NotSafu(not_safu_opcodes) => {
            log::error!(
                "Token {:?} is not safu, found the following opcodes: {:?}",
                pool.token_1,
                not_safu_opcodes
            );
            is_safu = false;
        }
    }

    // define a bool to check if the tx reverted
    let reverted = match result {
        ExecutionResult::Success { .. } => false,
        ExecutionResult::Revert { .. } => true,
        ExecutionResult::Halt { .. } => true,
    };

    // get the post balance of weth
    let post_balance_weth = get_balance_of_evm(
        pool.token_0, // weth
        get_snipe_contract_address(),
        &next_block,
        &mut evm
    )?;

    // calculate the final amount of weth
    let final_amount_weth = post_balance_weth.checked_sub(before_balance).unwrap_or_default();

    // ** Check which txs are reverted
    // if the token is not safu then return 0
    if !is_safu {
        log::error!("Token is not safu, returning 0");
        return Ok(U256::zero());
    }

    // ** if our tx is reverted then return 0
    if reverted {
        log::error!("Our tx is reverted, returning 0");
        return Ok(U256::zero());
    } else {
        return Ok(final_amount_weth);
    }
}

pub fn generate_sell_tx_data(
    pool: Pool,
    next_block: BlockInfo,
    fork_db: ForkDB
) -> Result<TxData, anyhow::Error> {
    let mut evm = revm::EVM::new();
    evm.database(fork_db);

    // setup the next block state
    setup_block_state(&mut evm, &next_block);

    // ** get the token balance for the amount_in to sell

    let amount_in = get_balance_of_evm(
        pool.token_1, // token1 is always a shitcoin
        get_snipe_contract_address(),
        &next_block,
        &mut evm
    )?;

    // ** Get The initial WETH Balance
    let before_balance = get_balance_of_evm(
        pool.token_0, // weth
        get_snipe_contract_address(),
        &next_block,
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

    // commit tx
    let (access_list, gas_used) = commit_tx_with_access_list(
        &mut evm,
        call_data.clone(),
        &next_block
    )?;

    // get the post balance of weth
    let post_balance_weth = get_balance_of_evm(
        pool.token_0, // weth
        get_snipe_contract_address(),
        &next_block,
        &mut evm
    )?;

    // calculate the final amount of weth
    let expected_amount = post_balance_weth.checked_sub(before_balance).unwrap_or_default();

    // ** generate TxData
    let tx_data = TxData::new(
        call_data.into(),
        access_list,
        gas_used,
        expected_amount,
        get_snipe_contract_address(),
        Transaction::default(),
        U256::from(2u128) // 2 because we dont do frontrun or backrun
    );

    Ok(tx_data)
}

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
    setup_block_state(&mut evm, next_block);

    // disable checks
    evm.env.cfg.disable_base_fee = true;
    evm.env.cfg.disable_block_gas_limit = true;
    evm.env.cfg.disable_balance_check = true;

    // simulate the pending tx
    evm.env.tx.caller = rAddress::from_slice(&tx.from.0);
    evm.env.tx.transact_to = TransactTo::Call(rAddress::from_slice(&tx.to.unwrap_or_default().0));
    evm.env.tx.data = tx.input.0.clone();
    evm.env.tx.value = tx.value.into();
    evm.env.tx.gas_limit = 5000000;

    let res = match evm.transact_ref() {
        Ok(result) => result,
        Err(e) => {
            return Err(anyhow!("Failed to commit pending tx for touched pools: {:?}", e));
        }
    };

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

    /* 
    let output: Bytes = match res {
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
    */

    // if the touched_pools vector is empty return None
    if touched_pools.is_empty() {
        return Ok(None);
    }

    // else return the touched_pools
    Ok(Some(touched_pools))
}

pub fn get_pair(
    next_block: BlockInfo,
    tx: &Transaction,
    sync_event: Event,
    mint_event: Event,
    pair_created_event: Event,
    fork_db: ForkDB
) -> Result<(Address, Address, Address, U256), anyhow::Error> {
    // setup an evm instance
    let mut evm = revm::EVM::new();
    evm.database(fork_db);

    // setup block state
    setup_block_state(&mut evm, &next_block);

    // disable checks
    evm.env.cfg.disable_base_fee = true;
    evm.env.cfg.disable_block_gas_limit = true;
    evm.env.cfg.disable_balance_check = true;

    // simulate tx
    evm.env.tx.caller = rAddress::from_slice(&tx.from.0);
    evm.env.tx.transact_to = TransactTo::Call(rAddress::from_slice(&tx.to.unwrap_or_default().0));
    evm.env.tx.data = tx.input.0.clone();
    evm.env.tx.value = tx.value.into();
    evm.env.tx.gas_limit = 5000000;

    let res = match evm.transact_commit() {
        Ok(result) => result,
        Err(e) => {
            return Err(anyhow!("Failed to commit GetPair tx: {:?}", e));
        }
    };

    // get the logs from the tx
    let logs = res.logs();

    let _output: Bytes = match res {
        ExecutionResult::Success { output, .. } =>
            match output {
                Output::Call(o) => o.into(),
                Output::Create(o, _) => o.into(),
            }
        ExecutionResult::Revert { output, .. } => {
            return Err(anyhow!("GetPair with tx hash {:?}  reverted: {:?}", tx.hash, output));
        }
        ExecutionResult::Halt { reason, .. } => {
            return Err(anyhow!("GetPair with tx hash {:?}  halted: {:?}", tx.hash, reason));
        }
    };

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
    for log in &logs {
        let converted_topics: Vec<_> = log.topics
            .iter()
            .map(|b256| H256::from_slice(b256.as_bytes()))
            .collect();

        // Check for PairCreated event
        if
            let Ok(decoded_log) = pair_created_event.parse_log(RawLog {
                topics: converted_topics.clone(),
                data: log.data.clone().to_vec(),
            })
        {
            pair_created_opt = Some(decoded_log);
        }

        // Check for Mint event
        if
            let Ok(decoded_log) = mint_event.parse_log(RawLog {
                topics: converted_topics.clone(),
                data: log.data.clone().to_vec(),
            })
        {
            mint_opt = Some(decoded_log);
            mint_pool_address = H160::from(log.address);
        }

        // Check for Sync event
        if
            let Ok(decoded_log) = sync_event.parse_log(RawLog {
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
        (token_0, token_1) = match simulate_token_call(pool_address, &mut evm.clone()) {
            Ok(tokens) => tokens,
            Err(e) => {
                return Err(anyhow!("Failed to simulate token call: {:?}", e));
            }
        };
    }

    // ** determine which token is weth and its corrospending reserve
    // ** we want to return the weth token address as token_0
    let (weth, token_1, weth_reserve) = if token_0 == get_weth_address() {
        (token_0, token_1, reserve_0)
    } else {
        (token_1, token_0, reserve_1)
    };

    Ok((pool_address, weth, token_1, weth_reserve))
}
