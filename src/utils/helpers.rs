use std::sync::Arc;
use std::str::FromStr;
use ethers::prelude::*;
use ethers::types::transaction::eip2718::TypedTransaction;
use bigdecimal::BigDecimal;
use crate::utils::abi::UniswapV2Pair;
use anyhow::anyhow;

use super::constants::WETH;


/// Create Websocket Client
pub async fn create_local_client() -> Result<Arc<Provider<Ws>>, anyhow::Error> {
    let url: &str = "ws://localhost:8546";
    let client = Provider::<Ws>::connect(url).await?;
    Ok(Arc::new(client))
}


pub fn convert_wei_to_ether(wei: U256) -> BigDecimal {
    let divisor = BigDecimal::from_str("1000000000000000000").unwrap();
    let wei_as_decimal = BigDecimal::from_str(&wei.to_string()).unwrap();
    wei_as_decimal / divisor
}

pub fn convert_wei_to_gwei(wei: U256) -> BigDecimal {
    let divisor = BigDecimal::from_str("1000000000").unwrap();
    let wei_as_decimal = BigDecimal::from_str(&wei.to_string()).unwrap();
    wei_as_decimal / divisor
}



/// Sign eip1559 transactions
pub async fn sign_eip1559(
    tx: Eip1559TransactionRequest,
    signer_wallet: &LocalWallet
) -> Result<Bytes, WalletError> {
    let tx_typed = TypedTransaction::Eip1559(tx);
    //log::info!("Signing transaction: {:?}", tx_typed);
    let signed_frontrun_tx_sig = match signer_wallet.sign_transaction(&tx_typed).await {
        Ok(s) => s,
        Err(e) => {
            return Err(e);
        }
    };

    Ok(tx_typed.rlp_signed(&signed_frontrun_tx_sig))
}

// get the reserves from a V2 pool
pub async fn get_reserves(
    target_pool: Address,
    client: Arc<Provider<Ws>>
) -> Result<U256, anyhow::Error> {
    let pair = UniswapV2Pair::new(target_pool, client.clone());

    let token_a = pair.token_0().call().await?;

    let (reserve_a, reserve_b, _) = match pair.get_reserves().call().await {
        Ok(r) => r,
        Err(e) => {
            return Err(anyhow!("Error getting reserves {:?}", e));
        }
    };

    // match the tokens with the corrospinding reserves
    let reserve_base = if token_a == *WETH {
        reserve_a
    } else {
        reserve_b
    };

    Ok(reserve_base.into())
}