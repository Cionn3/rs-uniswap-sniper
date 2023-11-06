use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::broadcast;

use crate::utils::helpers::{ create_local_client, convert_wei_to_ether, MINER_TIP_TO_SELL, MAX_SELL_ATTEMPTS };

use crate::bot::send_normal_tx::send_normal_tx;

use super::BlockInfo;
use crate::utils::simulate::simulate::{ simulate_sell, generate_sell_tx_data };
use crate::utils::simulate::insert_pool_storage;
use crate::bot::bot_config::BotConfig;
use crate::forked_db::fork_factory::ForkFactory;
use crate::utils::types::{
    structs::{ SnipeTx, SellOracle, AntiRugOracle },
    events::NewBlockEvent,
};
use anyhow::anyhow;



// ** Running swap simulations on every block to keep track of the selling price
pub fn start_sell_oracle(
    bot_config: BotConfig,
    shared_oracle: Arc<Mutex<SellOracle>>,
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
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


                    // if we reached the retry limit remove tx from oracles
                    if tx.attempts_to_sell >= *MAX_SELL_ATTEMPTS {
                        let mut oracle_guard = shared_oracle_clone.lock().await;
                        oracle_guard.remove_tx_data(tx.clone());
                        drop(oracle_guard);
                        let mut anti_rug_oracle_guard = anti_rug_oracle.lock().await;
                        anti_rug_oracle_guard.remove_tx_data(tx.clone());
                        drop(anti_rug_oracle_guard);
                        log::warn!("Sell Oracle: Retries >= 5, Removed tx from oracles");
                        continue;
                    }

                    let client = client.clone();

                    // ** The pool of the token we are selling
                    let pool = tx.pool;

                    // ** The Initial Amount in in WETH
                    let initial_amount_in = tx.amount_in;

                    // ** The Target Amount to sell in WETH
                    let mut target_amount_weth = tx.target_amount_weth;

                    let next_block = next_block.clone();

                    // ** check the price concurrently
                    tokio::spawn(async move {
                        // ** Adjust these values as you like
                        // ** implement time checks
                        // ** eg: is_2_min_passed => if we dont 20% up within 2 minutes (aporx 10 blocks) then sell
                        // ** 10mins -> 60% up
                        // ** 20mins -> 100% up

                        // ** caluclate how many blocks have passed since we bought the token
                        let blocks_passed = latest_block.number - tx.block_bought;
                        let is_2_min_passed = blocks_passed == (10u64).into();
                        let is_5_min_passed = blocks_passed == (25u64).into();
                        let is_10_min_passed = blocks_passed == (50u64).into();
                        let is_20_min_passed = blocks_passed == (100u64).into();
                        

                        // if 2 mins have passed check the price
                        if is_2_min_passed {
                            // ** calculate the target price difference (20% gain)
                            let target_price_difference = (initial_amount_in * 120) / 100;

                            match
                                process_tx(
                                    client.clone(),
                                    tx.clone(),
                                    next_block.clone(),
                                    latest_block_number,
                                    shared_oracle_clone.clone(),
                                    anti_rug_oracle.clone(),
                                    target_price_difference
                                ).await
                            {
                                Ok(_) => {
                                    // log::info!("Tx Sent Successfully");
                                }
                                Err(e) => {
                                    log::error!(
                                        "Early sell failed for token: {:?} Error: {:?}",
                                        tx.pool.token_1,
                                        e
                                    );
                                }
                            }
                            return;
                        }
                        if is_5_min_passed {
                            // ** calculate the target price difference (60% gain)
                            let target_price_difference = (initial_amount_in * 160) / 100;

                            match
                                process_tx(
                                    client.clone(),
                                    tx.clone(),
                                    next_block.clone(),
                                    latest_block_number,
                                    shared_oracle_clone.clone(),
                                    anti_rug_oracle.clone(),
                                    target_price_difference
                                ).await
                            {
                                Ok(_) => {
                                    // log::info!("Tx Sent Successfully");
                                }
                                Err(e) => {
                                    log::error!(
                                        "Early sell failed for token: {:?} Error: {:?}",
                                        tx.pool.token_1,
                                        e
                                    );
                                }
                            }
                        }

                        // if 10 mins have passed check the price
                        if is_10_min_passed {
                            // ** calculate the target price difference (100% gain)
                            let target_price_difference = initial_amount_in * 2;

                            match
                                process_tx(
                                    client.clone(),
                                    tx.clone(),
                                    next_block.clone(),
                                    latest_block_number,
                                    shared_oracle_clone.clone(),
                                    anti_rug_oracle.clone(),
                                    target_price_difference
                                ).await
                            {
                                Ok(_) => {
                                    // log::info!("Tx Sent Successfully");
                                }
                                Err(e) => {
                                    log::error!(
                                        "Early sell failed for token: {:?} Error: {:?}",
                                        tx.pool.token_1,
                                        e
                                    );
                                }
                            }
                        }

                        // if 20 mins have passed check the price
                        if is_20_min_passed {
                            // ** calculate the target price difference (300% gain)
                            let target_price_difference = initial_amount_in * 4;

                            match
                                process_tx(
                                    client.clone(),
                                    tx.clone(),
                                    next_block.clone(),
                                    latest_block_number,
                                    shared_oracle_clone.clone(),
                                    anti_rug_oracle.clone(),
                                    target_price_difference
                                ).await
                            {
                                Ok(_) => {
                                    // log::info!("Tx Sent Successfully");
                                }
                                Err(e) => {
                                    log::error!(
                                        "Early sell failed for token: {:?} Error: {:?}",
                                        tx.pool.token_1,
                                        e
                                    );
                                }
                            }
                        }

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

                        // ** get the amount out in weth
                        let amount_out_weth = match
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

                        // check if we got a bad position
                        // We do this check only once, when the tx is added to the oracle
                        // to do this check only once we check if current_block is equal to block_bought

                        if latest_block.number == tx.block_bought {
                            // If the amount_out is less than 70% of the initial amount in, we probably got a bad position
                            if amount_out_weth < (initial_amount_in * 7) / 10 {
                                // set the target_amount_weth to 1.5x
                                target_amount_weth = (initial_amount_in * 15) / 10;
                                // update the target_amount_weth in the oracle
                                let mut oracle_guard = shared_oracle_clone.lock().await;
                                oracle_guard.update_target_amount(tx.clone(), target_amount_weth);
                                drop(oracle_guard);
                                log::warn!("Got a bad position, changed target amount to 1.5x");
                            } else {
                                log::info!("Position is good");
                            }
                        }

                        // convert final_amount_weth from wei to Ether
                        let final_amount_weth_converted = convert_wei_to_ether(amount_out_weth);
                        let initial_amount_in_converted = convert_wei_to_ether(initial_amount_in);

                        if amount_out_weth < target_amount_weth {
                            log::info!(
                                "Token: {:?} initial amount in: {:?} ETH, current amount out: {:?} ETH",
                                pool.token_1,
                                initial_amount_in_converted,
                                final_amount_weth_converted
                            );
                            return;
                        }

                        // ** if amount_out_weth is >= target_amount_weth, send the tx to builders
                        if amount_out_weth >= target_amount_weth {
                            // ** generate tx_data
                            let tx_data = match
                                generate_sell_tx_data(
                                    pool,
                                    next_block.clone(),
                                    fork_factory.new_sandbox_fork()
                                )
                            {
                                Ok(tx) => tx,
                                Err(e) => {
                                    log::error!(
                                        "Failed to generate tx_data for Token: {:?} Error {:?}",
                                        pool.token_1,
                                        e
                                    );
                                    return;
                                }
                            };

                            // ** miner tip
                            let miner_tip = *MINER_TIP_TO_SELL;

                            // ** max fee per gas must always be higher than miner tip
                            let max_fee_per_gas = next_block.base_fee + miner_tip;

                            // ** Send The Tx **
                            // use send_tx module to send the tx to flashbots
                            // but because we are just selling we are not exposed to frontrunning
                            let is_bundle_included = match
                                send_normal_tx(
                                    client.clone(),
                                    tx_data.clone(),
                                    miner_tip,
                                    max_fee_per_gas
                                ).await
                            {
                                Ok(result) => result,
                                Err(e) => {
                                    log::error!("Failed to send tx to flashbots: {:?}", e);
                                    return;
                                }
                            };

                            if is_bundle_included {
                                log::info!(
                                    "Bundle included, initial amount in: {:?} ETH, final amount out: {:?} ETH",
                                    initial_amount_in_converted,
                                    final_amount_weth_converted
                                );
                                // ** remove the tx from the oracles
                                let mut oracle_guard = shared_oracle_clone.lock().await;
                                oracle_guard.remove_tx_data(tx.clone());
                                drop(oracle_guard);
                                let mut anti_rug_oracle_guard = anti_rug_oracle.lock().await;
                                anti_rug_oracle_guard.remove_tx_data(tx.clone());
                                drop(anti_rug_oracle_guard);
                            } else {
                                log::error!(
                                    "Bundle not included, will try again in the next block"
                                );
                                return;
                            }
                        }
                    }); // end of tokio::spawn
                } // end of for loop
            } // end of while loop
        } // end of main loop
    }); // end of main tokio::spawn
}

async fn process_tx(
    client: Arc<Provider<Ws>>,
    snipe_tx: SnipeTx,
    next_block: BlockInfo,
    latest_block_number: Option<BlockId>,
    shared_oracle: Arc<Mutex<SellOracle>>,
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
    target_price_difference: U256
) -> Result<(), anyhow::Error> {
    // ** initialize cache db and insert pool storage
    let cache_db = match
        insert_pool_storage(client.clone(), snipe_tx.pool, latest_block_number).await
    {
        Ok(cache_db) => cache_db,
        Err(e) => {
            log::error!("Failed to insert pool storage: {:?}", e);
            return Err(anyhow!("Failed to insert pool storage: {:?}", e));
        }
    };

    // ** setup fork factory backend
    let fork_factory = ForkFactory::new_sandbox_factory(
        client.clone(),
        cache_db,
        latest_block_number
    );

    // ** get the amount out in weth
    let amount_out_weth = match
        simulate_sell(snipe_tx.pool, next_block.clone(), fork_factory.new_sandbox_fork())
    {
        Ok(result) => result,
        Err(e) => {
            // ** if we get an error here GG
            // ** add plus 1 to the retries
            let mut oracle_guard = shared_oracle.lock().await;
            oracle_guard.update_attempts_to_sell(snipe_tx.clone());
            drop(oracle_guard);
            return Err(anyhow!("Failed to simulate early sell: {:?}", e));
        }
    };

    // ** If amount_out_weth is not at target price
    let is_price_met = amount_out_weth >= target_price_difference;

    // ** if price is not met Sell
    if !is_price_met {
        // ** generate tx_data
        let tx_data = match
            generate_sell_tx_data(
                snipe_tx.pool,
                next_block.clone(),
                fork_factory.new_sandbox_fork()
            )
        {
            Ok(tx) => tx,
            Err(e) => {
                // ** if we get an error here GG
                // ** add plus 1 to the retries
                let mut oracle_guard = shared_oracle.lock().await;
                oracle_guard.update_attempts_to_sell(snipe_tx.clone());
                drop(oracle_guard);
                return Err(anyhow!("Failed to generate tx_data: {:?}", e));
            }
        };

        // ** miner tip
        // adjust the tip as you like
        let miner_tip = *MINER_TIP_TO_SELL; // 3 gwei

        // ** max fee per gas must always be higher than miner tip
        let max_fee_per_gas = next_block.base_fee + miner_tip;

        // ** First check if its worth it to sell it
        // ** calculate the total gas cost
        let total_gas_cost = (next_block.base_fee + miner_tip) * tx_data.gas_used;

        if total_gas_cost > tx_data.expected_amount {
            //  log::error!("Doesnt Worth to sell the token for now, will try again in the next block");
            return Err(
                anyhow!("Doesnt Worth to sell the token for now, will try again in the next block")
            );
        }

        // ** Send The Tx
        // Because we want immedietly sell the token in the next block
        // We are sending the tx without Mev builders, hoping that the tx will be included in the next block
        let is_bundle_included = match
            send_normal_tx(client.clone(), tx_data.clone(), miner_tip, max_fee_per_gas).await
        {
            Ok(result) => result,
            Err(e) => {
                return Err(anyhow!("Failed to send tx: {:?}", e));
            }
        };

        if is_bundle_included {
            log::info!(
                "Bundle included, sold token {:?} for {:?} ETH",
                snipe_tx.pool.token_0,
                convert_wei_to_ether(tx_data.expected_amount)
            );
            // ** remove the tx from the oracle
            let mut oracle_guard = shared_oracle.lock().await;
            oracle_guard.remove_tx_data(snipe_tx.clone());
            drop(oracle_guard);
            let mut anti_rug_oracle = anti_rug_oracle.lock().await;
            anti_rug_oracle.remove_tx_data(snipe_tx.clone());
            drop(anti_rug_oracle);
        } else {
            return Err(anyhow!("Bundle not included, will try again in the next block"));
        }
    } // end of if !is_price_met

    // ** if price is met, do nothing
    Ok(())
}
