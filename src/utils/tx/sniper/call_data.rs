use sha3::{Digest, Keccak256};

use ethers::types::{Address, U256};




pub fn encode_swap(
    input_token: Address,
    output_token: Address,
    pool_address: Address,
    amount_in: U256,
    expected_amount: U256,
) -> Vec<u8> {

        // The method's signature hash (first 4 bytes of the keccak256 hash of the signature).
    let method_id = &keccak256(b"snipaaaaaa(address,address,address,uint256,uint256)")[0..4];

  
    // ABI-encode the arguments
    let encoded_args = ethabi::encode(&[
        ethabi::Token::Address(input_token),
        ethabi::Token::Address(output_token),
        ethabi::Token::Address(pool_address),
        ethabi::Token::Uint(amount_in),
        ethabi::Token::Uint(expected_amount),
    ]);

    let mut payload = vec![];
    payload.extend_from_slice(method_id);
    payload.extend_from_slice(&encoded_args);


    payload

}


pub fn create_withdraw_data(
    input_token: Address,
    amount_in: U256,
) -> Vec<u8> {

        // The method's signature hash (first 4 bytes of the keccak256 hash of the signature).
    let method_id = &keccak256(b"withdraw(address,uint256)")[0..4];

  
    // ABI-encode the arguments
    let encoded_args = ethabi::encode(&[
        ethabi::Token::Address(input_token),
        ethabi::Token::Uint(amount_in),
    ]);

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