use std::sync::Arc;
use std::str::FromStr;
use ethers::prelude::*;
use ethers::types::transaction::eip2718::TypedTransaction;
use bigdecimal::BigDecimal;
use crate::utils::abi::UniswapV2Pair;
use anyhow::anyhow;
use std::fs;
use sha3::{ Digest, Keccak256 };

// transfer event abi
const TRANSFER_EVENT_ABI: &str =
    "[{\"anonymous\":false,\"inputs\":[{\"indexed\":true,\"name\":\"from\",\"type\":\"address\"},{\"indexed\":true,\"name\":\"to\",\"type\":\"address\"},{\"indexed\":false,\"name\":\"value\",\"type\":\"uint256\"}],\"name\":\"Transfer\",\"type\":\"event\"}]";

// swap event abi
const SWAP_EVENT_ABI: &str =
    "[{\"anonymous\":false,\"inputs\":[{\"indexed\":true,\"name\":\"sender\",\"type\":\"address\"},{\"indexed\":false,\"name\":\"amount0In\",\"type\":\"uint256\"},{\"indexed\":false,\"name\":\"amount1In\",\"type\":\"uint256\"},{\"indexed\":false,\"name\":\"amount0Out\",\"type\":\"uint256\"},{\"indexed\":false,\"name\":\"amount1Out\",\"type\":\"uint256\"},{\"indexed\":true,\"name\":\"to\",\"type\":\"address\"}],\"name\":\"Swap\",\"type\":\"event\"}]";




// the address which you sign the transactions and call the contract
pub fn get_my_address() -> Address {
    Address::from_str("0xYOUR_ADDRESS").unwrap()
}

// the address of the snipe contract
pub fn get_snipe_contract_address() -> Address {
    Address::from_str("0xCONTRACT_ADDRESS").unwrap()
}

// address to withdraw funds to
pub fn get_admin_address() -> Address {
    Address::from_str("0xYOUR_ADDRESS").unwrap()
}

pub fn get_weth_address() -> Address {
    Address::from_str("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2").unwrap()
}


// wallet used to sign the trasnactions and call the contract
// fill in your private key here
pub fn get_my_wallet() -> LocalWallet {
    let private_key: String = "0xYOUR_PRIVATE_KEY"
        .parse()
        .expect("private key wrong format?");
    private_key.parse::<LocalWallet>().expect("Failed to parse private key")
}

pub fn get_flashbot_identity() -> LocalWallet {
    let private_key: String = get_flashbots_auth_key();
    private_key.parse::<LocalWallet>().expect("Failed to parse flashbot signer private key")
}

pub fn get_flashbot_searcher() -> LocalWallet {
    let private_key: String = get_flashbots_searcher_key();
    private_key.parse::<LocalWallet>().expect("Failed to parse flashbot identity private key")
}

// flashbot identity , could also be a random private key
pub fn get_flashbots_auth_key() -> String {
    "0xYOUR_PRIVATE_KEY".to_string()
}

// flashbot searcher signer, must be the same private key as the wallet used to sign the tx
pub fn get_flashbots_searcher_key() -> String {
    "0xYOUR_PRIVATE_KEY".to_string()
}






/// Create Websocket Client
pub async fn create_local_client() -> Result<Arc<Provider<Ws>>, anyhow::Error> {
    let client = get_local_client().await?;
    Ok(Arc::new(client))
}

pub async fn get_local_client() -> Result<Provider<Ws>, anyhow::Error> {
    let url: &str = "ws://localhost:8546";
    let provider = Provider::<Ws>::connect(url).await?;
    Ok(provider)
}



pub async fn get_nonce(
    client: Arc<Provider<Ws>>,
    address: Address
) -> Result<Option<u64>, ProviderError> {
    client.get_transaction_count(address, None).await.map(|nonce| Some(nonce.as_u64()))
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

// Load ABI from a file
pub fn load_abi_from_file(file_path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(file_path)?;
    Ok(content)
}

pub fn get_swap_event() -> ethabi::Event {
    let load_swap_event = ethabi::Contract::load(SWAP_EVENT_ABI.as_bytes()).unwrap();
    let swap_event = load_swap_event.event("Swap").unwrap();
    swap_event.clone()
}

pub fn get_transfer_event() -> ethabi::Event {
    let load_transfer_event = ethabi::Contract::load(TRANSFER_EVENT_ABI.as_bytes()).unwrap();
    let transfer_event = load_transfer_event.event("Transfer").unwrap();
    transfer_event.clone()
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
    let reserve_base = if token_a == get_weth_address() {
        reserve_a
    } else {
        reserve_b
    };

    Ok(reserve_base.into())
}



pub fn encode_swap(
    input_token: Address,
    output_token: Address,
    pool_address: Address,
    amount_in: U256,
    expected_amount: U256
) -> Vec<u8> {
    // The method's signature hash (first 4 bytes of the keccak256 hash of the signature).
    let method_id = &keccak256(b"snipaaaaaa(address,address,address,uint256,uint256)")[0..4];

    // ABI-encode the arguments
    let encoded_args = ethabi::encode(
        &[
            ethabi::Token::Address(input_token),
            ethabi::Token::Address(output_token),
            ethabi::Token::Address(pool_address),
            ethabi::Token::Uint(amount_in),
            ethabi::Token::Uint(expected_amount),
        ]
    );

    let mut payload = vec![];
    payload.extend_from_slice(method_id);
    payload.extend_from_slice(&encoded_args);

    payload
}

#[allow(dead_code)]
pub fn create_withdraw_data(input_token: Address, amount_in: U256) -> Vec<u8> {
    // The method's signature hash (first 4 bytes of the keccak256 hash of the signature).
    let method_id = &keccak256(b"withdraw(address,uint256)")[0..4];

    // ABI-encode the arguments
    let encoded_args = ethabi::encode(
        &[ethabi::Token::Address(input_token), ethabi::Token::Uint(amount_in)]
    );

    let mut payload = vec![];
    payload.extend_from_slice(method_id);
    payload.extend_from_slice(&encoded_args);

    payload
}

pub fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&result);
    output
}