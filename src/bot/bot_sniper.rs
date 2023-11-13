use tokio::sync::broadcast;
use ethers::prelude::*;

use tokio::sync::Mutex;
use std::sync::Arc;
use anyhow::anyhow;

use ethers::types::transaction::eip2930::AccessList;
use crate::forked_db::fork_factory::ForkFactory;
use crate::utils::simulate::insert_pool_storage;
use crate::utils::simulate::simulate::{ tax_check, generate_buy_tx_data, transfer_check };
use crate::utils::helpers::*;
use crate::oracles::{ add_tx_to_oracles, remove_tx_from_oracles };
use super::send_tx::send_tx;
use crate::utils::types::{ structs::*, events::* };
use revm::db::{ CacheDB, EmptyDB };



pub fn start_sniper(
    mut new_pair_receiver: broadcast::Receiver<NewPairEvent>,
    bot: Arc<Mutex<Bot>>,
    retry_oracle: Arc<Mutex<RetryOracle>>
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
                match event {
                    NewPairEvent::NewPairWithTx { pool, tx } => {
                        // process the tx
                        match
                            process_tx(
                                bot.clone(),
                                retry_oracle.clone(),
                                client.clone(),
                                pool,
                                tx
                            ).await
                        {
                            Ok(_) => log::info!("Tx Sent Successfully"),
                            Err(e) =>
                                log::error!(
                                    "Sniper Failed: for token {:?} Err {:?}",
                                    pool.token_1,
                                    e
                                ),
                        }
                    }
                }
            }
        }
    });
}

async fn process_tx(
    bot: Arc<Mutex<Bot>>,
    retry_oracle: Arc<Mutex<RetryOracle>>,
    client: Arc<Provider<Ws>>,
    pool: Pool,
    pending_tx: Transaction
) -> Result<(), anyhow::Error> {
    // get block info from oracle
    let bot_guard = bot.lock().await;
    let (latest_block, next_block) = bot_guard.get_block_info().await;
    drop(bot_guard);

    let latest_block = Some(BlockId::Number(BlockNumber::Number(latest_block.number)));

    // initialize an empty cache db
    let cache_db = CacheDB::new(EmptyDB::default());

    // setup fork factory backend
    let fork_factory = ForkFactory::new_sandbox_factory(client.clone(), cache_db, latest_block);

    // simulate the tx
    let is_swap_success = match
        tax_check(
            &pool,
            *INITIAL_AMOUNT_IN_WETH,
            &next_block,
            Some(pending_tx.clone()),
            get_transfer_event(),
            get_swap_event(),
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
            pool: pool,
            amount_in: *INITIAL_AMOUNT_IN_WETH,
            expected_amount_of_tokens: U256::zero(),
            target_amount_weth: *TARGET_AMOUNT_TO_SELL,
            tx_call_data: Bytes::new(),
            access_list: AccessList::default(),
            gas_used: 0u64,
            buy_cost: U256::zero(),
            sniper_contract_address: Address::zero(),
            pending_tx: pending_tx.clone(),
            block_bought: next_block.number,
            attempts_to_sell: 0u8, // attempts to sell
            snipe_retries: 0u8, // snipe retries
            is_pending: false,
            retry_pending: false, // retry pending
            reason: 1u8, // 1 means swap failed
            got_initial_out: false,
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
            *INITIAL_AMOUNT_IN_WETH,
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

    log::info!("Sniping with miner tip: {:?}", convert_wei_to_gwei(*MINER_TIP_TO_SNIPE));

    // simulate the tx once again and generate accesslist

    let snipe_tx = match
        generate_buy_tx_data(
            &pool,
            *INITIAL_AMOUNT_IN_WETH,
            &next_block,
            Some(pending_tx.clone()),
            *MINER_TIP_TO_SNIPE,
            get_swap_event(),
            get_transfer_event(),
            fork_factory.new_sandbox_fork()
        )
    {
        Ok(result) => result,
        Err(e) => {
            // log::error!("Final Check Failed: {:?}", e);
            return Err(anyhow!("Generating Access List Failed: {:?}", e));
        }
    };

    // ** max fee per gas must always be higher than base fee
    let max_fee_per_gas = next_block.base_fee + *MINER_TIP_TO_SNIPE;

    let expected_amount = U256::zero();

    // ** create the tx data for the bundle
    let tx_data = TxData {
        tx_call_data: snipe_tx.tx_call_data.clone(),
        access_list: snipe_tx.access_list.clone(),
        gas_used: snipe_tx.gas_used,
        expected_amount,
        sniper_contract_address: snipe_tx.sniper_contract_address,
        pending_tx: pending_tx.clone(),
        frontrun_or_backrun: U256::from(1u128), // 1 because we do backrun
    };

    // ** calculate the total gas cost
    let total_gas_cost = (next_block.base_fee + *MINER_TIP_TO_SNIPE) * tx_data.gas_used;

    // ** If gas cost is more than amount_in we dont snipe
    // you can remove this check if you want to snipe anyway
    if total_gas_cost > snipe_tx.amount_in * 2 {
        return Err(anyhow!("Gas Cost Is Higher Than Amount In"));
    }

    log::info!("Token {:?} Passed All Checks! 🚀", pool.token_1);

    // send the new tx to oracles before sending to flashbots
    // it takes time to get the bundle response
    // We dont want to get rugged while we wait

    // add the snipe_tx to oracles
    add_tx_to_oracles(bot.clone(), snipe_tx.clone()).await;

    // get the nonce and update it
    let mut bot_guard = bot.lock().await;
    let nonce = bot_guard.get_nonce().await;
    drop(bot_guard);

    log::info!("Token {:?} Sent To Oracles! 🚀", pool.token_1);

    // ** Send The Tx To Flashbots **
    let is_bundle_included = match
        send_tx(
            client.clone(),
            tx_data.clone(),
            next_block.clone(),
            *MINER_TIP_TO_SNIPE,
            max_fee_per_gas,
            nonce
        ).await
    {
        Ok(result) => result,
        Err(e) => {
            log::warn!("Failed to send tx to flashbots: {:?}", e);
            //return Err(anyhow!("Failed to send tx to flashbots: {:?}", e));
            false
        }
    };

    // if the bundle is not included
    if is_bundle_included == false {
        // remove the snipe_tx from the oracles
        remove_tx_from_oracles(bot.clone(), snipe_tx.clone()).await;

        log::info!("Token {:?} Removed From Oracles! 🚀", pool.token_1);

        // if our tx is not included its better to remove it and move on
        // as it is possible to get a really bad position and get wrecked most of the times

        return Err(anyhow!("Bundle Not Included"));
    }

    Ok(())
}

pub fn snipe_retry(
    bot: Arc<Mutex<Bot>>,
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
                if snipe_txs.is_empty() {
                    continue;
                }

                let client = client.clone();

                // get the next_block
                let bot_guard = bot.lock().await;
                let (_, next_block) = bot_guard.get_block_info().await;
                drop(bot_guard);

                let latest_block_number = Some(
                    BlockId::Number(BlockNumber::Number(latest_block.number))
                );

                for tx in snipe_txs {
                    // if the tx is pending skip
                    if tx.retry_pending {
                        continue;
                    }
                    // if we reached the retry limit remove tx from oracles
                    if tx.snipe_retries >= *MAX_SNIPE_RETRIES {
                        // remove tx from retry oracle
                        let mut retry_oracle = retry_oracle.lock().await;
                        retry_oracle.remove_tx_data(tx.clone());
                        drop(retry_oracle);
                        log::warn!(
                            "Retries >={:?}, Removed tx from retry oracle",
                            *MAX_SNIPE_RETRIES
                        );
                        continue;
                    }

                    let bot = bot.clone();
                    let retry_oracle = retry_oracle.clone();

                    let client = client.clone();

                    // usually when a buy is reverted is because of max buy size (can also be due to trading is not open yet)
                    // TODO find a way to understand the real reason why the tx reverted
                    // if for example is due to max buy size we can just lower the amount in and try again
                    // let amount_in = *INITIAL_AMOUNT_IN_WETH / 2;
                    let next_block = next_block.clone();

                    // initialize cache db
                    let cache_db = match
                        insert_pool_storage(client.clone(), tx.pool, latest_block_number).await
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
                                *INITIAL_AMOUNT_IN_WETH,
                                &next_block,
                                None,
                                get_transfer_event(),
                                get_swap_event(),
                                fork_factory.new_sandbox_fork()
                            )
                        {
                            Ok(result) => result,
                            Err(e) => {
                                log::error!("Retry Tax Check Failed: {:?}", e);
                                // update the retry counter
                                let mut retry_oracle_guard = retry_oracle.lock().await;
                                retry_oracle_guard.update_retry_counter(tx.clone());
                                drop(retry_oracle_guard);
                                return;
                            }
                        };

                        // if we fail to swap
                        if !swap {
                            // update the retry counter
                            let mut retry_oracle_guard = retry_oracle.lock().await;
                            retry_oracle_guard.update_retry_counter(tx.clone());
                            drop(retry_oracle_guard);
                            return;
                        }

                        // ** Do transfer checks
                        let _transfer_result = match
                            transfer_check(
                                &tx.pool,
                                *INITIAL_AMOUNT_IN_WETH,
                                &next_block,
                                None,
                                fork_factory.new_sandbox_fork()
                            )
                        {
                            Ok(result) => result,
                            Err(e) => {
                                log::error!("Retry Tax Check Failed: {:?}", e);
                                // update the retry counter
                                let mut retry_oracle_guard = retry_oracle.lock().await;
                                retry_oracle_guard.update_retry_counter(tx.clone());
                                drop(retry_oracle_guard);
                                return;
                            }
                        };

                        // simulate the tx once again and generate accesslist

                        let snipe_tx = match
                            generate_buy_tx_data(
                                &tx.pool,
                                *INITIAL_AMOUNT_IN_WETH,
                                &next_block,
                                None,
                                *MINER_TIP_TO_SNIPE_RETRY,
                                get_swap_event(),
                                get_transfer_event(),
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
                        let miner_tip = *MINER_TIP_TO_SNIPE_RETRY;

                        if tx.reason == 2u8 {
                            let mut retry_oracle_guard = retry_oracle.lock().await;
                            retry_oracle_guard.remove_tx_data(tx.clone());
                            drop(retry_oracle_guard);
                            log::info!("Removed from retry oracle due to bundle not included");
                            return;
                        }

                        // ** max fee per gas must always be higher than miner tip
                        let max_fee_per_gas = next_block.base_fee + miner_tip;

                        // just zero
                        let expected_amount = U256::zero();

                        // ** create the tx data for the bundle
                        let tx_data = TxData {
                            tx_call_data: snipe_tx.tx_call_data.clone(),
                            access_list: snipe_tx.access_list.clone(),
                            gas_used: snipe_tx.gas_used,
                            expected_amount,
                            sniper_contract_address: snipe_tx.sniper_contract_address,
                            pending_tx: snipe_tx.pending_tx.clone(),
                            frontrun_or_backrun: U256::from(2u128), // 2 cause we dont backrun or frontrun
                        };

                        // ** calculate the total gas cost
                        let total_gas_cost = (next_block.base_fee + miner_tip) * tx_data.gas_used;

                        // ** If gas cost is more than amount_in we dont snipe
                        // you can remove this check if you want to snipe anyway
                        if total_gas_cost > snipe_tx.amount_in {
                            log::warn!("Gas Cost Is Higher Than Amount In");
                            return;
                        }

                        // add the snipe_tx to oracles
                        add_tx_to_oracles(bot.clone(), snipe_tx.clone()).await;

                        // set tx to retry pennding true
                        let mut retry_oracle_guard = retry_oracle.lock().await;
                        retry_oracle_guard.set_tx_is_pending(tx.clone(), true);
                        drop(retry_oracle_guard);

                        // get the nonce and update it
                        let mut bot_guard = bot.lock().await;
                        let nonce = bot_guard.get_nonce().await;
                        drop(bot_guard);

                        log::info!("Retry Oracle: Sent Tx To Oracles! 🚀");

                        let is_bundle_included = match
                            send_tx(
                                client.clone(),
                                tx_data.clone(),
                                next_block.clone(),
                                miner_tip,
                                max_fee_per_gas,
                                nonce
                            ).await
                        {
                            Ok(result) => result,
                            Err(e) => {
                                log::warn!("Failed to send tx to flashbots: {:?}", e);
                                false // set is_bundle_included to false if there's an error
                            }
                        };

                        if is_bundle_included {
                            // remove it from the retry oracle
                            let mut retry_oracle_guard = retry_oracle.lock().await;
                            retry_oracle_guard.remove_tx_data(tx.clone());
                            drop(retry_oracle_guard);
                            log::info!("Bundle Included! 🚀");
                            log::info!("Removed Token {:?} From Retry Oracle! 🚀", tx.pool.token_1);
                        } else {
                            // update the retry counter
                            let mut retry_oracle_guard = retry_oracle.lock().await;
                            retry_oracle_guard.update_retry_counter(tx.clone());

                            // update the pending status to false
                            retry_oracle_guard.set_tx_is_pending(tx.clone(), false);

                            // if we fail to snipe the token the 2nd time
                            // its better to remove it and move on
                            // as it is possible to get a really bad position and get wrecked most of the times
                            if tx.reason == 2u8 {
                                retry_oracle_guard.remove_tx_data(tx.clone());
                            }
                            retry_oracle_guard.update_reason(tx.clone(), 2);
                            drop(retry_oracle_guard);

                            // remove the tx from oracles so we dont get bombarded with logs
                            remove_tx_from_oracles(bot.clone(), tx.clone()).await;

                            log::warn!("Bundle Not Included");
                            return;
                        }
                    }); // end of tokio::spawn
                } // end of for loop
            } // end of while loop
        } // end of loop
    }); // end of tokio::spawn
}
