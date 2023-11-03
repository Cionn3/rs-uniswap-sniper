use tokio::sync::broadcast;
use crate::bot::TxData;
use crate::bot::bot_runner::NewSnipeTxEvent;
use crate::oracles::mempool_stream::MemPoolEvent;

use crate::utils::helpers::get_admin_address;
use crate::utils::helpers::get_my_address;

use ethers::prelude::*;
use tokio::sync::Mutex;
use std::sync::Arc;
use crate::oracles::SellOracle;
use revm::db::{ CacheDB, EmptyDB };
use crate::utils::helpers::{
    create_local_client,
    convert_wei_to_ether,
    convert_wei_to_gwei,
    calculate_miner_tip,
};
use crate::bot::send_normal_tx::send_normal_tx;
use crate::utils::simulate::simulate::{
    simulate_sell,
    simulate_sell_after,
    generate_sell_tx_data,
    get_touched_pools,
};
use crate::utils::simulate::insert_pool_storage;
use crate::forked_db::fork_factory::ForkFactory;
use crate::bot::bot_config::BotConfig;
use crate::utils::simulate::SnipeTx;

use super::Pool;

#[derive(Debug, Clone, PartialEq)]
pub struct AntiRugOracle {
    pub tx_data: Vec<SnipeTx>,
}

impl AntiRugOracle {
    pub fn new() -> Self {
        AntiRugOracle { tx_data: Vec::new() }
    }

    pub fn add_tx_data(&mut self, tx_data: SnipeTx) {
        self.tx_data.push(tx_data);
    }

    pub fn remove_tx_data(&mut self, tx_data: SnipeTx) {
        self.tx_data.retain(|x| x != &tx_data);
    }
}

// ** Pushes the new snipe tx data to the AntiRugOracle
pub fn push_tx_data_to_antirug(
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
    mut new_snipe_event_receiver: broadcast::Receiver<NewSnipeTxEvent>
) {
    tokio::spawn(async move {
        while let Ok(snipe_event) = new_snipe_event_receiver.recv().await {
            let snipe_event = match snipe_event {
                NewSnipeTxEvent::SnipeTxData(snipe_event) => snipe_event,
            };

            let mut oracle = anti_rug_oracle.lock().await;
            oracle.add_tx_data(snipe_event);
        }
    });
}

pub fn start_anti_rug(
    bot_config: BotConfig,
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
    sell_oracle: Arc<Mutex<SellOracle>>,
    mut new_mempool_receiver: broadcast::Receiver<MemPoolEvent>
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


            // start the anti rug oracle by subscribing to pending txs
            while let Ok(event) = new_mempool_receiver.recv().await {
                let pending_tx = match event {
                    MemPoolEvent::NewTx { tx } => tx,
                };
                // ** Get the snipe tx data from the oracle
                let oracle = anti_rug_oracle.lock().await;
                let snipe_txs = &oracle.tx_data;
                // log::info!("txs len in Anti-Rug: {:?}", snipe_txs.len());

                // ** get the pools from the SnipeTx
                let vec_pools = snipe_txs
                    .iter()
                    .map(|x| x.pool.clone())
                    .collect::<Vec<Pool>>();

                // ** no pools yet in the oracle, skip
                if vec_pools.len() == 0 {
                    continue;
                }

                // exclude our address
                if pending_tx.from == get_my_address() || pending_tx.from == get_admin_address() {
                    continue;
                }
                let block_oracle = bot_config.block_oracle.clone();

                let next_block = {
                    let block_oracle = block_oracle.read().await;
                    block_oracle.next_block.clone()
                };

                // get the latest block from oracle
                let latest_block = {
                    let block_oracle = block_oracle.read().await;
                    block_oracle.latest_block.clone()
                };
                let latest_block_number = Some(
                    BlockId::Number(BlockNumber::Number(latest_block.number))
                );

                // ** if we receive an unusual amount of pending txs
                // ** we could limit the connections to the fork factory so it doesnt panic
                // ** but thats a rare case

                // initialize an empty cache db
                let empty_cache_db = CacheDB::new(EmptyDB::default());

                // setup the backend
                let empty_fork_factory = ForkFactory::new_sandbox_factory(
                    client.clone(),
                    empty_cache_db,
                    latest_block_number
                );

                let empty_fork_db = empty_fork_factory.new_sandbox_fork();
                

                // ** first see if the pending_tx touches one of the pools in the oracle
                // ** We want to check if the DeV is trying to rug by removing liquidity
                let touched_pools = if
                    let Ok(Some(tp)) = get_touched_pools(
                        &pending_tx,
                        &next_block,
                        vec_pools,
                        empty_fork_db.clone()
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
                        let anti_rug_oracle_clone = anti_rug_oracle.clone();
                        let sell_oracle = sell_oracle.clone();
                        let client = client.clone();
                        let next_block = next_block.clone();
                        let pending_tx = pending_tx.clone();
                        let empty_fork_db = empty_fork_db.clone();
                        let latest_block_number = latest_block_number.clone();

                        tokio::spawn(async move {
                            // ** First simulate a sell tx before the pending tx

                            // ** setup a new backend and populate it with the pool storage

                            // initialize cache db by inserting pool storage
                            let cache_db = match
                                insert_pool_storage(
                                    client.clone(),
                                    pool,
                                    latest_block_number.clone()
                                ).await
                            {
                                Ok(cache_db) => cache_db,
                                Err(e) => {
                                    log::error!("Failed to insert pool storage: {:?}", e);
                                    return;
                                }
                            };

                            // setup fork factory backend
                            let fork_factory = ForkFactory::new_sandbox_factory(
                                client.clone(),
                                cache_db,
                                latest_block_number.clone()
                            );

                            let fork_db = fork_factory.new_sandbox_fork();

                            // ** get the amount_out in weth before the pending tx
                            let amount_out_before = match
                                simulate_sell(pool, next_block.clone(), fork_db)
                            {
                                Ok(amount_out) => amount_out,
                                Err(e) => {
                                    // TODO remove the tx from the oracle
                                    // if we we get an error here GG
                                    log::error!(
                                        "Failed to simulate Anti-Rug Before sell tx for Token: {:?} Err {:?}",
                                        pool.token_1,
                                        e
                                    );
                                    return;
                                }
                            };

                            // ** get the amount_out in weth after the pending tx
                            
                            let amount_out_after = match
                                simulate_sell_after(
                                    &pending_tx,
                                    pool,
                                    next_block.clone(),
                                    empty_fork_db
                                )
                            {
                                Ok(amount_out) => amount_out,
                                Err(e) => {
                                    log::error!(
                                        "Failed to simulate Anti-Rug After sell tx for Token: {:?} Err {:?}",
                                        pool.token_1,
                                        e
                                    );
                                    return;
                                }
                            };

                            // ** EXTRA SAFE VERSION
                            // ** compare the amount_out_before and amount_out_after
                            // ** if amount_out_after is at least 20% less than amount_out_before
                            // ** Frontrun the pending tx
                            if amount_out_after < (amount_out_before * 8) / 10 {
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
                                log::info!("Amount out After Raw: {:?}", amount_out_after);

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
                                        log::error!("Failed to generate tx_data: {:?}", e);
                                        return;
                                    }
                                };

                                // ** replace tx_data
                                let tx_data = TxData {
                                    tx_call_data: tx_data.tx_call_data,
                                    access_list: tx_data.access_list,
                                    gas_used: tx_data.gas_used,
                                    expected_amount: tx_data.expected_amount,
                                    sniper_contract_address: tx_data.sniper_contract_address,
                                    pending_tx: pending_tx.clone(),
                                    frontrun_or_backrun: U256::zero(), // 0 because we do frontrun
                                };

                                // if the tx is legacy should return 0
                                let pending_tx_priority_fee = pending_tx.max_priority_fee_per_gas.unwrap_or_default();


                                // ** calculate miner tip based on the pending tx priority fee
                                let miner_tip = calculate_miner_tip(pending_tx_priority_fee);

                                // ** max fee per gas must always be higher than miner tip
                                let max_fee_per_gas = next_block.base_fee + miner_tip;

                                // ** First check if its worth it to frontrun the tx
                                // ** calculate the total gas cost
                                let total_gas_cost =
                                    (next_block.base_fee + miner_tip) * tx_data.gas_used;

                                if total_gas_cost > tx_data.expected_amount {
                                    log::error!(
                                        "Anti-Rug🚨: Doesnt Worth to escape the rug pool, GG"
                                    );
                                    // ** find the corrosponding SnipeTx from the pool address
                                    let snipe_tx = snipe_txs
                                        .iter()
                                        .find(|&x| x.pool.address == pool.address)
                                        .unwrap();

                                    // ** remove the tx from the anti-rug and sell oracle
                                    let mut oracle = anti_rug_oracle_clone.lock().await;
                                    oracle.remove_tx_data(snipe_tx.clone());
                                    let mut sell_oracle = sell_oracle.lock().await;
                                    sell_oracle.remove_tx_data(snipe_tx.clone());
                                    return;
                                }

                                log::info!("Escaping Rug!🚀");
                                log::info!(
                                    "Pending tx priority fee: {:?}",
                                    pending_tx_priority_fee
                                );
                                log::info!("Our Miner Tip: {:?}", convert_wei_to_gwei(miner_tip));

                                // ** Send the Tx
                                // Because we want immedietly sell the token in the next block
                                // We are sending the tx without Mev builders, hoping that they will respect the priority fee
                                // The reason behind this even if we tip them a good amount of ether they can still reject the bundle
                                let is_bundle_included = match
                                    send_normal_tx(
                                        client.clone(),
                                        tx_data,
                                        next_block.clone(),
                                        miner_tip,
                                        max_fee_per_gas
                                    ).await
                                {
                                    Ok(is_included) => is_included,
                                    Err(e) => {
                                        log::error!("Failed to send tx: {:?}", e);
                                        return;
                                    }
                                };

                                // ** if bundle is included remove the SnipeTx from the oracle
                                if is_bundle_included {
                                    log::info!("Bundle included we escaped the rug pool!🚀");

                                    // ** find the corrosponding SnipeTx from the pool address
                                    let snipe_tx = snipe_txs
                                        .iter()
                                        .find(|&x| x.pool.address == pool.address)
                                        .unwrap();

                                    // ** remove the tx from the anti-rug and sell oracle
                                    let mut oracle = anti_rug_oracle_clone.lock().await;
                                    oracle.remove_tx_data(snipe_tx.clone());
                                    let mut sell_oracle = sell_oracle.lock().await;
                                    sell_oracle.remove_tx_data(snipe_tx.clone());
                                    log::info!("SnipeTx removed from the oracles");
                                } else {
                                    log::error!("Bundle not included, we are getting rugged! GG");
                                    let snipe_tx = snipe_txs
                                        .iter()
                                        .find(|&x| x.pool.address == pool.address)
                                        .unwrap();

                                    // ** remove the tx from the anti-rug and sell oracle
                                    let mut oracle = anti_rug_oracle_clone.lock().await;
                                    oracle.remove_tx_data(snipe_tx.clone());
                                    let mut sell_oracle = sell_oracle.lock().await;
                                    sell_oracle.remove_tx_data(snipe_tx.clone());
                                    log::info!("SnipeTx removed from the oracles");
                                    return;
                                }
                            } // end of if amount_out_after < amount_out_before * 8 / 10
                        }); // end of tokio::spawn
                    } // end of for loop
                } // end of if touched_pools.len() > 0
                // TODO: could Move Anti-Honeypot here
            } // end of while pending txs loop
        } // end of main loop
    }); // end of main tokio::spawn
}


// Checks for transactions that touches the token contract address
pub fn anti_honeypot(
    bot_config: BotConfig,
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
    sell_oracle: Arc<Mutex<SellOracle>>,
    mut new_mempool_receiver: broadcast::Receiver<MemPoolEvent>
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

            // start the anti honeypot oracle by subscribing to pending txs
            while let Ok(event) = new_mempool_receiver.recv().await {
                let pending_tx = match event {
                    MemPoolEvent::NewTx { tx } => tx,
                };
                // ** Get the snipe tx data from the oracle
                let oracle = anti_rug_oracle.lock().await;
                let snipe_txs = &oracle.tx_data;
                // log::info!("txs len in Anti-Honeypot: {:?}", snipe_txs.len());

                // ** get the pools from the SnipeTx
                let vec_pools = snipe_txs
                    .iter()
                    .map(|x| x.pool.clone())
                    .collect::<Vec<Pool>>();

                // ** no pools yet in the oracle, skip
                if vec_pools.len() == 0 {
                    continue;
                }

                // excluded our address
                if pending_tx.from == get_my_address() || pending_tx.from == get_admin_address() {
                    continue;
                }

                // ** Check if pending_tx.to matches one of the token addresses in vec_pools
                let is_pending_to_token = vec_pools
                    .iter()
                    .any(|x| pending_tx.to == Some(x.token_1));

                // ** Clone vars
                let client = client.clone();
                let anti_rug_oracle = anti_rug_oracle.clone();
                let sell_oracle = sell_oracle.clone();
                let snipe_txs = snipe_txs.clone();
                let block_oracle = bot_config.block_oracle.clone();

                tokio::spawn(async move {
                    // ** if pending_tx.to matches one of the token addresses in vec_pools
                    if is_pending_to_token {
                        // ** get the pool that matches the pending_tx.to
                        let touched_pool = vec_pools
                            .iter()
                            .find(|x| pending_tx.to == Some(x.token_1))
                            .unwrap();

                        // get the next block
                        let next_block = {
                            let block_oracle = block_oracle.read().await;
                            block_oracle.next_block.clone()
                        };
                        // get the latest block from oracle
                        let latest_block = {
                            let block_oracle = block_oracle.read().await;
                            block_oracle.latest_block.clone()
                        };
                        let latest_block_number = Some(
                            BlockId::Number(BlockNumber::Number(latest_block.number))
                        );

                        // initialize the cache db by inserting pool storage
                        let cache_db = match
                            insert_pool_storage(
                                client.clone(),
                                *touched_pool,
                                latest_block_number.clone()
                            ).await
                        {
                            Ok(cache_db) => cache_db,
                            Err(e) => {
                                log::error!("Failed to insert pool storage: {:?}", e);
                                return;
                            }
                        };

                        // setup fork factory backend
                        let fork_factory = ForkFactory::new_sandbox_factory(
                            client.clone(),
                            cache_db,
                            latest_block_number.clone()
                        );
                        let fork_db = fork_factory.new_sandbox_fork();

                        // ** setup an empty cache db
                        let empty_cache_db = CacheDB::new(EmptyDB::default());

                        // setup the backend
                        let empty_fork_factory = ForkFactory::new_sandbox_factory(
                            client.clone(),
                            empty_cache_db,
                            latest_block_number
                        );

                        // ** First simulate the sell tx before the pending tx
                        // ** here we use the backend with the populated db
                        let amount_out_before = match
                            simulate_sell(touched_pool.clone(), next_block.clone(), fork_db.clone())
                        {
                            Ok(amount_out) => amount_out,
                            Err(e) => {
                                log::error!("Failed to simulate sell tx: {:?}", e);
                                return;
                            }
                        };

                        // ** get the amount_out in weth after the pending tx
                        // ** Here we use an empty db cause we do the sell after the pending tx
                        let amount_out_after = match
                            simulate_sell_after(
                                &pending_tx,
                                *touched_pool,
                                next_block.clone(),
                                empty_fork_factory.new_sandbox_fork()
                            )
                        {
                            Ok(amount_out) => amount_out,
                            Err(e) => {
                                log::error!("Failed to simulate sell tx: {:?}", e);
                                return;
                            }
                        };

                        // ** EXTRA SAFE VERSION
                        // ** compare the amount_out_before and amount_out_after
                        // ** if amount_out_after is at least 20% less than amount_out_before
                        // ** Frontrun the pending tx
                        if amount_out_after < (amount_out_before * 8) / 10 {
                            log::info!("Anti-HoneyPot Alert!🚨 Possible rug detected!");
                            log::info!("Detected Tx Hash: {:?}", pending_tx.hash);
                            log::info!(
                                "Amount out Before: ETH {:?}",
                                convert_wei_to_ether(amount_out_before)
                            );
                            log::info!(
                                "Amount out After: ETH {:?}",
                                convert_wei_to_ether(amount_out_after)
                            );
                            log::info!("Amount out After Raw: {:?}", amount_out_after);

                            // ** generate tx_data
                            // ** We use the populated backend
                            let tx_data = match
                                generate_sell_tx_data(
                                    *touched_pool,
                                    next_block.clone(),
                                    fork_db
                                )
                            {
                                Ok(tx) => tx,
                                Err(e) => {
                                    log::error!("Failed to generate tx_data: {:?}", e);
                                    return;
                                }
                            };

                            // replace tx_data
                            let tx_data = TxData {
                                tx_call_data: tx_data.tx_call_data,
                                access_list: tx_data.access_list,
                                gas_used: tx_data.gas_used,
                                expected_amount: tx_data.expected_amount,
                                sniper_contract_address: tx_data.sniper_contract_address,
                                pending_tx: pending_tx.clone(),
                                frontrun_or_backrun: U256::zero(), // 0 because we do frontrun
                            };

                            // if the tx is legacy should return 0
                            let pending_tx_priority_fee = pending_tx.max_priority_fee_per_gas.unwrap_or_default();


                            // ** calculate miner tip
                            let miner_tip = calculate_miner_tip(pending_tx_priority_fee);

                            // ** max fee per gas must always be higher than miner tip
                            let max_fee_per_gas = next_block.base_fee + miner_tip;

                            // ** First check if its worth it to frontrun the tx
                            // ** calculate the total gas cost
                            let total_gas_cost =
                                (next_block.base_fee + miner_tip) * tx_data.gas_used;

                            if total_gas_cost > tx_data.expected_amount {
                                log::error!(
                                    "Anti-Honeypot🚨: Doesnt Worth to escape the rug pool, GG"
                                );
                                // ** find the corrosponding SnipeTx from the touched pool address
                                let snipe_tx = snipe_txs
                                    .iter()
                                    .find(|&x| x.pool.address == touched_pool.address)
                                    .unwrap();

                                // ** remove the tx from the oracles
                                let mut oracle = anti_rug_oracle.lock().await;
                                oracle.remove_tx_data(snipe_tx.clone());
                                let mut sell_oracle = sell_oracle.lock().await;
                                sell_oracle.remove_tx_data(snipe_tx.clone());
                                log::info!("SnipeTx removed from the oracles");
                                return;
                            }
                            log::info!("Escaping HoneyPot!🚀");
                            log::info!("Pending tx priority fee: {:?}", convert_wei_to_gwei(pending_tx_priority_fee));
                            log::info!("Our Miner Tip: {:?}", convert_wei_to_gwei(miner_tip));

                            // ** Send the Tx to Flashbots
                            // Because we want immedietly sell the token in the next block
                            // We are sending the tx without Mev builders, hoping that they will respect the priority fee

                            let is_bundle_included = match
                                send_normal_tx(
                                    client.clone(),
                                    tx_data,
                                    next_block.clone(),
                                    miner_tip,
                                    max_fee_per_gas
                                ).await
                            {
                                Ok(is_included) => is_included,
                                Err(e) => {
                                    log::error!("Failed to send tx: {:?}", e);
                                    return;
                                }
                            };

                            // ** if bundle is included remove the SnipeTx from the oracle
                            if is_bundle_included {
                                log::info!("Bundle included we escaped the rug pool!🚀");

                                // ** find the corrosponding SnipeTx from the touched pool address
                                let snipe_tx = snipe_txs
                                    .iter()
                                    .find(|&x| x.pool.address == touched_pool.address)
                                    .unwrap();

                                // ** remove the tx from the oracles
                                let mut oracle = anti_rug_oracle.lock().await;
                                oracle.remove_tx_data(snipe_tx.clone());
                                let mut sell_oracle = sell_oracle.lock().await;
                                sell_oracle.remove_tx_data(snipe_tx.clone());
                                log::info!("SnipeTx removed from the oracles");
                            } else {
                                log::error!("Bundle not included, we are getting rugged! GG");
                                // ** find the corrosponding SnipeTx from the touched pool address
                                let snipe_tx = snipe_txs
                                    .iter()
                                    .find(|&x| x.pool.address == touched_pool.address)
                                    .unwrap();

                                // ** remove the tx from the oracles
                                let mut oracle = anti_rug_oracle.lock().await;
                                oracle.remove_tx_data(snipe_tx.clone());
                                let mut sell_oracle = sell_oracle.lock().await;
                                sell_oracle.remove_tx_data(snipe_tx.clone());
                                log::info!("SnipeTx removed from the oracles");
                                return;
                            }
                        } // end of if amount_out_after < amount_out_before * 8 / 10
                    } // end of if is_pending_to_token
                }); // end of tokio::spawn
            } // end of while pending txs loop
        } // end of main loop
    }); // end of main tokio::spawn
}