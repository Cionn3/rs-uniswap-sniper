use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::broadcast;

use crate::utils::helpers::*;

use crate::utils::constants::*;
use crate::bot:: remove_tx_from_oracles;
use super::{ time_check, take_profit, process_tx };

use crate::utils::types::structs::bot::Bot;
use crate::oracles::block_oracle::BlockInfo;
use crate::utils::evm::simulate::sim::simulate_sell;



// ** Running swap simulations on every block to keep track of the selling price
pub fn start_sell_oracle(
    bot: Arc<RwLock<Bot>>,
    mut new_block_receiver: broadcast::Receiver<BlockInfo>
) {
    tokio::spawn(async move {
        loop {
            let client = match create_local_client().await {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to create local client: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            while let Ok(latest_block) = new_block_receiver.recv().await {

                match process(bot.clone(), client.clone(), latest_block).await {
                    Ok(_) => log::trace!("Tx Sent Successfully"),
                    Err(e) => log::error!("Retry Snipe failed {:?}", e),
                }
            } // end while of loop
        } // end of loop
    }); // end tokio::spawn
} // end of start_sell_oracle

async fn process(
    bot: Arc<RwLock<Bot>>,
    client: Arc<Provider<Ws>>,
    latest_block: BlockInfo
) -> Result<(), anyhow::Error> {
    // ** Get the snipe tx data from the oracle
    let bot_guard = bot.read().await;
    let snipe_txs = bot_guard.get_sell_oracle_tx_data().await;
    let (_, next_block) = bot_guard.get_block_info().await;
    let fork_db = bot_guard.get_fork_db().await;
    drop(bot_guard);

    // ** if there are no txs in the oracle, continue
    if snipe_txs.is_empty() {
        return Ok(());
    }

    for tx in snipe_txs {
        let bot = bot.clone();
        let fork_db = fork_db.clone();
        let client = client.clone();
        let next_block = next_block.clone();

        // if we reached the retry limit remove tx from oracles
        if tx.attempts_to_sell >= *MAX_SELL_ATTEMPTS {
            remove_tx_from_oracles(bot.clone(), tx.clone()).await;
            log::warn!("Sell Oracle: Retries >={:?}, Removed tx from oracles", *MAX_SELL_ATTEMPTS);
            continue;
        }

        // ** check the price concurrently
        tokio::spawn(async move {
            // ** caluclate how many blocks have passed since we bought the token
            let blocks_passed = latest_block.number - tx.block_bought;

            // ** get current amount out
            let current_amount_out = simulate_sell(
                None,
                tx.pool,
                next_block.clone(),
                fork_db.clone()
            ).expect("Failed to simulate sell");

            // ** see if we got taxed
            if current_amount_out < (tx.amount_in * 9) / 100 {
                log::info!("Got Taxed 90% + for {:?}", tx.pool.token_1);
                log::info!("GG!");
                // update the retry counter
                let mut bot_guard = bot.write().await;
                bot_guard.update_attempts_to_sell(tx.clone()).await;
                drop(bot_guard);
            }

            // see if the token is pumping
            // make sure we dont hold it forever
            time_check(
                client.clone(),
                next_block.clone(),
                tx.clone(),
                bot.clone(),
                blocks_passed
            ).await.expect("Failed to do time check");

            // ** if we hit our initial profit take target take the initial amount in out + buy cost
            // ** take the initial out only once
            if tx.got_initial_out == false {
                
                if tx.is_pending {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }

                // first check just in case if the token pumped to the moon
                let to_the_moon = current_amount_out >= tx.target_amount_weth;
                let target = (tx.gas_cost + tx.amount_in) * *INITIAL_PROFIT_TAKE;

                // if its not then check if we hit the initial profit take target
                if !to_the_moon && current_amount_out >= target {
                    take_profit(
                        client.clone(),
                        tx.clone(),
                        next_block.clone(),
                        bot.clone()
                    ).await.expect("Failed to take profit");
                }
            } // end if got initial out

            // ** if amount_out_weth is >= target_amount_weth, send the tx
            if current_amount_out >= tx.target_amount_weth {
                process_tx(
                    client.clone(),
                    tx.clone(),
                    next_block.clone(),
                    bot.clone()
                ).await.expect("Failed to process tx");
            }

            // if we dont met target price, skip
            if current_amount_out < tx.target_amount_weth {
                log::info!("Token {:?} amount in {} ETH, current amount out {} ETH ",
                    tx.pool.token_1,
                    convert_wei_to_ether(tx.amount_in),
                    convert_wei_to_ether(current_amount_out)
                );
            }
        }); // end of tokio::spawn
    } // end for loop

    Ok(())
}
