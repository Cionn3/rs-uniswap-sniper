use ethers::prelude::*;

use crate::oracles::{
    oracle_status,
    mempool_stream::start_mempool_stream,
    pair_oracle::start_pair_oracle,
    block_oracle::{ BlockOracle, start_block_oracle },
    sell_oracle::start_sell_oracle,
    anti_rug_oracle::{ start_anti_rug, anti_honeypot },
    nonce_oracle::start_nonce_oracle,
};

use super::bot_sniper::{ snipe_retry, start_sniper };
use crate::utils::types::{ structs::*, events::* };
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::{ signal, task };


    pub async fn start(client: Arc<Provider<Ws>>) {
        log::info!("Starting Bot");

        // ** prepare oracles
        let block_oracle = BlockOracle::new(&client).await.unwrap();
        let mut block_oracle = Arc::new(RwLock::new(block_oracle));

        // Use Arc<Mutex<>> to share Oracles across tasks.
        let sell_oracle = Arc::new(Mutex::new(SellOracle::new()));
        let retry_oracle = Arc::new(Mutex::new(RetryOracle::new()));
        let anti_rug_oracle = Arc::new(Mutex::new(AntiRugOracle::new()));
        let nonce_oracle = Arc::new(Mutex::new(NonceOracle::new()));

        // hold all oracles inside bot struct
        let bot = Arc::new(Mutex::new(Bot::new(
            block_oracle.clone(),
            nonce_oracle.clone(),
            sell_oracle.clone(),
            anti_rug_oracle.clone(),
        )));

        // setup the new pair event channel
        let new_pair_sender = broadcast::channel::<NewPairEvent>(1000); // buffer size 1000
        let new_pair_receiver = new_pair_sender.0.subscribe();

        // new block event channel
        let new_block_sender = broadcast::channel::<NewBlockEvent>(1000); // buffer size 1000
        let new_block_receiver_2 = new_block_sender.0.subscribe();
        let new_block_receiver_3 = new_block_sender.0.subscribe();
        let new_block_receiver_4 = new_block_sender.0.subscribe();
        let new_block_receiver_5 = new_block_sender.0.subscribe();

        // new mempool event channel
        let new_mempool_sender = broadcast::channel::<MemPoolEvent>(1000); // buffer size 1000
        let new_mempool_receiver = new_mempool_sender.0.subscribe();
        let new_mempool_receiver_2 = new_mempool_sender.0.subscribe();
        let new_mempool_receiver_3 = new_mempool_sender.0.subscribe();

        // start the block oracle
        start_block_oracle(&mut block_oracle, new_block_sender.0.clone());

        // start nonce oracle
        start_nonce_oracle(nonce_oracle.clone(), new_block_receiver_5);

        // start oracle status
        oracle_status(bot.clone(), new_block_receiver_4);

        // ** start mempool_stream
        start_mempool_stream(new_mempool_sender.0);

        // ** start the pair oracle
        // ** Sends new pairs to the sniper
        start_pair_oracle(block_oracle.clone(), new_pair_sender.0.clone(), new_mempool_receiver);

        // ** start the sniper
        // ** Recieves new pairs from the pair oracle
        start_sniper(
            new_pair_receiver,
            bot.clone(),
            retry_oracle.clone(),
        );

        // start the retry oracle
        // ** Recieves new snipe tx data from the sniper
        snipe_retry(
            bot.clone(),
            retry_oracle.clone(),
            new_block_receiver_3
        );

        // ** Start The Sell Oracle
        start_sell_oracle(
            bot.clone(),
            new_block_receiver_2
        );

        // ** Start Anti-Rug Oracle
        start_anti_rug(
            bot.clone(),
            new_mempool_receiver_2
        );

        // ** Start Anti-Honeypot Oracle
        anti_honeypot(
            bot.clone(),
            new_mempool_receiver_3
        );
        log::info!("All Oracles Started");

        let sleep = tokio::time::Duration::from_secs_f32(60.0);
        // keep the bot running
        tokio::select! {
            _ = signal::ctrl_c() => {
                println!("CTRL+C received... exiting");
            }
            _ = async {
                loop {
                    tokio::time::sleep(sleep).await;
                    task::yield_now().await;
                }
            } => {}
        }
    }
