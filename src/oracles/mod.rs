use crate::utils::types::structs::{ bot::Bot, snipe_tx::SnipeTx };
use ethers::prelude::*;
use crate::oracles::block_oracle::BlockInfo;
use crate::utils::evm::simulate::sim::{ generate_tx_data, profit_taker };
use crate::bot::{ send_tx::send_tx, remove_tx_from_oracles };
use crate::utils::{ constants::*, helpers::* };

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::anyhow;

pub mod block_oracle;
pub mod sell_oracle;
pub mod anti_rug_oracle;
pub mod nonce_oracle;
pub mod mempool_stream;
pub mod pair_oracle;
pub mod fork_db_oracle;

// monitor the status of the oracles
pub fn oracle_status(
    bot: Arc<RwLock<Bot>>,
) {
    tokio::spawn(async move {
        // print the status of the oracles every 15 seconds
        let sleep = tokio::time::Duration::from_secs_f32(15.0);
        loop {
            tokio::time::sleep(sleep).await;

            let bot_guard = bot.read().await;
            let sell_oracle_txs = bot_guard.get_sell_oracle_tx_len().await;
            let anti_rug_oracle_txs = bot_guard.get_anti_rug_oracle_tx_len().await;
            drop(bot_guard);

            log::info!("Sell Oracle: {:?} txs", sell_oracle_txs);
            log::info!("Anti Rug Oracle: {:?} txs", anti_rug_oracle_txs);
        }
    });
}

// ** HELPER FUNCTIONS FOR SELL ORACLE **

pub async fn time_check(
    client: Arc<Provider<Ws>>,
    next_block: BlockInfo,
    snipe_tx: SnipeTx,
    bot: Arc<RwLock<Bot>>,
    blocks_passed: U64,
    current_amount_out_weth: U256
) -> Result<(), anyhow::Error> {
    let is_10_min_passed = blocks_passed == (50u64).into();
    let is_20_min_passed = blocks_passed == (100u64).into();
    let is_40_min_passed = blocks_passed == (200u64).into();
    let is_60_min_passed = blocks_passed == (300u64).into();
    let is_8_hours_passed = blocks_passed >= (2400u64).into();

    // first check if any of the bools are true
    if
        !is_10_min_passed &&
        !is_20_min_passed &&
        !is_40_min_passed &&
        !is_60_min_passed &&
        !is_8_hours_passed
    {
        return Ok(());
    }

    let target_price_difference;

    if is_10_min_passed {
        // ** if 10 mins passed, set the target price to 30% up
        target_price_difference = (snipe_tx.amount_in * 130) / 100;
        log::info!("10min check is triggered for {:?}", snipe_tx.pool.token_1);
    } else if is_20_min_passed {
        // ** if 20 mins passed, set the target price to 60% up
        target_price_difference = (snipe_tx.amount_in * 160) / 100;
        log::info!("20min check is triggered for {:?}", snipe_tx.pool.token_1);
    } else if is_40_min_passed {
        // ** if 40 mins passed, set the target price to 100% up
        target_price_difference = (snipe_tx.amount_in * 200) / 100;
        log::info!("40min check is triggered for {:?}", snipe_tx.pool.token_1);
    } else if is_60_min_passed {
        // ** if 60 mins passed, set the target price to 200% up
        target_price_difference = (snipe_tx.amount_in * 300) / 100;
        log::info!("60min check is triggered for {:?}", snipe_tx.pool.token_1);
    } else if is_8_hours_passed {
        // ** if 8 hours passed, set the target price to 800% up
        target_price_difference = (snipe_tx.amount_in * 800) / 100;
        log::info!("8 hours check is triggered for {:?}", snipe_tx.pool.token_1);
    } else {
        return Ok(());
    }

    // ** If current_amount_out_weth is not at target price
    let is_price_met = current_amount_out_weth >= target_price_difference;

    // ** if price is not met Sell
    if !is_price_met {
        // sell the token
        process_tx(
            client.clone(),
            snipe_tx.clone(),
            next_block.clone(),
            bot
        ).await?;
    }

    Ok(())
}

pub async fn take_profit(
    client: Arc<Provider<Ws>>,
    snipe_tx: SnipeTx,
    next_block: BlockInfo,
    bot: Arc<RwLock<Bot>>
) -> Result<(), anyhow::Error> {
    
    // get the fork db
    let bot_guard = bot.read().await;
    let fork_db = bot_guard.get_fork_db().await;
    drop(bot_guard);

    // ** generate tx_data
    let tx_data = profit_taker(
        &next_block,
        snipe_tx.pool,
        snipe_tx.amount_in,
        fork_db
    )?;

    // update tx to pending
    let mut bot_guard = bot.write().await;
    bot_guard.set_tx_is_pending(snipe_tx.clone(), true).await;
    let nonce = bot_guard.get_nonce().await;
    drop(bot_guard);

    // ** send the tx
    let is_bundle_included = send_tx(client, tx_data.clone(), next_block, *MINER_TIP_TO_SELL, nonce).await?;

    if is_bundle_included {
        log::info!("Bundle included, took profit for {:?}", snipe_tx.pool.token_1);
        log::info!("Expected amount: {:?}", convert_wei_to_ether(tx_data.expected_amount));

        // update status
        let mut bot_guard = bot.write().await;
        bot_guard.set_tx_is_pending(snipe_tx.clone(), false).await;
        bot_guard.update_got_initial_out(snipe_tx, true).await;
        drop(bot_guard);
    } else {
        let mut bot_guard = bot.write().await;
        bot_guard.set_tx_is_pending(snipe_tx.clone(), false).await;
        bot_guard.update_got_initial_out(snipe_tx, false).await;
        drop(bot_guard);
        return Err(anyhow!("Bundle not included, will try again in the next block"));
    }

    Ok(())
}

// Sell the token
pub async fn process_tx(
    client: Arc<Provider<Ws>>,
    snipe_tx: SnipeTx,
    next_block: BlockInfo,
    bot: Arc<RwLock<Bot>>
) -> Result<(), anyhow::Error> {
    
    // get the fork db
    let bot_guard = bot.read().await;
    let fork_db = bot_guard.get_fork_db().await;
    drop(bot_guard);

    // ** generate tx_data
    let (tx_snipe, tx_data) = generate_tx_data(
        &snipe_tx.pool,
        U256::zero(),
        &next_block,
        None,
        *MINER_TIP_TO_SELL,
        2, // no frontrun or backrun
        false, // we sell
        fork_db
    ).expect("Failed to generate tx data");

    // ** First check if its worth it to sell it
    if tx_snipe.gas_cost > tx_data.expected_amount {
        return Err(
            anyhow!("Doesnt Worth to sell the token for now, will try again in the next block")
        );
    }

    // get the nonce and update it
    let mut bot_guard = bot.write().await;
    let nonce = bot_guard.get_nonce().await;
    drop(bot_guard);

    // ** Send The Tx
    let is_bundle_included = send_tx(
        client,
        tx_data.clone(),
        next_block,
        *MINER_TIP_TO_SELL,
        nonce
    ).await?;

    if is_bundle_included {
        log::info!(
            "Bundle included, sold token {:?} for {:?} ETH",
            snipe_tx.pool.token_1,
            convert_wei_to_ether(tx_data.expected_amount)
        );
        // ** remove the tx from the oracle
        remove_tx_from_oracles(bot.clone(), snipe_tx.clone()).await;
    } else {
        return Err(anyhow!("Bundle not included, will try again in the next block"));
    }

    Ok(())
}
