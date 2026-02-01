// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

/**
 * @title NornToken
 * @dev Implementation of a standard ERC20 token for the norn blockchain
 * @custom:security-contact security@norn.io
 */
contract NornToken is ERC20, Ownable {
    // Cap on the total supply of tokens (1 billion)
    uint256 public constant CAP = 1_000_000_000 * 10**18;

    // Token metadata
    string public constant VERSION = "1.0.0";

    /**
     * @dev Constructor that gives msg.sender all of existing tokens.
     */
    constructor() ERC20("Norn Token", "NORN") Ownable(msg.sender) {
        // Mint initial supply to deployer (10 million tokens)
        _mint(msg.sender, 10_000_000 * 10**decimals());
    }

    /**
     * @dev Mints tokens to an address.
     * @param to The address to mint tokens to.
     * @param amount The amount of tokens to mint.
     */
    function mint(address to, uint256 amount) public onlyOwner {
        require(totalSupply() + amount <= CAP, "NornToken: cap exceeded");
        _mint(to, amount);
    }

    /**
     * @dev Burns tokens from an address.
     * @param from The address to burn tokens from.
     * @param amount The amount of tokens to burn.
     */
    function burn(address from, uint256 amount) public onlyOwner {
        _burn(from, amount);
    }

    /**
     * @dev Returns the cap on the token's total supply.
     */
    function cap() public pure returns (uint256) {
        return CAP;
    }
}
