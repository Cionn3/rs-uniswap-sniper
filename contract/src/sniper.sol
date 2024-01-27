// SPDX-License-Identifier: MIT
pragma solidity >=0.7.0 <0.9.0;

// Interfaces
import "../interfaces/IERC20.sol";

// Libraries
import {V2Swapper} from "../interfaces/Uniswap/V2_Swapper.sol";
import {SafeERC20} from "../interfaces/SafeERC20/SafeERC20.sol";


contract Sniper {
    using SafeERC20 for IERC20;

       // CONSTANTS
       // uncoment and add your address
   // address public constant SWAP_USER = 0xyour address;
  //  address public constant ADMIN = 0xyour address;


    constructor() {        
}


// swaps directly on pool
// swaps input for output
function snipaaaaaa(
    address input_token,
    address output_token,
    address pool,
    uint256 amount_in,
    uint256 minimum_received
) external {

    require(msg.sender == SWAP_USER, "Hello Stranger!");

        // swap input for ouput
        // returns real amount (considering any balance left in the contract)
       uint256 amount_out = V2Swapper._swap_on_V2(
            input_token,
            output_token,
            amount_in,
            pool
        );

        // passing 0 as minimum_received means we have no slippage set
        require(amount_out >= minimum_received, "Yeeeeeeeeet");
}

 // ** Withdraw WETH Function

    // withdraws any ERC20 token from the contract
    function withdraw(address token, uint256 amount) external {
        require(msg.sender == ADMIN, "Hello Stranger!");

        // withdraw token
        IERC20(token).safeTransfer(ADMIN, amount);
    }

    // ** Withdraw ETH Function

    // withdraws ETH from the contract
    function withdraw_ETH() external {

        require(msg.sender == ADMIN, "Hello Stranger!");

        payable(ADMIN).transfer(address(this).balance);
    }


    // fallback to receive ETH
    receive() external payable {}

}