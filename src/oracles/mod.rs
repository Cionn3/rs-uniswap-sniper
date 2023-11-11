use crate::utils::types::{structs::*, events::NewBlockEvent};
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast};

pub mod block_oracle;
pub mod pair_oracle;

pub use block_oracle::*;
pub use pair_oracle::*;

pub mod sell_oracle;
pub use sell_oracle::*;

pub mod anti_rug_oracle;
pub use anti_rug_oracle::*;


pub mod mempool_stream;
pub use mempool_stream::*;

pub mod nonce_oracle;
pub use nonce_oracle::*;


// monitor the status of the oracles
pub fn oracle_status(
    sell_oracle: Arc<Mutex<SellOracle>>,
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
    mut new_block_receive: broadcast::Receiver<NewBlockEvent>
) {

    tokio::spawn(async move {

        loop {
            
            while let Ok(event) = new_block_receive.recv().await {
                let _latest_block = match event {
                    NewBlockEvent::NewBlock { latest_block } => latest_block,
                };
                
                let sell_oracle = sell_oracle.lock().await;
                let sell_oracle_tx_len = sell_oracle.get_tx_len();
                drop(sell_oracle);

                let anti_rug_oracle = anti_rug_oracle.lock().await;
                let anti_rug_oracle_tx_len = anti_rug_oracle.get_tx_len();
                drop(anti_rug_oracle);

                log::info!("Sell Oracle: {:?} txs", sell_oracle_tx_len);
                log::info!("Anti Rug Oracle: {:?} txs", anti_rug_oracle_tx_len);

            }
            
        }
    });
    
}
