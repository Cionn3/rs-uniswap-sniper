use ethers::prelude::*;
use ethers::abi::parse_abi;
use ethers::utils::keccak256;
use std::fs;
use lazy_static::lazy_static;


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
pub fn encode_withdraw(input_token: Address, amount_in: U256) -> Vec<u8> {
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



// PairCreated event abi
const PAIR_CREATED_ABI: &str =
    "[{\"anonymous\":false,\"inputs\":[{\"indexed\":true,\"name\":\"token0\",\"type\":\"address\"},{\"indexed\":true,\"name\":\"token1\",\"type\":\"address\"},{\"indexed\":false,\"name\":\"pair\",\"type\":\"address\"},{\"indexed\":false,\"name\":\"\",\"type\":\"uint256\"}],\"name\":\"PairCreated\",\"type\":\"event\"}]";

// Transfer event abi
const TRANSFER_EVENT_ABI: &str =
    "[{\"anonymous\":false,\"inputs\":[{\"indexed\":true,\"name\":\"from\",\"type\":\"address\"},{\"indexed\":true,\"name\":\"to\",\"type\":\"address\"},{\"indexed\":false,\"name\":\"value\",\"type\":\"uint256\"}],\"name\":\"Transfer\",\"type\":\"event\"}]";

// Swap event abi
const V2_SWAP_EVENT_ABI: &str =
    "[{\"anonymous\":false,\"inputs\":[{\"indexed\":true,\"name\":\"sender\",\"type\":\"address\"},{\"indexed\":false,\"name\":\"amount0In\",\"type\":\"uint256\"},{\"indexed\":false,\"name\":\"amount1In\",\"type\":\"uint256\"},{\"indexed\":false,\"name\":\"amount0Out\",\"type\":\"uint256\"},{\"indexed\":false,\"name\":\"amount1Out\",\"type\":\"uint256\"},{\"indexed\":true,\"name\":\"to\",\"type\":\"address\"}],\"name\":\"Swap\",\"type\":\"event\"}]";


// ** HOLDS ALL THE ABIS WE ARE GOING TO USE **
lazy_static! {
    pub static ref SYNC_EVENT: ethabi::Event = get_sync_event();
    pub static ref MINT_EVENT: ethabi::Event = get_mint_event();
    pub static ref PAIR_CREATED_EVENT: ethabi::Event = get_pair_created_event();
    pub static ref V2_SWAP_EVENT: ethabi::Event = get_v2_swap_event();
    pub static ref TRANSFER_EVENT: ethabi::Event = get_transfer_event();

    pub static ref ERC20_BALANCE_OF: BaseContract = get_erc20_balanceof();
    pub static ref TOKEN0: BaseContract = get_token0();
    pub static ref TOKEN1: BaseContract = get_token1();
}


// ** Getter functions for the ABIs **


pub fn get_sync_event() -> ethabi::Event {
    let abi = load_abi_from_file("src/utils/abi/UniswapV2Pair.abi").expect("Failed to load UniswapV2Pair ABI");
    let contract = ethabi::Contract::load(abi.as_bytes()).expect("Failed to load UniswapV2Pair ABI");
    contract.event("Sync").expect("Failed to extract Sync event").clone()
}

pub fn get_mint_event() -> ethabi::Event {
    let abi = load_abi_from_file("src/utils/abi/UniswapV2Pair.abi").expect("Failed to load UniswapV2Pair ABI");
    let contract = ethabi::Contract::load(abi.as_bytes()).expect("Failed to load UniswapV2Pair ABI");
    contract.event("Mint").expect("Failed to extract Mint event").clone()
}

pub fn get_pair_created_event() -> ethabi::Event {
    let load_pair_created_event = ethabi::Contract::load(PAIR_CREATED_ABI.as_bytes()).unwrap();
    let pair_created_event = load_pair_created_event.event("PairCreated").unwrap();
    pair_created_event.clone()
}

pub fn get_v2_swap_event() -> ethabi::Event {
    let load_swap_event = ethabi::Contract::load(V2_SWAP_EVENT_ABI.as_bytes()).unwrap();
    let swap_event = load_swap_event.event("Swap").unwrap();
    swap_event.clone()
}

pub fn get_transfer_event() -> ethabi::Event {
    let load_transfer_event = ethabi::Contract::load(TRANSFER_EVENT_ABI.as_bytes()).unwrap();
    let transfer_event = load_transfer_event.event("Transfer").unwrap();
    transfer_event.clone()
}

fn get_erc20_balanceof() -> BaseContract {
    BaseContract::from(
        parse_abi(&["function balanceOf(address) external returns (uint)"]).unwrap()
    )
}

fn get_token0() -> BaseContract {
    BaseContract::from(
        parse_abi(&["function token0() external view returns (address)"]).unwrap()
    )
}

fn get_token1() -> BaseContract {
    BaseContract::from(
        parse_abi(&["function token1() external view returns (address)"]).unwrap()
    )
}



pub fn load_abi_from_file(file_path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(file_path)?;
    Ok(content)
}    


abigen!(
    UniswapV2Pair,
    "src/utils/abi/IUniswapV2Pair.abi",
    event_derives(serde::Deserialize, serde::Serialize)
);

abigen!(
    UniswapV2Router,
    "src/utils/abi/IUniswapV2Router.abi",
    event_derives(serde::Deserialize, serde::Serialize)
);
