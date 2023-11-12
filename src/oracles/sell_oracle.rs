use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::broadcast;

use crate::utils::helpers::*;
use super::*;

use crate::utils::simulate::simulate::simulate_sell;
use crate::utils::simulate::insert_pool_storage;
use crate::forked_db::fork_factory::ForkFactory;
use crate::utils::types::{
    structs::Bot,
    events::NewBlockEvent,
};




// ** Running swap simulations on every block to keep track of the selling price
pub fn start_sell_oracle(
    bot: Arc<Mutex<Bot>>,
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
                let bot_guard = bot.lock().await;
                let snipe_txs = bot_guard.get_sell_oracle_tx_data().await;
                drop(bot_guard);

                // ** if there are no txs in the oracle, continue
                if snipe_txs.is_empty() {
                    continue;
                }

                let client = client.clone();

                // get the the next block
                let bot_guard = bot.lock().await;
                let (_, next_block) = bot_guard.get_block_info().await;
                drop(bot_guard);

                let latest_block_number = Some(
                    BlockId::Number(BlockNumber::Number(latest_block.number))
                );

                for tx in snipe_txs {

                    // if we reached the retry limit remove tx from oracles
                    if tx.attempts_to_sell >= *MAX_SELL_ATTEMPTS {
                        remove_tx_from_oracles(bot.clone(), tx.clone()).await;

                        log::warn!(
                            "Sell Oracle: Retries >={:?}, Removed tx from oracles",
                            *MAX_SELL_ATTEMPTS
                        );
                        continue;
                    }

                    // ** Clone vars
                    let client = client.clone();
                    let bot = bot.clone();

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
                            insert_pool_storage(client.clone(), tx.pool, latest_block_number).await
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
                            simulate_sell(
                                tx.pool,
                                next_block.clone(),
                                fork_factory.new_sandbox_fork()
                            )
                        {
                            Ok(result) => result,
                            Err(e) => {
                                log::error!(
                                    "Failed to sell token: {:?} Error: {:?}",
                                    tx.pool.token_1,
                                    e
                                );
                                // ** if we get an error here GG
                                // ** add plus 1 to the retries
                                let mut bot_guard = bot.lock().await;
                                bot_guard.update_attempts_to_sell(tx.clone()).await;
                                drop(bot_guard);
                                return;
                            }
                        };

                        // see if we got taxed 90% +
                        if current_amount_out_weth < (initial_amount_in * 9) / 100 {
                            log::info!("Got Taxed 90% + for {:?}", tx.pool.token_1);
                            log::info!("GG");
                            // remove tx from oracles
                            remove_tx_from_oracles(bot.clone(), tx.clone()).await;
                        }

                        // check if we got a bad position
                        // We do this check only once, when the tx is added to the oracle
                        // to do this check only once we check if current_block is equal to block_bought

                        if latest_block.number == tx.block_bought {
                            match
                                check_position(
                                    initial_amount_in,
                                    &next_block,
                                    tx.pool.token_1,
                                    bot.clone(),
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
                                bot.clone(),
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
                                        bot.clone()
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
                                    bot
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
                                tx.pool.token_1,
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