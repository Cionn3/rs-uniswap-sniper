use tokio::sync::broadcast;

use ethers::prelude::*;
use tokio::sync::RwLock;
use std::sync::Arc;

use crate::{
    utils::types::{ structs::{ bot::Bot, pool::Pool }, events::* },
    bot::{ calculate_miner_tip, remove_tx_from_oracles },
};
use crate::utils::constants::*;
use crate::utils::helpers::*;
use crate::utils::evm::simulate::
    sim::{ generate_tx_data, simulate_sell, get_touched_pools};

use crate::bot::send_tx::send_tx;






pub fn start_anti_rug(
    bot: Arc<RwLock<Bot>>,
    mut new_mempool_receiver: broadcast::Receiver<MemPoolEvent>
) {
    tokio::spawn(async move {
        // client reconnect loop
        loop {
            let client = match create_local_client().await {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to create local client: {}", e);
                    // we reconnect by restarting the loop
                    continue;
                }
            };

            while let Ok(event) = new_mempool_receiver.recv().await {
                let pending_tx = match event {
                    MemPoolEvent::NewTx { tx } => tx,
                };

                match process_anti_rug(bot.clone(), &client, pending_tx).await {
                    Ok(_) => log::trace!("Anti Rug processed successfully"),
                    Err(e) => log::error!("Anti Rug Err {:?}", e),
                }
            } // end while of loop
        } // end of loop
    });
}

async fn process_anti_rug(
    bot: Arc<RwLock<Bot>>,
    client: &Arc<Provider<Ws>>,
    pending_tx: Transaction
) -> Result<(), anyhow::Error> {
    // ** Get the snipe tx data from the oracle
    let bot_guard = bot.read().await;
    let snipe_txs = bot_guard.get_sell_oracle_tx_data().await;
    drop(bot_guard);

    // ** no snipe tx in oracle, skip
    if snipe_txs.is_empty() {
        return Ok(());
    }

    // ** get the pools from the SnipeTx
    let vec_pools = snipe_txs
        .iter()
        .map(|x| x.pool)
        .collect::<Vec<Pool>>();

    // ** get the block info from the oracle
    let bot_guard = bot.read().await;
    let (_, next_block) = bot_guard.get_block_info().await;
    let fork_db = bot_guard.get_fork_db().await;
    drop(bot_guard);


    // ** first see if the pending_tx touches one of the pools in the oracle
    // ** We want to check if the DeV is trying to rug by removing liquidity
    let touched_pools = if
        let Ok(Some(tp)) = get_touched_pools(
            &pending_tx,
            &next_block,
            vec_pools,
            fork_db.clone()
        )
    {
        tp
    } else {
        // log::info!("No pools touched");
        Vec::new() // Return an empty Vec<Pool>
    };

    // ** if touched_pools vector contain some pools
    if touched_pools.len() > 0 {
        // ** Run simulations to detect any unusual behavior

        for pool in touched_pools {
            let snipe_txs = snipe_txs.clone();
            let bot = bot.clone();
            let fork_db = fork_db.clone();
            let next_block = next_block.clone();
            let pending_tx = pending_tx.clone();
            let client = client.clone();

            tokio::spawn(async move {

                // ** get the amount_out in weth before the pending tx
                let amount_out_before = simulate_sell(
                    None,
                    pool,
                    next_block.clone(),
                    fork_db.clone()
                ).expect("Failed to simulate sell");

                // ** get the amount_out after the pending tx
                let amount_out_after = simulate_sell(
                    Some(pending_tx.clone()),
                    pool,
                    next_block.clone(),
                    fork_db.clone()
                ).expect("Failed to simulate sell");

                // ** EXTRA SAFE VERSION
                // ** compare the amount_out_before and amount_out_after
                // ** if amount_out_after is at least 20% less than amount_out_before
                // ** Frontrun the pending tx

                if amount_out_after < (amount_out_before * 8) / 100 {
                    log::info!("Anti-Rug Alert!🚨 Possible rug detected!");
                    log::info!("Detected Tx Hash: {:?}", pending_tx.hash);
                    log::info!(
                        "Amount out Before: ETH {:?}",
                        convert_wei_to_ether(amount_out_before)
                    );
                    log::info!(
                        "Amount out After: ETH {:?}",
                        convert_wei_to_ether(amount_out_after)
                    );

                    let pending_tx_priority_fee = pending_tx.max_priority_fee_per_gas.unwrap_or_default();
                    let mut miner_tip = calculate_miner_tip(pending_tx_priority_fee);

                    // ** make sure the miner tip is not less than the sell priority fee
                    // ** in case we have conficting txs atleast we can replace it

                    if miner_tip < *MINER_TIP_TO_SELL {
                        miner_tip = (*MINER_TIP_TO_SELL * 12) / 10; // +20%
                    }

                    // ** generate tx data
                    let (tx_snipe, tx_data) = generate_tx_data(
                        &pool,
                        U256::zero(),
                        &next_block,
                        None,
                        miner_tip,
                        0, // frontrun
                        false, // we sell
                        fork_db
                    ).expect("Failed to generate tx data");

                    // ** First check if its worth it to frontrun the tx
                    if tx_snipe.gas_cost > tx_data.expected_amount {
                        log::warn!("Anti-Rug🚨: Doesnt Worth to escape the rug pool, GG");
                        return;
                    }

                    log::info!("Escaping Rug!🚀");
                    log::info!("Pending tx priority fee: {:?}", pending_tx_priority_fee);
                    log::info!("Our Miner Tip: {:?}", convert_wei_to_gwei(miner_tip));

                    // get the nonce
                    let mut bot_guard = bot.write().await;
                    let nonce = bot_guard.get_nonce().await;
                    drop(bot_guard);

                    // ** Send Tx
                    let is_bundle_included = send_tx(
                        client.clone(),
                        tx_data,
                        next_block,
                        miner_tip,
                        nonce
                    ).await.expect("Failed to send tx");

                    if is_bundle_included {
                        log::info!("Bundle included we escaped the rug pool!🚀");
                        // ** find the corrosponding SnipeTx from the pool address
                        let snipe_tx = snipe_txs
                            .iter()
                            .find(|&x| x.pool.address == pool.address)
                            .unwrap();

                        remove_tx_from_oracles(bot, snipe_tx.clone()).await;
                    } else {
                        log::warn!("Bundle not included, we are getting rugged! GG");
                        return;
                    }
                }
            }); // end of tokio::spawn
        } // end of for pool in touched_pools
    } // end of if touched_pools.len() > 0

    Ok(())
}

pub fn start_anti_honeypot(
    bot: Arc<RwLock<Bot>>,
    mut new_mempool_receiver: broadcast::Receiver<MemPoolEvent>
) {
    tokio::spawn(async move {
        // client reconnect loop
        loop {
            let client = match create_local_client().await {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to create local client: {}", e);
                    // we reconnect by restarting the loop
                    continue;
                }
            };

            while let Ok(event) = new_mempool_receiver.recv().await {
                let pending_tx = match event {
                    MemPoolEvent::NewTx { tx } => tx,
                };

                match process_anti_honeypot(bot.clone(), &client, pending_tx).await {
                    Ok(_) => log::trace!("Anti Honeypot processed successfully"),
                    Err(e) => log::error!("Anti Honeypot Err {:?}", e),
                }
            } // end while of loop
        } // end of loop
    });
}

async fn process_anti_honeypot(
    bot: Arc<RwLock<Bot>>,
    client: &Arc<Provider<Ws>>,
    pending_tx: Transaction
) -> Result<(), anyhow::Error> {
    // ** Get the snipe tx data from the oracle
    let bot_guard = bot.read().await;
    let snipe_txs = bot_guard.get_sell_oracle_tx_data().await;
    drop(bot_guard);

    // ** no snipe tx in oracle, skip
    if snipe_txs.is_empty() {
        return Ok(());
    }

    // ** get the pools from the SnipeTx
    let vec_pools = snipe_txs
        .iter()
        .map(|x| x.pool)
        .collect::<Vec<Pool>>();

    // ** Check if pending_tx.to matches one of the token addresses in vec_pools
    let is_pending_to_token = vec_pools.iter().any(|x| pending_tx.to == Some(x.token_1));

    // ** Clone vars
    let client = client.clone();
    let bot = bot.clone();

    tokio::spawn(async move {
        // ** if pending_tx.to matches one of the token addresses in vec_pools
        if is_pending_to_token {
            // ** get the pool that matches the pending_tx.to
            let touched_pool = vec_pools
                .iter()
                .find(|x| pending_tx.to == Some(x.token_1))
                .unwrap();

            // get the blockinfo
            let bot_guard = bot.read().await;
            let (_, next_block) = bot_guard.get_block_info().await;
            let fork_db = bot_guard.get_fork_db().await;
            drop(bot_guard);

            // ** First simulate the sell tx before the pending tx
            let amount_out_before = simulate_sell(
                None,
                *touched_pool,
                next_block.clone(),
                fork_db.clone()
            ).expect("Failed to simulate sell");

            // ** get the amount_out in weth after the pending tx
            // ** here we use the backend with the empty db
            let amount_out_after = simulate_sell(
                Some(pending_tx.clone()),
                *touched_pool,
                next_block.clone(),
                fork_db.clone()
            ).expect("Failed to simulate sell");

            // ** EXTRA SAFE VERSION
            // ** compare the amount_out_before and amount_out_after
            // ** if amount_out_after is at least 20% less than amount_out_before
            // ** Frontrun the pending tx

            if amount_out_after < (amount_out_before * 8) / 10 {
                log::info!("Anti-HoneyPot Alert!🚨 Possible rug detected!");
                log::info!("Detected Tx Hash: {:?}", pending_tx.hash);
                log::info!("Amount out Before: ETH {:?}", convert_wei_to_ether(amount_out_before));
                log::info!("Amount out After: ETH {:?}", convert_wei_to_ether(amount_out_after));

                let pending_tx_priority_fee = pending_tx.max_priority_fee_per_gas.unwrap_or_default();
                let mut miner_tip = calculate_miner_tip(pending_tx_priority_fee);

                // ** make sure the miner tip is not less than the sell priority fee
                // ** in case we have conficting txs atleast we can replace it

                if miner_tip < *MINER_TIP_TO_SELL {
                    miner_tip = (*MINER_TIP_TO_SELL * 12) / 10; // +20%
                }

                // ** generate tx data
                let (tx_snipe, tx_data) = generate_tx_data(
                    touched_pool,
                    U256::zero(),
                    &next_block,
                    None,
                    miner_tip,
                    0, // frontrun
                    false, // we sell
                    fork_db
                ).expect("Failed to generate tx data");

                // ** First check if its worth it to frontrun the tx
                if tx_snipe.gas_cost > tx_data.expected_amount {
                    log::warn!("Anti-HoneyPot🚨: Doesnt Worth to escape the rug pool, GG");
                    return;
                }

                log::info!("Escaping HoneyPot!🚀");
                log::info!("Pending tx priority fee: {:?}", pending_tx_priority_fee);
                log::info!("Our Miner Tip: {:?}", convert_wei_to_gwei(miner_tip));

                // get the nonce
                let mut bot_guard = bot.write().await;
                let nonce = bot_guard.get_nonce().await;
                drop(bot_guard);

                // ** Send Tx
                let is_bundle_included = send_tx(
                    client.clone(),
                    tx_data,
                    next_block,
                    miner_tip,
                    nonce
                ).await.expect("Failed to send tx");

                if is_bundle_included {
                    log::info!("Bundle included we escaped the rug pool!🚀");
                    // ** find the corrosponding SnipeTx from the pool address
                    let snipe_tx = snipe_txs
                        .iter()
                        .find(|&x| x.pool.address == touched_pool.address)
                        .unwrap();

                    remove_tx_from_oracles(bot, snipe_tx.clone()).await;
                } else {
                    log::warn!("Bundle not included, we are getting rugged! GG");
                    return;
                }
            } // end of if amount_out_after < (amount_out_before * 8) / 10
        } // end of if is_pending_to_token
    }); // end of tokio::spawn

    Ok(())
}
