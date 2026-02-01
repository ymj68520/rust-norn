// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/**
 * @title SimpleStorage
 * @dev A simple contract for testing Remix with norn blockchain
 *      Store and retrieve a value
 */
contract SimpleStorage {
    uint256 private value;

    event ValueChanged(uint256 newValue, address indexed changer);

    /**
     * @dev Store a new value
     * @param _value The value to store
     */
    function setValue(uint256 _value) public {
        value = _value;
        emit ValueChanged(_value, msg.sender);
    }

    /**
     * @dev Get the stored value
     * @return The stored value
     */
    function getValue() public view returns (uint256) {
        return value;
    }
}

/**
 * @title Counter
 * @dev A simple counter contract that increments
 */
contract Counter {
    uint256 private count;
    address public owner;

    event CountIncremented(uint256 newCount, address indexed incrementer);

    constructor() {
        count = 0;
        owner = msg.sender;
    }

    function increment() public {
        count += 1;
        emit CountIncremented(count, msg.sender);
    }

    function getCount() public view returns (uint256) {
        return count;
    }

    function reset() public {
        require(msg.sender == owner, "Only owner can reset");
        count = 0;
    }
}

/**
 * @title SimpleToken
 * @dev A simple token contract for testing
 */
contract SimpleToken {
    mapping(address => uint256) public balanceOf;
    uint256 public totalSupply;
    string public name = "Simple Token";
    string public symbol = "ST";
    uint8 public decimals = 18;

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Mint(address indexed to, uint256 value);

    constructor(uint256 initialSupply) {
        totalSupply = initialSupply;
        balanceOf[msg.sender] = initialSupply;
        emit Mint(msg.sender, initialSupply);
    }

    function transfer(address to, uint256 amount) public returns (bool) {
        require(balanceOf[msg.sender] >= amount, "Insufficient balance");
        require(to != address(0), "Cannot transfer to zero address");

        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;

        emit Transfer(msg.sender, to, amount);
        return true;
    }

    function mint(address to, uint256 amount) public {
        totalSupply += amount;
        balanceOf[to] += amount;
        emit Mint(to, amount);
        emit Transfer(address(0), to, amount);
    }

    function getBalance(address account) public view returns (uint256) {
        return balanceOf[account];
    }
}
