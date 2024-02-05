// SPDX-License-Identifier: UNLICENSED
pragma solidity >=0.7.0 <0.9.0;



import "../lib/forge-std/src/Test.sol";

import {IERC20} from "../interfaces/IERC20.sol";
import {SafeERC20} from "../interfaces/SafeERC20/SafeERC20.sol";
import {Swapper} from "../libraries/Swapper.sol";
import {IWETH} from "../interfaces/IWETH.sol";


contract SniperTest is Test {
    using SafeERC20 for IERC20;

    address public constant SWAP_USER = 0x0000Fd55524058D96255053C0098397E59B9500d;
    address public constant ADMIN = 0x008712be3C996bb73008f0eA47C5742D653c10A8;
    address private constant WETH = 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2;
    address private constant USDT = 0xdAC17F958D2ee523a2206206994597C13D831ec7;
    address private constant WETH_USDT_POOL = 0x0d4a11d5EEaaC28EC3F61d100daF4d40471f1852;


function test_withdraw() external {
    // comment require to actually test the function, gotta find out how to test calls with specific msg.sender
    require(msg.sender == ADMIN, "Hello Stranger!");

    // fund contract with native eth
    uint256 eth_amount = 5000000000000000000;
    vm.deal(address(this), eth_amount);

    // deposit eth to weth
    IWETH(WETH).deposit{value: 5000000000000000000}();

    withdraw_erc20(WETH, eth_amount);
}

function test_withdraw_eth() external {
    require(msg.sender == ADMIN, "Hello Stranger!");

    uint256 eth_amount = 5000000000000000000;
    vm.deal(address(this), eth_amount);

    payable(ADMIN).transfer(address(this).balance);
    uint256 admin_balance = ADMIN.balance;
    console.log("ADMIN ETH Balance", admin_balance);
}


function test_swap() external {
    
    uint256 eth_amount = 5000000000000000000;
    vm.deal(address(this), eth_amount);

    IWETH(WETH).deposit{value: 5000000000000000000}();

    // swap weth for usdt
    address input_token = WETH;
    address output_token = USDT;
    address pool = WETH_USDT_POOL;
    uint256 amount_in = 5000000000000000000;
    uint256 minimum_received = 0;

           uint256 amount_out = Swapper._swap_on_V2(
            input_token,
            output_token,
            amount_in,
            pool
        );

        require(amount_out >= minimum_received, "Yeeeeeeeeet");

        uint256 contract_balance = IERC20(USDT).balanceOf(address(this));
        console.log("Contract USDT Balance", contract_balance);

        // also make sure safeTransfer works for USDT
        withdraw_erc20(USDT, contract_balance);
}







function withdraw_erc20(address token, uint256 amount) internal {

    IERC20(token).safeTransfer(ADMIN, amount);
    uint256 admin_balance = IERC20(token).balanceOf(ADMIN);
    console.log("ADMIN Balance", admin_balance);

}

}