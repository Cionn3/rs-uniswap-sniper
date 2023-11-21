use ethers::prelude::*;
use tokio::sync::broadcast::Sender;

use crate::utils::types::events::MemPoolEvent;
use crate::utils::helpers::*;

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
            };

            let mut mempool_stream = if let Ok(stream) = client.subscribe_full_pending_txs().await {
                stream
            } else {
                log::error!("Failed to create new block stream");
                // we reconnect by restarting the loop
                continue;
            };

            while let Some(tx) = mempool_stream.next().await {
                // exclude our own transactions
                if tx.from == get_my_address() || tx.from == get_admin_address() {
                    continue;
                }

                if tx.to == Some(Address::zero()) {
                    //log::info!("skipped Tx with address zero: {:?}", tx.hash);
                    continue;
                }

                // send the new tx through channel
                new_tx_sender
                    .send(MemPoolEvent::NewTx { tx })
                    .expect("Failed to send MemPoolEvent");
            }
        }
    });
}
