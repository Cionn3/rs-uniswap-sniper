use ethers::prelude::*;
use tokio::sync::broadcast::Sender;

use crate::utils::helpers::create_local_client;

#[derive(Debug, Clone)]
pub enum MemPoolEvent {
    NewTx {
        tx: Transaction,
    },
}

pub fn start_mempool_stream(new_tx_sender: Sender<MemPoolEvent>) {
    tokio::spawn(async move {
        loop {
            let client = match create_local_client().await {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to create local client: {}", e);
                    // we reconnect by restarting the loop
                    continue;
                }
            }; // subscribe to full pending tx
            let mut mempool_stream = if let Ok(stream) = client.subscribe_full_pending_txs().await {
                stream
            } else {
                log::error!("Failed to create new block stream");
                // we reconnect by restarting the loop
                continue;
            };

            // define transfer methodid
            let transfer_id: [u8; 4] = [0xa9, 0x05, 0x9c, 0xbb];

            while let Some(tx) = mempool_stream.next().await {
                // if method id is transfer, skip
                if tx.input.0.len() >= 4 && tx.input.0[0..4] == transfer_id {
                   // log::info!("skipped Tx with Transfer method: {:?}", tx.hash);
                    continue;
                }

                // send the new tx through channel
                new_tx_sender.send(MemPoolEvent::NewTx { tx }).unwrap();
            }
        }
    });
}
