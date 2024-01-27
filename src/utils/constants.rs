use lazy_static::lazy_static;
use ethers::prelude::*;
use std::str::FromStr;


// ** Addresses **
lazy_static!{
    pub static ref WETH: Address = Address::from_str("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2").unwrap();

    // the address which you sign the transactions and call the contract
    pub static ref CALLER_ADDRESS: Address = Address::from_str("0xYOUR_ADDRESS").unwrap();

    // The address which you withdraw the profits to
    pub static ref ADMIN_ADDRESS: Address = Address::from_str("0xYOUR_ADDRESS").unwrap();

    pub static ref CALLER_WALLET: LocalWallet = get_my_wallet();

    pub static ref FLASHBOT_IDENTITY: LocalWallet = get_flashbot_identity();

    pub static ref FLASHBOT_SEARCHER: LocalWallet = get_flashbot_searcher();

    // the address of the snipe contract
    pub static ref CONTRACT_ADDRESS: Address = Address::from_str("0xCONTRACT_ADDRESS").unwrap();

    // Locally inserted contract
    pub static ref SWAPPER_ADDRESS: Address = Address::from_str("00000000000000000000000000000000F3370000").unwrap();
}


// ** BOT SETTINGS **
lazy_static! {


    // change these settings as you like

    // ** BUY/SELL SLIPPAGE SETTINGS **
    // chnage numerator to adjust slippage
    // 9 is for 10% slippage
    // for example if you want 20% slippage change it to 8 and so on
    pub static ref BUY_NUMERATOR: u128 = 9;
    pub static ref BUY_DENOMINATOR: u128 = 10;

    // minimum buy size in weth
    // default 0.025 eth
    pub static ref MIN_BUY_SIZE: U256 = U256::from(25000000000000000u128);

    // maximum buy size in weth
    // default 0.05 eth
    pub static ref MAX_BUY_SIZE: U256 = U256::from(50000000000000000u128);

    // target amount to sell in eth (All Tokens)
    // default 0.5 eth
    pub static ref TARGET_AMOUNT_TO_SELL: U256 = U256::from(500000000000000000u128);

    // how many xs the token must do in order to get
    // the initial amount back
    // default is 5 which the price must pump 5x
    // if you dont want to take your initial out just put a very high number here
    // **NOTE: we calculate the target amount to take profit as follows:
    // **(gas_cost + initial_amount_in) * *INITIAL_PROFIT_TAKE;
    // **Because gas is ridiculous high we also calculating the gas fees
    // ** If we bought 50$ in eth and it costed us 100$ in gas to swap
    // ** The token must do at least 4x to get our initial amount back and leave a bag
    pub static ref INITIAL_PROFIT_TAKE: U256 = U256::from(5u128);

    // miner tip to snipe
    // default is 100 gwei
    pub static ref MINER_TIP_TO_SNIPE: U256 = U256::from(100000000000u128);

    // miner tip to use when selling
    // default 10 gwei
    pub static ref MINER_TIP_TO_SELL: U256 = U256::from(10000000000u128);

    // how many times we try to sell before we remove the token from the sell oracle
    pub static ref MAX_SELL_ATTEMPTS: u8 = 20;

    // how many times we retry to buy a token before we remove it from the retry oracle
    pub static ref MAX_SNIPE_RETRIES: u8 = 10;

    // minimum weth reserve for a new pair
    // default is 1 weth
    pub static ref MIN_WETH_RESERVE: U256 = U256::from(1000000000000000000u128);

    // maximum weth reserve for a new pair
    // default is 4 weth
    pub static ref MAX_WETH_RESERVE: U256 = U256::from(4000000000000000000u128);

    
}



// wallet used to sign the trasnactions and call the contract
// fill in your private key here
pub fn get_my_wallet() -> LocalWallet {
    let private_key: String = "0xYOUR_PRIVATE_KEY"
        .parse()
        .expect("private key wrong format?");
    private_key.parse::<LocalWallet>().expect("Failed to parse private key")
}

// flashbot identity , could also be a random private key
pub fn get_flashbot_identity() -> LocalWallet {
    let private_key: String = "0xYOUR_PRIVATE_KEY".to_string();
    private_key.parse::<LocalWallet>().expect("Failed to parse flashbot signer private key")
}

// flashbot searcher signer, must be the same private key as the wallet used to sign the tx
pub fn get_flashbot_searcher() -> LocalWallet {
    let private_key: String = "0xYOUR_PRIVATE_KEY".to_string();
    private_key.parse::<LocalWallet>().expect("Failed to parse flashbot identity private key")
}