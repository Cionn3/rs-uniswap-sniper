// SPDX-License-Identifier: MIT
pragma solidity >=0.7.0 <0.9.0;

// Interfaces
import "../interfaces/IERC20.sol";

// Libraries
import {V2Swapper} from "../interfaces/Uniswap/V2_Swapper.sol";


contract Sniper {

       // CONSTANTS
       // uncoment and add your address
   // address public constant SWAP_USER = 0xyour address;
  //  address public constant ADMIN = 0xyour address;


    constructor() {        
}


// swap function
// swaps directly on pool
// swaps input for output
function snipaaaaaa(
    address input_token,
    address output_token,
    address pool,
    uint256 amount_in,
    uint256 expected_amount
) external {

    require(msg.sender == SWAP_USER, "Hello Stranger!");

        // swap input for ouput
       uint256 amount_out = V2Swapper._swap_on_V2(
            input_token,
            output_token,
            amount_in,
            pool
        );

        // expected amount out is calculated with a 10% tolerance
        // everything lower than that we revert the transaction
        // passing 0 as expected amount will skip this check

       
    // Check if expected_amount is provided
    if (expected_amount != 0) {
        require(amount_out >= expected_amount, "Yeeeeeeeeet");
    }

}

 // ** Withdraw WETH Function

    // withdraws any ERC20 token from the contract
    function withdraw(address token, uint256 amount) external {
        require(msg.sender == ADMIN, "Hello Stranger!");

        // withdraw token
        IERC20(token).transfer(ADMIN, amount);
    }

    // ** Withdraw ETH Function

    // withdraws ETH from the contract
    function withdraw_ETH() external {

        require(msg.sender == ADMIN, "Hello Stranger!");

        payable(ADMIN).transfer(address(this).balance);
    }


    // fallback to receive WETH
    receive() external payable {}

}