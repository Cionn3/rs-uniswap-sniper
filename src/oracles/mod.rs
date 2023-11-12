use crate::utils::types::{structs::*, events::NewBlockEvent};
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast};
use ethers::prelude::*;
use anyhow::anyhow;

use crate::utils::simulate::{profit_taker, generate_sell_tx_data, insert_pool_storage, get_token_balance};
use crate::utils::helpers::*;
use crate::forked_db::fork_factory::ForkFactory;
use crate::forked_db::fork_db::ForkDB;

use crate::bot::send_normal_tx::send_normal_tx;


pub mod block_oracle;
pub mod pair_oracle;

pub use block_oracle::*;
pub use pair_oracle::*;

pub mod sell_oracle;
pub use sell_oracle::*;

pub mod anti_rug_oracle;
pub use anti_rug_oracle::*;


pub mod mempool_stream;
pub use mempool_stream::*;

pub mod nonce_oracle;
pub use nonce_oracle::*;


// monitor the status of the oracles
pub fn oracle_status(
    bot: Arc<Mutex<Bot>>,
    mut new_block_receive: broadcast::Receiver<NewBlockEvent>
) {

    tokio::spawn(async move {

        loop {
            
            while let Ok(event) = new_block_receive.recv().await {
                let _latest_block = match event {
                    NewBlockEvent::NewBlock { latest_block } => latest_block,
                };
                
                let bot_guard = bot.lock().await;
                let sell_oracle_txs = bot_guard.get_sell_oracle_tx_len().await;
                let anti_rug_oracle_txs = bot_guard.get_anti_rug_oracle_tx_len().await;
                drop(bot_guard);

                log::info!("Sell Oracle: {:?} txs", sell_oracle_txs);
                log::info!("Anti Rug Oracle: {:?} txs", anti_rug_oracle_txs);

            }
            
        }
    });
    
}

// ** Helper functions


pub async fn remove_tx_from_oracles(
    bot: Arc<Mutex<Bot>>,
    snipe_tx: SnipeTx
) {
    let mut bot_guard = bot.lock().await;
    bot_guard.remove_tx_data(snipe_tx.clone()).await;
    bot_guard.remove_anti_rug_tx_data(snipe_tx.clone()).await;
    drop(bot_guard);
}

pub async fn add_tx_to_oracles(
    bot: Arc<Mutex<Bot>>,
    snipe_tx: SnipeTx
) {
    let mut bot_guard = bot.lock().await;
    bot_guard.add_tx_data(snipe_tx.clone()).await;
    bot_guard.add_anti_rug_tx_data(snipe_tx.clone()).await;
    drop(bot_guard);
    
}

// checks the position we got
// and adjust target sell price accordingly
pub async fn check_position(
    initial_amount_in: U256,
    next_block: &BlockInfo,
    token: Address,
    bot: Arc<Mutex<Bot>>,
    snipe_tx: SnipeTx,
    fork_db: ForkDB
) -> Result<(), anyhow::Error> {
    // get the token balance
    let token_balance = get_token_balance(
        token,
        get_snipe_contract_address(),
        &next_block,
        fork_db
    )?;

    // if the token_balance is less than 40% of expected amount
    // we assume that we got a bad position
    if token_balance < (snipe_tx.expected_amount_of_tokens * 6) / 10 {
        // set the target_amount_weth to 3x
       let target_amount_weth = (initial_amount_in * 30) / 10;
        // update the target_amount_weth in the oracle
        let mut bot_guard = bot.lock().await;
        bot_guard.update_target_amount(snipe_tx.clone(), target_amount_weth).await;
        drop(bot_guard);
        log::warn!("Got a bad position, changed target amount to 3x");
        Ok(())
    } else {
        log::info!("Position is good");
        Ok(())
    }
}


// see if the token is pumping
pub async fn time_check(
    client: Arc<Provider<Ws>>,
    next_block: BlockInfo,
    snipe_tx: SnipeTx,
    latest_block_number: Option<BlockId>,
    bot: Arc<Mutex<Bot>>,
    blocks_passed: U64,
    initial_amount_in: U256,
    current_amount_out_weth: U256
) -> Result<(), anyhow::Error> {
    let is_20_min_passed = blocks_passed == (100u64).into();
    let is_40_min_passed = blocks_passed == (200u64).into();
    let is_60_min_passed = blocks_passed == (300u64).into();
    let is_2_hours_passed = blocks_passed == (600u64).into();
    let is_8_hours_passed = blocks_passed >= (2400u64).into();

    // first check if any of the bools are true
    if
        !is_20_min_passed &&
        !is_40_min_passed &&
        !is_60_min_passed &&
        !is_2_hours_passed &&
        !is_8_hours_passed
    {
        return Ok(());
    }

    let target_price_difference;

    if is_20_min_passed {
        // ** if 20 mins passed, set the target price to 20% up
        target_price_difference = (initial_amount_in * 120) / 100;
        log::info!("20 min check is triggered for {:?}", snipe_tx.pool.token_1);
    } else if is_40_min_passed {
        // ** if 40 mins passed, set the target price to 40% up
        target_price_difference = (initial_amount_in * 140) / 100;
        log::info!("40 min check is triggered for {:?}", snipe_tx.pool.token_1);
    } else if is_60_min_passed {
        // ** if 60 mins passed, set the target price to 60% up
        target_price_difference = (initial_amount_in * 160) / 100;
        log::info!("60 min check is triggered for {:?}", snipe_tx.pool.token_1);
    } else if is_2_hours_passed {
        // ** if 2 hours passed, set the target price to 200% up
        target_price_difference = (initial_amount_in * 300) / 100;
        log::info!("2 hours check is triggered for {:?}", snipe_tx.pool.token_1);
    } else if is_8_hours_passed {
        // ** if 8 hours passed, set the target price to 900% up
        target_price_difference = (initial_amount_in * 900) / 100;
        log::info!("8 hours check is triggered for {:?}", snipe_tx.pool.token_1);
    } else {
        return Ok(());
    }

    // ** If current_amount_out_weth is not at target price
    let is_price_met = current_amount_out_weth >= target_price_difference;

    // ** if price is not met Sell
    if !is_price_met {
        // call process tx
        process_tx(
            client.clone(),
            snipe_tx.clone(),
            next_block.clone(),
            latest_block_number,
            bot
        ).await?;
    }

    Ok(())
}

// take initial out + the gas cost
pub async fn take_profit(
    client: Arc<Provider<Ws>>,
    snipe_tx: SnipeTx,
    next_block: BlockInfo,
    latest_block_number: Option<BlockId>,
    initial_amount_in_weth: U256,
    bot: Arc<Mutex<Bot>>
) -> Result<(), anyhow::Error> {
    let cache_db = insert_pool_storage(client.clone(), snipe_tx.pool, latest_block_number).await?;

    // ** setup fork factory backend
    let fork_factory = ForkFactory::new_sandbox_factory(
        client.clone(),
        cache_db,
        latest_block_number
    );

    // we dont only want the initial out but also the gas cost
    let amount_in = initial_amount_in_weth + snipe_tx.buy_cost;

    // generate tx data
    let tx_data = profit_taker(
        next_block.clone(),
        snipe_tx.pool,
        amount_in,
        SWAP_EVENT.clone(),
        TRANSFER_EVENT.clone(),
        fork_factory.new_sandbox_fork()
    )?;

    // ** miner tip
    // adjust the tip as you like
    let miner_tip = *MINER_TIP_TO_SELL;

    // ** max fee per gas must always be higher than miner tip
    let max_fee_per_gas = next_block.base_fee + miner_tip;

    // update tx to pending
    let mut bot_guard = bot.lock().await;
    bot_guard.set_tx_is_pending(snipe_tx.clone(), true).await;
    let nonce = bot_guard.get_nonce().await;
    drop(bot_guard);

    // ** Send The Tx
    // Because we want immedietly sell the token in the next block
    // We are sending the tx without Mev builders, hoping that the tx will be included in the next block
    let is_bundle_included = match
        send_normal_tx(client.clone(), tx_data.clone(), miner_tip, max_fee_per_gas, nonce).await
    {
        Ok(result) => result,
        Err(e) => {
            // update status
            let mut bot_guard = bot.lock().await;
            bot_guard.set_tx_is_pending(snipe_tx.clone(), false).await;
            bot_guard.update_got_initial_out(snipe_tx, false).await;
            drop(bot_guard);
            return Err(anyhow!("Failed to send tx: {:?}", e));
        }
    };

    if is_bundle_included {
        log::info!(
            "Bundle included, Took profit {:?} for {:?}",
            convert_wei_to_ether(amount_in),
            snipe_tx.pool.token_1
        );
        // update status
        let mut bot_guard = bot.lock().await;
        bot_guard.set_tx_is_pending(snipe_tx.clone(), false).await;
        bot_guard.update_got_initial_out(snipe_tx, true).await;
        drop(bot_guard);
    } else {
            // update status
            let mut bot_guard = bot.lock().await;
            bot_guard.set_tx_is_pending(snipe_tx.clone(), false).await;
            bot_guard.update_got_initial_out(snipe_tx, false).await;
            drop(bot_guard);
        return Err(anyhow!("Bundle not included, will try again in the next block"));
    }

    Ok(())
}


// sell the token
pub async fn process_tx(
    client: Arc<Provider<Ws>>,
    snipe_tx: SnipeTx,
    next_block: BlockInfo,
    latest_block_number: Option<BlockId>,
    bot: Arc<Mutex<Bot>>
) -> Result<(), anyhow::Error> {
    // ** initialize cache db and insert pool storage
    let cache_db = match
        insert_pool_storage(client.clone(), snipe_tx.pool, latest_block_number).await
    {
        Ok(cache_db) => cache_db,
        Err(e) => {
            return Err(anyhow!("Failed to insert pool storage: {:?}", e));
        }
    };

    // ** setup fork factory backend
    let fork_factory = ForkFactory::new_sandbox_factory(
        client.clone(),
        cache_db,
        latest_block_number
    );

    // ** generate tx_data
    let tx_data = match
        generate_sell_tx_data(snipe_tx.pool, next_block.clone(), fork_factory.new_sandbox_fork())
    {
        Ok(tx) => tx,
        Err(e) => {
            // ** if we get an error here GG
            // ** add plus 1 to the retries
            let mut bot_guard = bot.lock().await;
            bot_guard.update_attempts_to_sell(snipe_tx.clone()).await;
            drop(bot_guard);
            return Err(anyhow!("Failed to generate tx_data: {:?}", e));
        }
    };

    // ** max fee per gas must always be higher than miner tip
    let max_fee_per_gas = next_block.base_fee + *MINER_TIP_TO_SELL;

    // ** First check if its worth it to sell it
    // ** calculate the total gas cost
    let total_gas_cost = (next_block.base_fee + *MINER_TIP_TO_SELL) * tx_data.gas_used;

    if total_gas_cost > tx_data.expected_amount {
        return Err(
            anyhow!("Doesnt Worth to sell the token for now, will try again in the next block")
        );
    }

    // get the nonce and update it
    let mut bot_guard = bot.lock().await;
    let nonce = bot_guard.get_nonce().await;
    drop(bot_guard);

    // ** Send The Tx
    // Because we want immedietly sell the token in the next block
    // We are sending the tx without Mev builders, hoping that the tx will be included in the next block
    let is_bundle_included = match
        send_normal_tx(
            client.clone(),
            tx_data.clone(),
            *MINER_TIP_TO_SELL,
            max_fee_per_gas,
            nonce
        ).await
    {
        Ok(result) => result,
        Err(e) => {
            return Err(anyhow!("Failed to send tx: {:?}", e));
        }
    };

    if is_bundle_included {
        log::info!(
            "Bundle included, sold token {:?} for {:?} ETH",
            snipe_tx.pool.token_1,
            convert_wei_to_ether(tx_data.expected_amount)
        );
        // ** remove the tx from the oracle
        remove_tx_from_oracles(
            bot.clone(),
            snipe_tx.clone()
        ).await;
    } else {
        return Err(anyhow!("Bundle not included, will try again in the next block"));
    }

    Ok(())
}