// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/**
 * @title SimpleStorage
 * @dev A simple storage contract for testing EVM functionality
 */
contract SimpleStorage {
    uint256 private value;
    string private message;

    struct Item {
        uint256 id;
        address owner;
        uint256 timestamp;
        string data;
    }

    mapping(uint256 => Item) public items;
    uint256 public itemCount;

    event ValueChanged(uint256 newValue, address indexed changer);
    event MessageChanged(string newMessage, address indexed changer);
    event ItemAdded(uint256 itemId, address indexed owner);

    /**
     * @dev Stores a new value in the contract.
     * @param newValue The new value to store.
     */
    function setValue(uint256 newValue) public {
        value = newValue;
        emit ValueChanged(newValue, msg.sender);
    }

    /**
     * @dev Retrieves the stored value.
     * @return The stored value.
     */
    function getValue() public view returns (uint256) {
        return value;
    }

    /**
     * @dev Stores a message in the contract.
     * @param newMessage The new message to store.
     */
    function setMessage(string memory newMessage) public {
        message = newMessage;
        emit MessageChanged(newMessage, msg.sender);
    }

    /**
     * @dev Retrieves the stored message.
     * @return The stored message.
     */
    function getMessage() public view returns (string memory) {
        return message;
    }

    /**
     * @dev Adds an item to storage.
     * @param data The data to store.
     * @return The ID of the newly created item.
     */
    function addItem(string memory data) public returns (uint256) {
        itemCount++;
        items[itemCount] = Item({
            id: itemCount,
            owner: msg.sender,
            timestamp: block.timestamp,
            data: data
        });
        emit ItemAdded(itemCount, msg.sender);
        return itemCount;
    }

    /**
     * @dev Retrieves an item by ID.
     * @param itemId The ID of the item to retrieve.
     * @return id The item ID.
     * @return owner The item owner.
     * @return timestamp The item timestamp.
     * @return data The item data.
     */
    function getItem(uint256 itemId) public view returns (
        uint256 id,
        address owner,
        uint256 timestamp,
        string memory data
    ) {
        Item memory item = items[itemId];
        return (item.id, item.owner, item.timestamp, item.data);
    }
}
