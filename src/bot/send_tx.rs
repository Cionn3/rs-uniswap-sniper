use anyhow::anyhow;
use ethers_flashbots::*;
use url::Url;
use std::sync::Arc;
use tokio::task::JoinError;
use ethers::prelude::*;
use crate::oracles::block_oracle::BlockInfo;
use crate::utils::constants::{
    CALLER_ADDRESS,
    CALLER_WALLET,
    FLASHBOT_IDENTITY,
    FLASHBOT_SEARCHER,
    CONTRACT_ADDRESS,
};
use crate::utils::types::structs::tx_data::TxData;
use crate::utils::helpers::sign_eip1559;

#[allow(unused_assignments)]
pub async fn send_tx(
    client: Arc<Provider<Ws>>,
    tx_data: TxData,
    next_block: BlockInfo,
    miner_tip: U256,
    nonce: U256
) -> Result<bool, anyhow::Error> {
    // 500k gas limit, way more than enough for a swap
    let gas_limit = U256::from(500000u128);

    let tx_request = Eip1559TransactionRequest {
        to: Some(NameOrAddress::Address(*CONTRACT_ADDRESS)),
        from: Some(*CALLER_ADDRESS),
        data: Some(tx_data.tx_call_data.clone()),
        chain_id: Some(U64::from(1)),
        max_priority_fee_per_gas: Some(miner_tip),
        max_fee_per_gas: Some(next_block.base_fee + miner_tip),
        gas: Some(gas_limit),
        nonce: Some(nonce),
        value: Some(U256::zero()),
        access_list: tx_data.access_list.clone(),
    };
    let frontrun_or_backrun = tx_data.frontrun_or_backrun;

    let signed_tx = sign_eip1559(tx_request, &CALLER_WALLET).await?;

    let pending_tx = tx_data.pending_tx.rlp();

    let bundle = construct_bundle(
        frontrun_or_backrun,
        signed_tx,
        pending_tx,
        next_block.number,
        next_block.timestamp.as_u64()
    );

    let urls = get_all_urls();
    let mut is_bundle_included = false;

    // ** Send the bundle concurently to all the MEV builders
    // ** Almost all builders support the same API eth_sendBundle
    let mut tasks = Vec::new();

    for url in urls {
        let client = client.clone();
        let bundle = bundle.clone();

        let task = tokio::spawn(async move {
            // Add signer to Flashbots middleware
            let flashbots_client = SignerMiddleware::new(
                FlashbotsMiddleware::new(client.clone(), url.clone(), FLASHBOT_IDENTITY.clone()),
                FLASHBOT_SEARCHER.clone()
            );

            // only simulate bundle for flashbot relay
            if url == Url::parse("https://relay.flashbots.net/").unwrap() {
                let simulated_bundle = flashbots_client.inner().simulate_bundle(&bundle).await;

                match simulated_bundle {
                    Ok(_sim_result) => {
                        // log::info!("Simulated Bundle Result: {:?}", sim_result);
                    }
                    Err(e) => {
                        log::error!("Failed to simulate bundle: {}", e);
                    }
                }
            }

            // send tx to MEV builders
            let pending_bundle = match flashbots_client.inner().send_bundle(&bundle).await {
                Ok(pending_bundle) => pending_bundle,
                Err(e) => {
                    // log::info!("Failed to send bundle: {:?}", e);
                    return Err(anyhow!("Failed to send bundle:: {:?}", e));
                }
            };

            // ** Check if the bundle was included **
            is_bundle_included = match pending_bundle.await {
                Ok(_) => true,
                Err(ethers_flashbots::PendingBundleError::BundleNotIncluded) => false,
                Err(e) => {
                    log::error!("Bundle Error: {:?}", e);
                    false
                }
            };
            // check if bundle is included and return the result
            if is_bundle_included {
                Ok::<bool, anyhow::Error>(true)
            } else {
                Err(anyhow!("Bundle was not included"))
            }
        }); // end of tokio spawn

        tasks.push(task);
    } // end of for loop

    // Await all tasks and check their results
    let results: Vec<Result<Result<bool, anyhow::Error>, JoinError>> = futures::future::join_all(
        tasks
    ).await;

    for task_result in results {
        if let Ok(inner_result) = task_result {
            if let Ok(included) = inner_result {
                if included {
                    is_bundle_included = true;
                    break; // Exit the loop once a bundle is confirmed as included
                }
            }
        }
    }

    log::info!("Is Bundle Included: {:?}", is_bundle_included);

    Ok(is_bundle_included)
}

fn construct_bundle(
    frontrun_or_backrun: U256,
    signed_tx: Bytes,
    signed_pending_tx: Bytes,
    target_block: U64,
    target_timestamp: u64
) -> BundleRequest {
    let mut bundle_request = BundleRequest::new();

    //** frontrun_or backrun Legend
    //** 0 = frontrun
    //** 1 = backrun
    //** 2 = normal sell

    // ** When we snipe we do backrun **
    // ** When we normally sell we dont do frontrun or backrun **
    // ** When we panic sell we do frontrun **

    // ** If we do frontrun we push our tx first
    if frontrun_or_backrun == U256::zero() {
        // ** First we push our tx
        bundle_request = bundle_request.push_transaction(signed_tx);

        // ** Then we push the pending_tx
        bundle_request = bundle_request.push_transaction(signed_pending_tx);

        bundle_request = bundle_request
            .set_block(target_block)
            .set_simulation_block(target_block - 1)
            .set_simulation_timestamp(target_timestamp)
            .set_min_timestamp(target_timestamp)
            .set_max_timestamp(target_timestamp);

        bundle_request
    } else if frontrun_or_backrun == U256::from(1u128) {
        // ** If we do backrun we push the pending_tx first **

        // ** push the pending_tx
        bundle_request = bundle_request.push_transaction(signed_pending_tx);

        // ** Then we push our tx
        bundle_request = bundle_request.push_transaction(signed_tx);

        bundle_request = bundle_request
            .set_block(target_block)
            .set_simulation_block(target_block - 1)
            .set_simulation_timestamp(target_timestamp)
            .set_min_timestamp(target_timestamp)
            .set_max_timestamp(target_timestamp);

        bundle_request
    } else {
        // ** Else if is 2 we do normal sell, we just push our tx
        bundle_request = bundle_request.push_transaction(signed_tx);

        bundle_request = bundle_request
            .set_block(target_block)
            .set_simulation_block(target_block - 1)
            .set_simulation_timestamp(target_timestamp)
            .set_min_timestamp(target_timestamp)
            .set_max_timestamp(target_timestamp);

        bundle_request
    }
}

fn get_all_urls() -> Vec<Url> {
    let endpoints = vec![
        "https://relay.flashbots.net/",
        "http://builder0x69.io/",
        "http://rpc.titanbuilder.xyz",
        "https://api.edennetwork.io/v1/bundle",
        "https://rpc.beaverbuild.org/",
        "https://rpc.lightspeedbuilder.info/",
        "https://eth-builder.com/",
        "https://relay.ultrasound.money/",
        "https://agnostic-relay.net/",
        "https://relayooor.wtf/",
        "https://rsync-builder.xyz/",
        "https://buildai.net/",
        "http://mainnet.aestus.live/",
        "https://mainnet-relay.securerpc.com",
        "https://builder.gmbit.co/rpc",
        "https://mev.api.blxrbdn.com/",
        "https://boba-builder.com/searcher/",
        "https://blockbeelder.com/rpc",
        "https://rpc.lokibuilder.xyz"
    ];

    let mut urls: Vec<Url> = vec![];

    for endpoint in endpoints {
        let url = Url::parse(endpoint).unwrap();
        urls.push(url);
    }

    urls
}
