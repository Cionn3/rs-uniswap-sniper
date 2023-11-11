use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::broadcast;

use crate::utils::helpers::*;

use crate::bot::send_normal_tx::send_normal_tx;

use super::BlockInfo;
use crate::utils::simulate::simulate::{ simulate_sell, generate_sell_tx_data, profit_taker };
use crate::utils::simulate::insert_pool_storage;
use crate::bot::bot_config::BotConfig;
use crate::forked_db::fork_factory::ForkFactory;
use crate::utils::types::{
    structs::{ SnipeTx, SellOracle, NonceOracle, AntiRugOracle },
    events::NewBlockEvent,
};
use anyhow::anyhow;

// ** Running swap simulations on every block to keep track of the selling price
pub fn start_sell_oracle(
    bot_config: BotConfig,
    shared_oracle: Arc<Mutex<SellOracle>>,
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
    nonce_oracle: Arc<Mutex<NonceOracle>>,
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

            // start the sell_oracle by subscribing to new blocks
            while let Ok(event) = new_block_receive.recv().await {
                let latest_block = match event {
                    NewBlockEvent::NewBlock { latest_block } => latest_block,
                };

                // ** Get the snipe tx data from the oracle
                let snipe_txs = {
                    let oracle = shared_oracle.lock().await;
                    oracle.tx_data.clone()
                };

                // ** if there are no txs in the oracle, continue
                if snipe_txs.is_empty() {
                    continue;
                }

                let client = client.clone();

                let block_oracle = bot_config.block_oracle.clone();

                // get the next block
                let next_block = {
                    let block_oracle = block_oracle.read().await;
                    block_oracle.next_block.clone()
                };

                let latest_block_number = Some(
                    BlockId::Number(BlockNumber::Number(latest_block.number))
                );

                for tx in snipe_txs {
                    let shared_oracle_clone = shared_oracle.clone();
                    let anti_rug_oracle = anti_rug_oracle.clone();
                    let nonce_oracle = nonce_oracle.clone();

                    // if we reached the retry limit remove tx from oracles
                    if tx.attempts_to_sell >= *MAX_SELL_ATTEMPTS {
                        remove_tx_from_oracles(
                            shared_oracle_clone.clone(),
                            anti_rug_oracle.clone(),
                            tx.clone()
                        ).await;

                        log::warn!("Sell Oracle: Retries >={:?}, Removed tx from oracles", *MAX_SELL_ATTEMPTS);
                        continue;
                    }

                    let client = client.clone();

                    // ** The pool of the token we are selling
                    let pool = tx.pool;

                    // ** The Initial Amount in in WETH
                    let initial_amount_in = tx.amount_in;

                    // ** The Target Amount to sell in WETH
                    let target_amount_weth = tx.target_amount_weth;

                    let next_block = next_block.clone();

                    // ** check the price concurrently
                    tokio::spawn(async move {
                        // ** caluclate how many blocks have passed since we bought the token
                        let blocks_passed = latest_block.number - tx.block_bought;

                        // ** initialize cache db and insert pool storage
                        let cache_db = match
                            insert_pool_storage(client.clone(), pool, latest_block_number).await
                        {
                            Ok(cache_db) => cache_db,
                            Err(e) => {
                                log::error!("Failed to insert pool storage: {:?}", e);
                                return;
                            }
                        };

                        // ** setup fork factory backend
                        let fork_factory = ForkFactory::new_sandbox_factory(
                            client.clone(),
                            cache_db,
                            latest_block_number
                        );

                        // ** get the current amount out in weth
                        let current_amount_out_weth = match
                            simulate_sell(pool, next_block.clone(), fork_factory.new_sandbox_fork())
                        {
                            Ok(result) => result,
                            Err(e) => {
                                log::error!(
                                    "Failed to sell token: {:?} Error: {:?}",
                                    pool.token_1,
                                    e
                                );
                                // ** if we get an error here GG
                                // ** add plus 1 to the retries
                                let mut oracle_guard = shared_oracle_clone.lock().await;
                                oracle_guard.update_attempts_to_sell(tx.clone());
                                drop(oracle_guard);
                                return;
                            }
                        };

                        // see if we got taxed 90% +
                        if current_amount_out_weth < (initial_amount_in * 9) / 100 {
                            log::info!("Got Taxed 90% + for {:?}", pool.token_1);
                            log::info!("GG");
                            // remove tx from oracles
                            remove_tx_from_oracles(
                                shared_oracle_clone.clone(),
                                anti_rug_oracle.clone(),
                                tx.clone()
                            ).await;
                        }

                        // check if we got a bad position
                        // We do this check only once, when the tx is added to the oracle
                        // to do this check only once we check if current_block is equal to block_bought

                        if latest_block.number == tx.block_bought {
                            match
                                check_position(
                                    initial_amount_in,
                                    &next_block,
                                    pool.token_1,
                                    shared_oracle_clone.clone(),
                                    tx.clone(),
                                    fork_factory.new_sandbox_fork()
                                ).await
                            {
                                Ok(_) => {}
                                Err(e) => {
                                    log::warn!("Failed to check position: {:?}", e);
                                }
                            }
                        }

                        // see if the token is pumping
                        // make sure we dont hold it forever
                        match
                            time_check(
                                client.clone(),
                                next_block.clone(),
                                tx.clone(),
                                latest_block_number,
                                shared_oracle_clone.clone(),
                                anti_rug_oracle.clone(),
                                nonce_oracle.clone(),
                                blocks_passed,
                                initial_amount_in.clone(),
                                current_amount_out_weth
                            ).await
                        {
                            Ok(_) => {}
                            Err(e) => {
                                log::warn!("Failed to do time check: {:?}", e);
                            }
                        }

                        // convert final_amount_weth from wei to Ether
                        let final_amount_weth_converted =
                            convert_wei_to_ether(current_amount_out_weth);
                        let initial_amount_in_converted = convert_wei_to_ether(initial_amount_in);

                        // if we hit our initial profit take target take the initial amount in out + buy cost
                        // take the initial out only once
                        if tx.got_initial_out == false {
                            // see if the current tx is pending
                            if tx.is_pending == true {
                                log::info!("Tx is pending, will try again in the next block");
                                return;
                            }

                            // first check just in case if the token pumped to the moon
                            let to_the_moon = current_amount_out_weth >= target_amount_weth;

                            // if its not then check if we hit the initial profit take target
                            if
                                !to_the_moon &&
                                current_amount_out_weth >= initial_amount_in * *INITIAL_PROFIT_TAKE
                            {
                                log::info!(
                                    "We hit a {:?}x! Taking the initial amount in out",
                                    *INITIAL_PROFIT_TAKE
                                );
                                match
                                    take_profit(
                                        client.clone(),
                                        tx.clone(),
                                        next_block.clone(),
                                        latest_block_number,
                                        initial_amount_in,
                                        shared_oracle_clone.clone(),
                                        nonce_oracle.clone()
                                    ).await
                                {
                                    Ok(_) => {}
                                    Err(e) => {
                                        log::warn!("Failed to take profit: {:?}", e);
                                    }
                                }
                            }
                        } // end of if tx.got_initial_out == false

                        // ** if amount_out_weth is >= target_amount_weth, send the tx to builders
                        if current_amount_out_weth >= target_amount_weth {
                            match
                                process_tx(
                                    client.clone(),
                                    tx.clone(),
                                    next_block.clone(),
                                    latest_block_number,
                                    shared_oracle_clone.clone(),
                                    anti_rug_oracle.clone(),
                                    nonce_oracle.clone()
                                ).await
                            {
                                Ok(_) => {}
                                Err(e) => {
                                    log::warn!("Failed to process tx: {:?}", e);
                                }
                            }
                        }

                        // if we dont met target price, skip
                        if current_amount_out_weth < target_amount_weth {
                            log::info!(
                                "Token: {:?} initial amount in: {:?} ETH, current amount out: {:?} ETH",
                                pool.token_1,
                                initial_amount_in_converted,
                                final_amount_weth_converted
                            );
                            return;
                        }
                    }); // end of tokio::spawn
                } // end of for loop
            } // end of while loop
        } // end of main loop
    }); // end of main tokio::spawn
}

async fn time_check(
    client: Arc<Provider<Ws>>,
    next_block: BlockInfo,
    snipe_tx: SnipeTx,
    latest_block_number: Option<BlockId>,
    shared_oracle: Arc<Mutex<SellOracle>>,
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
    nonce_oracle: Arc<Mutex<NonceOracle>>,
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
            shared_oracle.clone(),
            anti_rug_oracle.clone(),
            nonce_oracle.clone()
        ).await?;
    }

    Ok(())
}

async fn take_profit(
    client: Arc<Provider<Ws>>,
    snipe_tx: SnipeTx,
    next_block: BlockInfo,
    latest_block_number: Option<BlockId>,
    initial_amount_in_weth: U256,
    sell_oracle: Arc<Mutex<SellOracle>>,
    nonce_oracle: Arc<Mutex<NonceOracle>>
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
    let mut sell_oracle_guard = sell_oracle.lock().await;
    sell_oracle_guard.set_tx_is_pending(snipe_tx.clone(), true);
    drop(sell_oracle_guard);

    // get the nonce and update it
    let mut nonce_guard = nonce_oracle.lock().await;
    let nonce = nonce_guard.get_nonce();
    nonce_guard.update_nonce(nonce + 1);
    drop(nonce_guard);

    // ** Send The Tx
    // Because we want immedietly sell the token in the next block
    // We are sending the tx without Mev builders, hoping that the tx will be included in the next block
    let is_bundle_included = match
        send_normal_tx(client.clone(), tx_data.clone(), miner_tip, max_fee_per_gas, nonce).await
    {
        Ok(result) => result,
        Err(e) => {
            // update status
            let mut sell_oracle_guard = sell_oracle.lock().await;
            sell_oracle_guard.update_got_initial_out(snipe_tx.clone(), false);
            sell_oracle_guard.set_tx_is_pending(snipe_tx.clone(), false);
            drop(sell_oracle_guard);
            return Err(anyhow!("Failed to send tx: {:?}", e));
        }
    };

    if is_bundle_included {
        log::info!(
            "Bundle included, Took profit {:?} for {:?}",
            convert_wei_to_ether(amount_in),
            snipe_tx.pool.token_1
        );
        // update the got initial out
        let mut sell_oracle_guard = sell_oracle.lock().await;
        sell_oracle_guard.update_got_initial_out(snipe_tx.clone(), true);
        sell_oracle_guard.set_tx_is_pending(snipe_tx.clone(), false);
        // update status
        drop(sell_oracle_guard);
    } else {
        // update status
        let mut sell_oracle_guard = sell_oracle.lock().await;
        sell_oracle_guard.update_got_initial_out(snipe_tx.clone(), false);
        sell_oracle_guard.set_tx_is_pending(snipe_tx.clone(), false);
        drop(sell_oracle_guard);
        return Err(anyhow!("Bundle not included, will try again in the next block"));
    }

    Ok(())
}

async fn process_tx(
    client: Arc<Provider<Ws>>,
    snipe_tx: SnipeTx,
    next_block: BlockInfo,
    latest_block_number: Option<BlockId>,
    sell_oracle: Arc<Mutex<SellOracle>>,
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
    nonce_oracle: Arc<Mutex<NonceOracle>>
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
            let mut oracle_guard = sell_oracle.lock().await;
            oracle_guard.update_attempts_to_sell(snipe_tx.clone());
            drop(oracle_guard);
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
    let mut nonce_guard = nonce_oracle.lock().await;
    let nonce = nonce_guard.get_nonce();
    nonce_guard.update_nonce(nonce + 1);
    drop(nonce_guard);

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
            sell_oracle.clone(),
            anti_rug_oracle.clone(),
            snipe_tx.clone()
        ).await;
    } else {
        return Err(anyhow!("Bundle not included, will try again in the next block"));
    }

    Ok(())
}
