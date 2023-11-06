use std::sync::Arc;
use std::str::FromStr;
use ethers::{ prelude::*, types::transaction::eip2718::TypedTransaction };
use ethers::types::transaction::eip2930::{ AccessList, AccessListItem };
use revm::primitives::{ U256 as rU256, B160 as rAddress };
use bigdecimal::BigDecimal;
use std::fs;
use sha3::{ Digest, Keccak256 };
use lazy_static::lazy_static;

lazy_static! {
    pub static ref WETH: Address = get_weth_address();

    // change these numbers as you like

    // amount of eth to use for the snipe
    // 0.01 eth
    pub static ref INITIAL_AMOUNT_IN_WETH: U256 = U256::from(10000000000000000u128);

    // amount of eth to get from the trade
    // 0.05 eth = 5x
    pub static ref TARGET_AMOUNT_TO_SELL: U256 = U256::from(50000000000000000u128);

    // miner tip to use for the snipe
    // 10 gwei
    pub static ref MINER_TIP_TO_SNIPE: U256 = U256::from(10000000000u128); 

    // miner tip to use when we retry to snipe
    // 3 gwei
    pub static ref MINER_TIP_TO_SNIPE_RETRY: U256 = U256::from(3000000000u128); 

    // miner tip to use when we sell
    // 3 gwei
    pub static ref MINER_TIP_TO_SELL: U256 = U256::from(3000000000u128);

    // how many times we try to sell before we remove the token from the sell oracle
    pub static ref MAX_SELL_ATTEMPTS: u8 = 5;

    // how many times we retry to buy a token before we remove it from the retry oracle
    pub static ref MAX_SNIPE_RETRIES: u8 = 10; 

    // minimum weth reserve for a new pair
    // default is 1 eth
    pub static ref MIN_WETH_RESERVE: U256 = U256::from(1000000000000000000u128);

    // maximum weth reserve for a new pair
    // default is 7 eth
    pub static ref MAX_WETH_RESERVE: U256 = U256::from(7000000000000000000u128);
}

// address to call contract from (SWAP_USER)
pub fn get_my_address() -> Address {
    Address::from_str("0xyouraddress").unwrap()
}

// address to withdraw funds to
pub fn get_admin_address() -> Address {
    Address::from_str("0xyouraddress").unwrap()
}

// your contract address goes here
pub fn get_snipe_contract_address() -> Address {
    Address::from_str("0xyourcontracttaddress").unwrap()
}

pub fn get_weth_address() -> Address {
    Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap()
}

// wallet used to sign the trasnactions and call the contract
// fill in your private key here
pub fn get_my_wallet() -> LocalWallet {
    let private_key: String = "0xprivatekey".parse().expect("private key wrong format?");
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
    "0xprivatekey".to_string()
}

// flashbot searcher signer, must be the same private key as the wallet used to sign the tx
pub fn get_flashbots_searcher_key() -> String {
    "0xprivatekey".to_string()
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

// Converts access list from revm to ethers type
//
// Arguments:
// * `access_list`: access list in revm format
//
// Returns:
// `AccessList` in ethers format
pub fn convert_access_list(access_list: Vec<(rAddress, Vec<rU256>)>) -> AccessList {
    let mut converted_access_list = Vec::new();
    for access in access_list {
        let address = access.0;
        let keys = access.1;
        let access_item = AccessListItem {
            address: address.0.into(),
            storage_keys: keys
                .iter()
                .map(|k| {
                    let slot_u256: U256 = k.clone().into();
                    let slot_h256: H256 = H256::from_uint(&slot_u256);
                    slot_h256
                })
                .collect::<Vec<H256>>(),
        };
        converted_access_list.push(access_item);
    }

    AccessList(converted_access_list)
}

pub fn calculate_miner_tip(pending_tx_priority_fee: U256) -> U256 {
    let point_one_gwei = U256::from(100000000u128); // 0.1 gwei
    let point_five_gwei = U256::from(500000000u128); // 0.5 gwei
    let one_gwei = U256::from(1000000000u128); // 1 gwei
    let two_gwei = U256::from(2000000000u128); // 2 gwei
    let three_gwei = U256::from(3000000000u128); // 3
    let ten_gwei = U256::from(10000000000u128); // 10 gwei

    let miner_tip;

    // match pending_tx_priorite_fee to the different lvls we set
    match pending_tx_priority_fee {
        // if pending fee is 0
        fee if fee == (0).into() => {
            miner_tip = ten_gwei; // 10 gwei
        }
        // if pending fee is between  0 ish and 0.1 gwei
        fee if fee < point_one_gwei => {
            miner_tip = fee * 100; // maximum 10 gwei
        }
        // if pending fee is between 0.1 and 1 gwei
        fee if fee > point_one_gwei && fee < point_five_gwei => {
            miner_tip = fee * 25; // maximum 10 gwei
        }
        // if fee is between 0.5 and 1 gwei
        fee if fee > point_five_gwei && fee < one_gwei => {
            miner_tip = fee * 15; // maximum 15 gwei
        }
        // if pending fee is between 1 and 3 gwei
        fee if fee > one_gwei && fee < two_gwei => {
            miner_tip = fee * 7; // maximum 21 gwei
        }
        // if fee is between 2 and 3 gwei
        fee if fee > two_gwei && fee < three_gwei => {
            miner_tip = fee * 5; // maximum 15 gwei
        }
        // for anything else
        _ => {
            miner_tip = (pending_tx_priority_fee * 15) / 10; // +50%
        }
    }

    miner_tip
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
