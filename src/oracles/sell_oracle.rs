use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::broadcast;

use crate::oracles::AntiRugOracle;
use crate::utils::helpers::{ create_local_client, convert_wei_to_ether };
use crate::oracles::block_oracle::NewBlockEvent;

use crate::bot::send_normal_tx::send_normal_tx;

use super::BlockInfo;
use crate::utils::simulate::simulate::{ simulate_sell, generate_sell_tx_data };
use crate::utils::simulate::insert_pool_storage;
use crate::bot::bot_runner::NewSnipeTxEvent;
use crate::utils::simulate::SnipeTx;
use crate::bot::bot_config::BotConfig;
use crate::forked_db::fork_factory::ForkFactory;
use anyhow::anyhow;

#[derive(Debug, Clone, PartialEq)]
pub struct SellOracle {
    pub tx_data: Vec<SnipeTx>,
}

impl SellOracle {
    pub fn new() -> Self {
        SellOracle { tx_data: Vec::new() }
    }

    pub fn add_tx_data(&mut self, tx_data: SnipeTx) {
        self.tx_data.push(tx_data);
    }

    pub fn remove_tx_data(&mut self, tx_data: SnipeTx) {
        self.tx_data.retain(|x| x != &tx_data);
    }
}

// ** Pushes the new snipe tx data to the SellOracle
pub fn push_tx_data_to_sell_oracle(
    shared_oracle: Arc<Mutex<SellOracle>>,
    mut new_snipe_event_receiver: broadcast::Receiver<NewSnipeTxEvent>
) {
    tokio::spawn(async move {
        while let Ok(snipe_event) = new_snipe_event_receiver.recv().await {
            let snipe_event = match snipe_event {
                NewSnipeTxEvent::SnipeTxData(snipe_event) => snipe_event,
            };

            let mut oracle = shared_oracle.lock().await;
            oracle.add_tx_data(snipe_event);
        }
    });
}

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
                let oracle = shared_oracle.lock().await;
                let snipe_txs = &oracle.tx_data;

                // ** if there are no txs in the oracle, continue
                if snipe_txs.len() == 0 {
                    continue;
                }

                let client = client.clone();

                let block_oracle = bot_config.block_oracle.clone();

                // get the next block
                let next_block = {
                    let block_oracle = block_oracle.read().await;
                    block_oracle.next_block.clone()
                };

                log::info!("Latest Block From Sell Oracle: {:?}", latest_block.number);
                let latest_block_number = Some(
                    BlockId::Number(BlockNumber::Number(latest_block.number))
                );

                for tx in snipe_txs {
                    let shared_oracle_clone = shared_oracle.clone();
                    let anti_rug_oracle = anti_rug_oracle.clone();

                    let tx = tx.clone();
                    let client = client.clone();

                    // ** The pool of the token we are selling
                    let pool = tx.pool.clone();

                    // ** The Initial Amount in in WETH
                    let initial_amount_in = tx.amount_in.clone();

                    // target amount to sell
                    // by default we are looking for 2x
                    let mut target_amount_weth;

                    let next_block = next_block.clone();
                    let one_eth = U256::from(1000000000000000000u128);
                    let two_eth = U256::from(2000000000000000000u128);
                    let five_eth = U256::from(5000000000000000000u128);

                    // match pool.weth_liquidity to set different lvls for target_amount_weth
                    match tx.pool.weth_liquidity {
                        liq if liq >= one_eth && liq <= two_eth => {
                            // ** if liquidity is between 1 and 2 eth
                            // ** we are looking for 3x
                            target_amount_weth = initial_amount_in * 3;
                        }
                        liq if liq > two_eth && liq <= five_eth => {
                            // ** if liquidity is between 2 and 3 eth
                            // ** we are looking for 2x
                            target_amount_weth = initial_amount_in * 2;
                        }
                        _ => {
                            // ** if liquidity is more than 5 eth
                            // ** we are looking for 1.5x
                            target_amount_weth = (initial_amount_in * 15) / 10;
                        }
                    }

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
                        // let between_1_to_5_min =
                        //   blocks_passed >= (5u64).into() && blocks_passed <= (25u64).into();

                        // if 2 mins have passed check the price
                        if is_2_min_passed {
                            // ** calculate the target price difference (20% gain)
                            let target_price_difference = (initial_amount_in * 120) / 100;

                            match
                                process_tx(
                                    client.clone(),
                                    tx.clone(),
                                    next_block.clone(),
                                    latest_block_number.clone(),
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
                                    latest_block_number.clone(),
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
                                    latest_block_number.clone(),
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
                                    latest_block_number.clone(),
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
                            insert_pool_storage(
                                client.clone(),
                                pool.clone(),
                                latest_block_number.clone()
                            ).await
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
                                    "Failed to simulate sell for token: {:?} Error: {:?}",
                                    pool.token_1,
                                    e
                                );
                                // ** if we get an error here GG
                                // ** remove the tx from the oracles
                                let mut oracle = shared_oracle_clone.lock().await;
                                oracle.remove_tx_data(tx.clone());
                                let mut anti_rug_oracle = anti_rug_oracle.lock().await;
                                anti_rug_oracle.remove_tx_data(tx.clone());
                                log::warn!("We got Rugged, Removed tx from oracles");

                                return;
                            }
                        };

                        // check if we got a bad position
                        // We do this check only once, when the tx is added to the oracle
                        // to do this check only once we check if current_block is equal to block_bought

                        if latest_block.number == tx.block_bought {
                            // If the amount_out is less than 80% of the initial amount in, we probably got a bad position
                            if amount_out_weth < (initial_amount_in * 8) / 10 {
                                // set the target_amount_weth to 1.5x
                                target_amount_weth = (initial_amount_in * 15) / 10;
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
                                    pool.clone(),
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
                            let miner_tip = U256::from(10000000000u128); // 10 gwei

                            // ** max fee per gas must always be higher than miner tip
                            let max_fee_per_gas = next_block.base_fee + miner_tip;

                            // ** Send The Tx **
                            // use send_tx module to send the tx to flashbots
                            // but because we are just selling we are not exposed to frontrunning
                            let is_bundle_included = match
                                send_normal_tx(
                                    client.clone(),
                                    tx_data.clone(),
                                    next_block.clone(),
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
                                let mut oracle = shared_oracle_clone.lock().await;
                                oracle.remove_tx_data(tx.clone());
                                let mut anti_rug_oracle = anti_rug_oracle.lock().await;
                                anti_rug_oracle.remove_tx_data(tx.clone());
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
        insert_pool_storage(
            client.clone(),
            snipe_tx.pool.clone(),
            latest_block_number.clone()
        ).await
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
        simulate_sell(snipe_tx.pool.clone(), next_block.clone(), fork_factory.new_sandbox_fork())
    {
        Ok(result) => result,
        Err(e) => {
            // ** if we get an error here GG
            // ** remove the tx from the oracles
            let mut oracle = shared_oracle.lock().await;
            oracle.remove_tx_data(snipe_tx.clone());
            let mut anti_rug_oracle = anti_rug_oracle.lock().await;
            anti_rug_oracle.remove_tx_data(snipe_tx.clone());
            log::warn!("We got Rugged, Removed tx from oracles");

            return Err(anyhow!("Failed to simulate sell: {:?}", e));
        }
    };

    // ** If amount_out_weth is not at target price
    let is_price_met = amount_out_weth >= target_price_difference;

    // ** if price is not met Sell
    if !is_price_met {
        // ** generate tx_data
        let tx_data = match
            generate_sell_tx_data(
                snipe_tx.pool.clone(),
                next_block.clone(),
                fork_factory.new_sandbox_fork()
            )
        {
            Ok(tx) => tx,
            Err(e) => {
                // ** if we get an error here GG
                // ** remove the tx from the oracles
                let mut oracle = shared_oracle.lock().await;
                oracle.remove_tx_data(snipe_tx.clone());
                let mut anti_rug_oracle = anti_rug_oracle.lock().await;
                anti_rug_oracle.remove_tx_data(snipe_tx.clone());
                log::warn!("We got Rugged, Removed tx from oracles");
                return Err(anyhow!("Failed to generate tx_data: {:?}", e));
            }
        };

        // ** miner tip
        // adjust the tip as you like
        let miner_tip = U256::from(3000000000u128); // 3 gwei

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
            send_normal_tx(
                client.clone(),
                tx_data.clone(),
                next_block.clone(),
                miner_tip,
                max_fee_per_gas
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
                snipe_tx.pool.token_0,
                convert_wei_to_ether(tx_data.expected_amount)
            );
            // ** remove the tx from the oracle
            let mut oracle = shared_oracle.lock().await;
            oracle.remove_tx_data(snipe_tx.clone());
            let mut anti_rug_oracle = anti_rug_oracle.lock().await;
            anti_rug_oracle.remove_tx_data(snipe_tx.clone());
        } else {
            return Err(anyhow!("Bundle not included, will try again in the next block"));
        }
    } // end of if !is_price_met

    // ** if price is met, do nothing
    Ok(())
}
