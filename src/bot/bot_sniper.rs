use tokio::sync::broadcast;
use ethers::prelude::*;

use tokio::sync::RwLock;
use std::sync::Arc;

use crate::utils::constants::*;
use crate::utils::evm::simulate::sim::{ tax_check, generate_tx_data, find_amount_in };
use crate::utils::helpers::*;
use crate::utils::types::structs::snipe_tx::SnipeTx;
use crate::bot::{ add_tx_to_oracles, remove_tx_from_oracles };
use crate::utils::types::structs::{ bot::Bot, pool::Pool };
use crate::utils::types::events::{ NewPairEvent, NewBlockEvent };

use super::send_tx::send_tx;

pub fn start_sniper(
    mut new_pair_receiver: broadcast::Receiver<NewPairEvent>,
    bot: Arc<RwLock<Bot>>
) {
    tokio::spawn(async move {
        loop {
            let client = match create_local_client().await {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to create local client: {}", e);
                    // we reconnect by restarting the loop
                    continue;
                }
            };

            // start the oracle by subscribing to new pairs
            while let Ok(event) = new_pair_receiver.recv().await {
                let (pool, tx) = match event {
                    NewPairEvent::NewPairWithTx { pool, tx } => (pool, tx),
                };

                // process the tx
                match process_tx(bot.clone(), client.clone(), pool.clone(), tx).await {
                    Ok(_) => log::trace!("Tx Sent Successfully"),
                    Err(e) => log::error!("Snipe failed {:?}", e),
                }
            } // end of while loop
        }
    });
}

async fn process_tx(
    bot: Arc<RwLock<Bot>>,
    client: Arc<Provider<Ws>>,
    pool: Pool,
    pending_tx: Transaction
) -> Result<(), anyhow::Error> {
    // get block info from oracle

    let bot_guard = bot.read().await;
    let (_, next_block) = bot_guard.get_block_info().await;
    let fork_db = bot_guard.get_fork_db().await;
    drop(bot_guard);


    // find the amount in in case the token has a min buy size
    let amount_in = find_amount_in(
        &pool,
        &next_block,
        Some(pending_tx.clone()),
        fork_db.clone()
    )?;

    // do tax check
    let is_swap_success = tax_check(
        &pool,
        amount_in,
        &next_block,
        Some(pending_tx.clone()),
        fork_db.clone()
    )?;

    // if swap fails push it to retry oracle
    if !is_swap_success {
        let snipe_tx = SnipeTx::default(pool, *TARGET_AMOUNT_TO_SELL, next_block.number);
        let mut bot_guard = bot.write().await;
        bot_guard.add_tx_to_retry_oracle(snipe_tx).await;
        drop(bot_guard);
        return Err(anyhow::anyhow!("Swap failed, sent to retry oracle"));
    }

    log::info!("Sniping with miner tip: {:?}", convert_wei_to_gwei(*MINER_TIP_TO_SNIPE));

    // ** Generate TxData
    let (snipe_tx, tx_data) = generate_tx_data(
        &pool,
        amount_in,
        &next_block,
        Some(pending_tx),
        *MINER_TIP_TO_SNIPE,
        1, // 1 for backrun
        true, // yes we buy
        fork_db
    )?;

    // add snipe_tx to oracle
    add_tx_to_oracles(bot.clone(), snipe_tx.clone()).await;

    // get the nonce and update it
    let mut bot_guard = bot.write().await;
    let nonce = bot_guard.get_nonce().await;
    drop(bot_guard);

    let is_bundle_included = send_tx(
        client.clone(),
        tx_data,
        next_block,
        *MINER_TIP_TO_SNIPE,
        nonce
    ).await?;

    // if bundle not included push it to retry oracle

    if is_bundle_included == false {
        // remove the tx from oracles so we dont get bombarded with logs
        remove_tx_from_oracles(bot.clone(), snipe_tx.clone()).await;

        // push to retry oracle
        let mut bot_guard = bot.write().await;
        bot_guard.add_tx_to_retry_oracle(snipe_tx).await;
        drop(bot_guard);
    }

    Ok(())
}

pub fn snipe_retry(
    bot: Arc<RwLock<Bot>>,
    mut new_block_receive: broadcast::Receiver<NewBlockEvent>
) {
    tokio::spawn(async move {
        loop {
            let client = match create_local_client().await {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to create local client: {}", e);
                    // we reconnect by restarting the loop
                    continue;
                }
            };

            // start the oracle by subscribing to new blocks
            while let Ok(event) = new_block_receive.recv().await {
                let _latest_block = match event {
                    NewBlockEvent::NewBlock { latest_block } => latest_block,
                };

                match process_retry_tx(bot.clone(), client.clone()).await {
                    Ok(_) => log::trace!("Tx Sent Successfully"),
                    Err(e) => log::error!("Retry Snipe failed {:?}", e),
                }
            } // end of while loop
        } // end of loop
    }); // end of tokio spawn
}

async fn process_retry_tx(
    bot: Arc<RwLock<Bot>>,
    client: Arc<Provider<Ws>>,
) -> Result<(), anyhow::Error> {
    // get block info  and tx data
    let bot_guard = bot.read().await;
    let (_, next_block) = bot_guard.get_block_info().await;
    let snipe_txs = bot_guard.get_retry_oracle_tx_data().await;
    let fork_db = bot_guard.get_fork_db().await;
    drop(bot_guard);

    // ** if there are no txs in the oracle, skip
    if snipe_txs.is_empty() {
        return Ok(());
    }

    for tx in snipe_txs {
        let bot = bot.clone();
        let fork_db = fork_db.clone();
        let client = client.clone();
        let next_block = next_block.clone();

        // if the tx is pending skip
        if tx.retry_pending {
            continue;
        }

        // if we reached the retry limit remove tx from oracles
        if tx.snipe_retries >= *MAX_SNIPE_RETRIES {
            let mut bot_guard = bot.write().await;
            bot_guard.remove_tx_from_retry_oracle(tx.clone()).await;
            drop(bot_guard);
            continue;
        }

        // first check if the weth reserve is changed
        let current_reserve = match get_reserves(tx.pool.address, client.clone()).await {
            Ok(reserve) => reserve,
            Err(e) => {
                log::error!("Failed to get reserves: {:?}", e);
                continue;
            }
        };

        // if current weth reserve has increased by at least 20% skip
        if current_reserve > tx.pool.weth_liquidity * 120 / 100 {
            // remove tx from retry oracle
            let mut bot = bot.write().await;
            bot.remove_tx_from_retry_oracle(tx.clone()).await;
            drop(bot);
            log::warn!("Token already sniped, removed from retry oracle");
            continue;
        }

        // spawn tasks
        tokio::spawn(async move {
            // find the amount in in case the token has a min buy size
            let amount_in = find_amount_in(
                &tx.pool,
                &next_block,
                None,
                fork_db.clone()
            ).expect("Failed to find amount in");

            // if amount in is zero skip
            if amount_in == U256::zero() {
                let mut bot_guard = bot.write().await;
                bot_guard.update_retry_counter(tx.clone()).await;
                drop(bot_guard);
                return;
            }

            // do tax check
            let is_swap_success = tax_check(
                &tx.pool,
                amount_in,
                &next_block,
                None,
                fork_db.clone()
            ).expect("Failed to do tax check");

            // if swap fails update counter
            if !is_swap_success {
                let mut bot_guard = bot.write().await;
                bot_guard.update_retry_counter(tx.clone()).await;
                drop(bot_guard);
                return;
            }

            // ** Generate TxData
            let (snipe_tx, tx_data) = generate_tx_data(
                &tx.pool,
                amount_in,
                &next_block,
                None,
                *MINER_TIP_TO_SNIPE,
                2, // no backrun or frontrun
                true, // yes we buy
                fork_db
            ).expect("Failed to generate tx data");

            // add tx to oracles
            add_tx_to_oracles(bot.clone(), snipe_tx.clone()).await;

            // set tx to pending and get the nonce
            let mut bot_guard = bot.write().await;
            bot_guard.update_retry_pending(tx.clone(), true).await;
            let nonce = bot_guard.get_nonce().await;
            drop(bot_guard);

            // send the tx
            let is_bundle_included = send_tx(
                client.clone(),
                tx_data,
                next_block,
                *MINER_TIP_TO_SNIPE,
                nonce
            ).await.expect("Failed to send tx");

            if is_bundle_included {
                // remove it from retry
                let mut bot_guard = bot.write().await;
                bot_guard.remove_tx_from_retry_oracle(tx.clone()).await;
                drop(bot_guard);
            } else {
                // update the counter and pending status
                let mut bot_guard = bot.write().await;
                bot_guard.update_retry_counter(tx.clone()).await;
                bot_guard.update_retry_pending(tx.clone(), false).await;
                drop(bot_guard);

                // remove the tx from oracles so we dont get bombarded with logs
                remove_tx_from_oracles(bot.clone(), snipe_tx.clone()).await;
            }
        }); // end of tokio
    } // end of for loop

    Ok(())
}
