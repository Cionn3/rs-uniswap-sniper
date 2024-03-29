// SPDX-License-Identifier: MIT
pragma solidity >=0.7.0 <0.9.0;

// Interfaces
import "../interfaces/IERC20.sol";

// Libraries
import {Swapper} from "../libraries/Swapper.sol";
import {SafeERC20} from "../interfaces/SafeERC20/SafeERC20.sol";


contract Sniper {
    using SafeERC20 for IERC20;

       // replace with your addresses
    address public constant SWAP_USER = 0x0000Fd55524058D96255053C0098397E59B9500d;
    address public constant ADMIN = 0x008712be3C996bb73008f0eA47C5742D653c10A8;


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

        // returns real amount (considering any balance left in the contract)
       uint256 amount_out = Swapper._swap_on_V2(
            input_token,
            output_token,
            amount_in,
            pool
        );

        // passing 0 as minimum_received means we have no slippage set
        require(amount_out >= minimum_received, "Yeeeeeeeeet");
}


    // withdraws any ERC20 token from the contract
    function withdraw(address token, uint256 amount) external {
        require(msg.sender == ADMIN, "Hello Stranger!");


        IERC20(token).safeTransfer(ADMIN, amount);
    }

    // ** Withdraw ETH Function

    function withdraw_ETH() external {

        require(msg.sender == ADMIN, "Hello Stranger!");

        payable(ADMIN).transfer(address(this).balance);
    }


    // fallback to receive ETH
    receive() external payable {}

    fallback() external payable {}

}