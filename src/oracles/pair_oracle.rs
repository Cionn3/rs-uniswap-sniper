use tokio::sync::broadcast::Sender;
use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::broadcast;
use crate::utils::helpers::{
    create_local_client,
    convert_wei_to_ether,
    load_abi_from_file,
    MIN_WETH_RESERVE,
    MAX_WETH_RESERVE,
};
use crate::utils::types::events::MemPoolEvent;
use crate::utils::simulate::simulate::get_pair;
use crate::bot::bot_config::BotConfig;
use crate::forked_db::fork_factory::ForkFactory;
use crate::utils::types::{ structs::Pool, events::NewPairEvent };
use revm::db::{ CacheDB, EmptyDB };

const PAIR_CREATED_ABI: &str =
    "[{\"anonymous\":false,\"inputs\":[{\"indexed\":true,\"name\":\"token0\",\"type\":\"address\"},{\"indexed\":true,\"name\":\"token1\",\"type\":\"address\"},{\"indexed\":false,\"name\":\"pair\",\"type\":\"address\"},{\"indexed\":false,\"name\":\"\",\"type\":\"uint256\"}],\"name\":\"PairCreated\",\"type\":\"event\"}]";

// Monitor pending txs for new pairs created
pub fn start_pair_oracle(
    bot_config: BotConfig,
    new_pair_sender: Sender<NewPairEvent>,
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

            // define transfer method
            let transfer_id: [u8; 4] = [0xa9, 0x05, 0x9c, 0xbb];
            let approve: [u8; 4] = [0x09, 0x5e, 0xa7, 0xb3];

            // ** Load Burn and Sync events ABI
            let abi = load_abi_from_file("../../src/utils/abi/IUniswapV2Pair.abi").expect(
                "Failed to load ABI"
            );

            // Load the ABI into an ethabi::Contract
            let contract = ethabi::Contract::load(abi.as_bytes()).expect("Failed to load contract");
            let load_pair_created_event = ethabi::Contract
                ::load(PAIR_CREATED_ABI.as_bytes())
                .unwrap();
            let pair_created_event = load_pair_created_event.event("PairCreated").unwrap();
            let sync_event_abi = contract.event("Sync").expect("Failed to extract Sync event");
            let mint_event_abi = contract.event("Mint").expect("Failed to extract Mint event");

            // ** get the next block from oracle
            let block_oracle = bot_config.block_oracle.clone();

            // using semaphore to limit the number of concurrent requests
            // in case we receive a lot of new txs (reorgs)
            let semaphore = Arc::new(tokio::sync::Semaphore::new(5));

            while let Ok(event) = new_mempool_receiver.recv().await {
                let tx = match event {
                    MemPoolEvent::NewTx { tx } => tx,
                };

                if tx.input.0.len() >= 4 && tx.input.0[0..4] == transfer_id {
                    // log::info!("skipped Tx with Transfer method: {:?}", tx.hash);
                    continue;
                }

                if tx.input.0.len() >= 4 && tx.input.0[0..4] == approve {
                    // log::info!("skipped Tx with Transfer method: {:?}", tx.hash);
                    continue;
                }

                let semaphore = semaphore.clone();
                let client = client.clone();

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

                let new_pair_sender = new_pair_sender.clone();
                let sync_event_abi = sync_event_abi.clone();
                let mint_event_abi = mint_event_abi.clone();
                let pair_created_event = pair_created_event.clone();

                tokio::spawn(async move {
                    // acquire the semaphore
                    let permit = semaphore.acquire().await.unwrap();

                    // initialize an empty cache db
                    let cache_db = CacheDB::new(EmptyDB::default());

                    // setup the backend
                    let fork_factory = ForkFactory::new_sandbox_factory(
                        client.clone(),
                        cache_db,
                        latest_block_number
                    );

                    let fork_db = fork_factory.new_sandbox_fork();

                    // now we need to simulate the tx with revm to get the pair address from the logs
                    let (pool_address, weth, token_1, weth_reserve) = match
                        get_pair(
                            next_block,
                            &tx,
                            sync_event_abi,
                            mint_event_abi,
                            pair_created_event,
                            fork_db
                        )
                    {
                        Ok(address) => address,
                        Err(_e) => {
                            // log::error!(" {:?}", e);
                            return;
                        }
                    };

                    if pool_address == Address::zero() {
                        return;
                    }

                    // adjust these numbers as you like
                    if weth_reserve < *MIN_WETH_RESERVE {
                        log::error!(
                            "Weth Reserve < {:?} ETH: {:?}", *MIN_WETH_RESERVE,
                            convert_wei_to_ether(weth_reserve)
                        );
                        return;
                    }

                    if weth_reserve > *MAX_WETH_RESERVE {
                        log::error!(
                            "Weth Reserve > {:?} ETH: {:?}", *MAX_WETH_RESERVE,
                            convert_wei_to_ether(weth_reserve)
                        );
                        return;
                    }

                    // create a new pool
                    // token_a is always weth
                    let pool = Pool::new(
                        pool_address,
                        weth, // token_0 is always weth
                        token_1, // token_1 is the shitcoin
                        weth_reserve
                    );

                    log::info!("New Pool Found!🚀");
                    log::info!("Pool Address: {:?}", pool.address);
                    log::info!("Token Address: {:?}", pool.token_1);

                    // send the new pool event
                    new_pair_sender
                        .send(NewPairEvent::NewPairWithTx { pool: pool, tx: tx.clone() })
                        .unwrap();
                    drop(permit);
                }); // end of tokio spawn
            }
        } // end of main loop
    });
}
