use tokio::sync::broadcast;
use ethers::prelude::*;

use tokio::sync::Mutex;
use std::sync::Arc;
use anyhow::anyhow;

use crate::oracles::AntiRugOracle;
use crate::oracles::SellOracle;
use crate::oracles::pair_oracle::{ Pool, NewPairEvent };
use crate::oracles::block_oracle::NewBlockEvent;
use ethers::types::transaction::eip2930::AccessList;
use crate::forked_db::fork_factory::ForkFactory;
use crate::utils::simulate::insert_pool_storage;
use crate::utils::simulate::simulate::{ tax_check, generate_buy_tx_data, transfer_check, SnipeTx };
use crate::utils::helpers::{ calculate_miner_tip, convert_wei_to_gwei, create_local_client };
use super::send_tx::send_tx;
use super::TxData;
use revm::db::{ CacheDB, EmptyDB };

use super::bot_config::BotConfig;

#[derive(Debug, Clone)]
pub enum NewSnipeTxEvent {
    SnipeTxData(SnipeTx),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetryOracle {
    pub tx_data: Vec<SnipeTx>,
}

impl RetryOracle {
    pub fn new() -> Self {
        RetryOracle { tx_data: Vec::new() }
    }

    pub fn add_tx_data(&mut self, tx_data: SnipeTx) {
        self.tx_data.push(tx_data);
    }

    pub fn remove_tx_data(&mut self, tx_data: SnipeTx) {
        self.tx_data.retain(|x| x != &tx_data);
    }
}

pub fn start_sniper(
    bot_config: BotConfig,
    mut new_pair_receiver: broadcast::Receiver<NewPairEvent>,
    new_snipe_event_sender: broadcast::Sender<NewSnipeTxEvent>,
    sell_oracle: Arc<Mutex<SellOracle>>,
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
    retry_oracle: Arc<Mutex<RetryOracle>>
) {
    tokio::spawn(async move {
        while let Ok(event) = new_pair_receiver.recv().await {
            match event {
                NewPairEvent::NewPairWithTx { pool, tx } => {
                    // log::info!("Received new pool event: {:?}", pool);
                    // log::info!("Received pending tx event: {:?}", tx);

                    // process the tx
                    match
                        process_tx(
                            bot_config.clone(),
                            pool,
                            tx,
                            new_snipe_event_sender.clone(),
                            sell_oracle.clone(),
                            anti_rug_oracle.clone(),
                            retry_oracle.clone()
                        ).await
                    {
                        Ok(_) => log::info!("Tx Sent Successfully"),
                        Err(e) =>
                            log::error!("Sniper Failed: for token {:?} Err {:?}", pool.token_1, e),
                    }
                }
            }
        }
    });
}

async fn process_tx(
    bot_config: BotConfig,
    pool: Pool,
    pending_tx: Transaction,
    new_snipe_event_sender: broadcast::Sender<NewSnipeTxEvent>,
    sell_oracle: Arc<Mutex<SellOracle>>,
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
    retry_oracle: Arc<Mutex<RetryOracle>>
) -> Result<(), anyhow::Error> {
    let client = bot_config.client.clone();
    let block_oracle = bot_config.block_oracle.clone();
    let amount_in = bot_config.initial_amount_in_weth.clone();

    // get the next block
    let next_block = {
        let block_oracle = block_oracle.read().await;
        block_oracle.next_block.clone()
    };

    // prepare for simulations

    // get the latest blockId
    let latest_block = client.get_block_number().await.unwrap();
    let latest_block = Some(BlockId::Number(BlockNumber::Number(latest_block)));

    // initialize an empty cache db
    let cache_db = CacheDB::new(EmptyDB::default());

    // setup fork factory backend
    let fork_factory = ForkFactory::new_sandbox_factory(client.clone(), cache_db, latest_block);

    // simulate the tx
    let is_swap_success = match
        tax_check(
            &pool,
            amount_in.clone(),
            &next_block,
            Some(pending_tx.clone()),
            fork_factory.new_sandbox_fork()
        )
    {
        Ok(result) => result,
        Err(e) => {
            // log::error!("Tax Check Failed: {:?}", e);
            return Err(anyhow!("Tax Check Failed: {:?}", e));
        }
    };

    if !is_swap_success {
        // generate snipe_tx data
        let snipe_tx = SnipeTx {
            pool: pool.clone(),
            amount_in: amount_in.clone(),
            tx_call_data: Bytes::new(),
            access_list: AccessList::default(),
            gas_used: (0u64).into(),
            sniper_contract_address: Address::zero(),
            pending_tx: pending_tx.clone(),
            block_bought: next_block.number,
        };
        // push it to retry oracle
        let mut retry_oracle = retry_oracle.lock().await;
        retry_oracle.add_tx_data(snipe_tx.clone());
        return Err(anyhow!("Snipe Failed, sent it to retry oracle"));
    }

    // ** Do transfer checks
    let _transfer_result = match
        transfer_check(
            &pool,
            amount_in.clone(),
            &next_block,
            Some(pending_tx.clone()),
            fork_factory.new_sandbox_fork()
        )
    {
        Ok(result) => result,
        Err(e) => {
            // log::error!("Tax Check Failed: {:?}", e);
            return Err(anyhow!("Transfer Check Failed: {:?}", e));
        }
    };

    // simulate the tx once again and generate accesslist

    let snipe_tx = match
    generate_buy_tx_data(
            &pool,
            amount_in,
            &next_block,
            Some(pending_tx.clone()),
            fork_factory.new_sandbox_fork()
        )
    {
        Ok(result) => result,
        Err(e) => {
            // log::error!("Final Check Failed: {:?}", e);
            return Err(anyhow!("Generating Access List Failed: {:?}", e));
        }
    };

    // if the tx is legacy should return 0
    let pending_tx_priority_fee = pending_tx.max_priority_fee_per_gas.unwrap_or_default();

    // ** calculate the miner tip
    let miner_tip = calculate_miner_tip(pending_tx_priority_fee);
    log::info!("Sniping with miner tip: {:?}", convert_wei_to_gwei(miner_tip));

    // ** max fee per gas must always be higher than base fee
    let max_fee_per_gas = next_block.base_fee + miner_tip;

    let expected_amount = U256::zero();

    // ** create the tx data for the bundle
    let tx_data = TxData {
        tx_call_data: snipe_tx.tx_call_data.clone(),
        access_list: snipe_tx.access_list.clone(),
        gas_used: snipe_tx.gas_used.clone(),
        expected_amount,
        sniper_contract_address: snipe_tx.sniper_contract_address.clone(),
        pending_tx: pending_tx.clone(),
        frontrun_or_backrun: U256::from(1u128), // 1 because we do backrun
    };

    // ** calculate the total gas cost
    let total_gas_cost = (next_block.base_fee + miner_tip) * tx_data.gas_used;

    // ** If gas cost is more than amount_in we dont snipe
    // you can remove this check if you want to snipe anyway
    if total_gas_cost > snipe_tx.amount_in {
        return Err(anyhow!("Gas Cost Is Higher Than Amount In"));
    }

    // ** Send The Tx To Flashbots **
    log::info!("Token {:?} Passed All Checks! 🚀", pool.token_1);
    log::info!("Sending Tx...");

    // send the new tx to oracles before sending to flashbots
    // it takes time to get the bundle response
    // We dont want to get rugged while we wait

    // send the new snipe event
    new_snipe_event_sender.send(NewSnipeTxEvent::SnipeTxData(snipe_tx.clone())).unwrap();
    log::info!("New Snipe Event Sent To Sell Oracle! 🚀");

    let is_bundle_included = match
        send_tx(
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
            //return Err(anyhow!("Failed to send tx to flashbots: {:?}", e));
            false
        }
    };

    // send the new snipe event only if the bundle is included
    if is_bundle_included {
        // send the new snipe event
        // new_snipe_event_sender.send(NewSnipeTxEvent::SnipeTxData(snipe_tx)).unwrap();

    } else {
        // remove the snipe_tx from the oracles
        let mut sell_oracle = sell_oracle.lock().await;
        sell_oracle.remove_tx_data(snipe_tx.clone());
        let mut anti_rug_oracle = anti_rug_oracle.lock().await;
        anti_rug_oracle.remove_tx_data(snipe_tx.clone());
        log::info!("Snipe Tx Removed From Oracles! 🚀");
        return Err(anyhow!("Bundle Not Included"));
    }

    Ok(())
}

pub fn snipe_retry(
    bot_config: BotConfig,
    new_snipe_event_sender: broadcast::Sender<NewSnipeTxEvent>,
    sell_oracle: Arc<Mutex<SellOracle>>,
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
    retry_oracle: Arc<Mutex<RetryOracle>>,
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
                let latest_block = match event {
                    NewBlockEvent::NewBlock { latest_block } => latest_block,
                };

                // ** Get the snipe tx data from the oracle

                let snipe_txs = {
                    let oracle = retry_oracle.lock().await;
                    oracle.tx_data.clone()
                };

                // ** if there are no txs in the oracle, skip
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

                let latest_block_number = Some(
                    BlockId::Number(BlockNumber::Number(latest_block.number))
                );

                for tx in snipe_txs {
                    let new_snipe_event_sender = new_snipe_event_sender.clone();
                    let sell_oracle = sell_oracle.clone();
                    let anti_rug_oracle = anti_rug_oracle.clone();
                    let retry_oracle = retry_oracle.clone();

                    // we can use block_bought as the block
                    // that the tx entered into the retry oracle

                    // for the first iteration should return true
                    // and then false so we dont try again
                    let blocks_passed = latest_block.number == tx.block_bought;
                    // only retry for the next block
                    if !blocks_passed {
                        let mut retry_oracle = retry_oracle.lock().await;
                        retry_oracle.remove_tx_data(tx.clone());
                        log::info!("Snipe Tx Removed From Retry Oracle! 🚀");
                        continue;
                    }

                    let client = client.clone();
                    let amount_in = bot_config.initial_amount_in_weth.clone();
                    let next_block = next_block.clone();

                    // initialize cache db
                    let cache_db = match
                        insert_pool_storage(
                            client.clone(),
                            tx.pool,
                            latest_block_number.clone()
                        ).await
                    {
                        Ok(cache_db) => cache_db,
                        Err(e) => {
                            log::error!("Failed to insert pool storage: {:?}", e);
                            continue;
                        }
                    };

                    // setup fork factory backend
                    let fork_factory = ForkFactory::new_sandbox_factory(
                        client.clone(),
                        cache_db,
                        latest_block_number
                    );

                    tokio::spawn(async move {
                        // simulate the tx
                        let swap = match
                            tax_check(
                                &tx.pool,
                                amount_in.clone(),
                                &next_block,
                                None,
                                fork_factory.new_sandbox_fork()
                            )
                        {
                            Ok(result) => result,
                            Err(e) => {
                                log::error!("Retry Tax Check Failed: {:?}", e);
                                return;
                            }
                        };
                        if !swap {
                            // remove tx from retry oracle
                            let mut retry_oracle = retry_oracle.lock().await;
                            retry_oracle.remove_tx_data(tx.clone());
                            log::info!("Retry failed, removed from retry oracle! 🚀");
                            return;
                        }

                        // ** Do transfer checks
                        let _transfer_result = match
                            transfer_check(
                                &tx.pool,
                                amount_in.clone(),
                                &next_block,
                                None,
                                fork_factory.new_sandbox_fork()
                            )
                        {
                            Ok(result) => result,
                            Err(e) => {
                                log::error!("Retry Tax Check Failed: {:?}", e);
                                return;
                            }
                        };

                        // simulate the tx once again and generate accesslist

                        let snipe_tx = match
                        generate_buy_tx_data(
                                &tx.pool,
                                amount_in.clone(),
                                &next_block,
                                None,
                                fork_factory.new_sandbox_fork()
                            )
                        {
                            Ok(result) => result,
                            Err(e) => {
                                log::error!("Generating access list failed: {:?}", e);
                                return;
                            }
                        };

                        // change the miner tip as you like
                        let miner_tip = U256::from(1000000000u128); // 1 gwei
                        log::info!("Sniping with 1 gwei");

                        // ** max fee per gas must always be higher than miner tip
                        let max_fee_per_gas = next_block.base_fee + miner_tip;

                        // just zero
                        let expected_amount = U256::zero();

                        // ** create the tx data for the bundle
                        let tx_data = TxData {
                            tx_call_data: snipe_tx.tx_call_data.clone(),
                            access_list: snipe_tx.access_list.clone(),
                            gas_used: snipe_tx.gas_used.clone(),
                            expected_amount,
                            sniper_contract_address: snipe_tx.sniper_contract_address.clone(),
                            pending_tx: snipe_tx.pending_tx.clone(),
                            frontrun_or_backrun: U256::from(2u128), // 2 cause we dont backrun or frontrun
                        };

                        // ** calculate the total gas cost
                        let total_gas_cost = (next_block.base_fee + miner_tip) * tx_data.gas_used;

                        // ** If gas cost is more than amount_in we dont snipe
                        // you can remove this check if you want to snipe anyway
                        if total_gas_cost > snipe_tx.amount_in {
                            log::error!("Gas Cost Is Higher Than Amount In");
                            return;
                        }

                        // send the new snipe event
                        new_snipe_event_sender
                            .send(NewSnipeTxEvent::SnipeTxData(snipe_tx.clone()))
                            .unwrap();
                        log::info!("Retry: New Tx Sent to Oracles! 🚀");

                        let is_bundle_included = match
                            send_tx(
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
                                false // set is_bundle_included to false if there's an error
                            }
                        };

                        if is_bundle_included {
                            // remove it from the retry oracle
                            let mut retry_oracle = retry_oracle.lock().await;
                            retry_oracle.remove_tx_data(snipe_tx.clone());
                        } else {
                            // remove the snipe_tx from the sell and antirug oracles
                            let mut sell_oracle = sell_oracle.lock().await;
                            sell_oracle.remove_tx_data(snipe_tx.clone());
                            let mut anti_rug_oracle = anti_rug_oracle.lock().await;
                            anti_rug_oracle.remove_tx_data(snipe_tx.clone());

                            log::info!("Snipe Tx Removed From Oracles! 🚀");
                            log::error!("Bundle Not Included");
                            return;
                        }
                    }); // end of tokio::spawn
                } // end of for loop
            } // end of while loop
        } // end of loop
    }); // end of tokio::spawn
}