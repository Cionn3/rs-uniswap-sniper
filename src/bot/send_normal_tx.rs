use std::sync::Arc;
use ethers::prelude::*;
use ethers::types::transaction::eip2718::TypedTransaction;
use crate::oracles::block_oracle::BlockInfo;

use crate::utils::types::structs::tx_data::TxData;
use crate::utils::helpers::{ get_my_address, get_my_wallet, get_snipe_contract_address, sign_eip1559 };




pub async fn send_normal_tx(
    client: Arc<Provider<Ws>>,
    tx_data: TxData,
    next_block: BlockInfo,
    miner_tip: U256,
    nonce: U256,
) -> Result<bool, anyhow::Error> {
    let my_wallet = get_my_wallet();



    // 500k gas limit, way more than enough for a swap
    let gas_limit = U256::from(500000u128);

    let tx_request = Eip1559TransactionRequest {
        to: Some(NameOrAddress::Address(get_snipe_contract_address())),
        from: Some(get_my_address()),
        data: Some(tx_data.tx_call_data.clone()),
        chain_id: Some(U64::from(1)),
        max_priority_fee_per_gas: Some(miner_tip),
        max_fee_per_gas: Some(next_block.base_fee + miner_tip),
        gas: Some(gas_limit),
        nonce: Some(nonce),
        value: Some(U256::zero()),
        access_list: tx_data.access_list.clone(),
    };

    let signed_tx = sign_eip1559(tx_request, &my_wallet).await?;

    let tx_hash = match client.send_raw_transaction(signed_tx).await {
        Ok(tx) => tx,
        Err(e) => {
            log::error!("Error sending tx: {:?}", e);
            return Ok(false);
        }
    };

    let tx_hash = tx_hash.clone();

    // a small delay to not spam the node
    let delay = tokio::time::Duration::from_millis(10);
    let mut tx_receipt = None;

    // Wait until we get the receipt
    loop {
        match client.get_transaction_receipt(tx_hash).await {
            Ok(Some(receipt)) => {
                tx_receipt = Some(receipt);
                // we got receipt, break out of loop
                break;
            }
            Ok(None) => {
                tokio::time::sleep(delay).await;
            }
            Err(e) => {
                log::error!("Error getting tx receipt: {:?}", e);
                return Ok(false);
            }
        }
    }

    let tx_receipt = match tx_receipt {
        Some(receipt) => receipt,
        None => {
            log::error!("Error unwrapping tx receipt");
            return Ok(false);
        }
    };

    if tx_receipt.status != Some(U64::from(1u64)) {
        log::error!("Tx {:?} reverted", tx_hash);
        return Ok(false);
    }

    Ok(true)
}