const { expect } = require("chai");
const { ethers } = require("hardhat");

describe("SimpleStorage", function () {
  let storage;
  let owner;
  let addr1;

  beforeEach(async function () {
    [owner, addr1] = await ethers.getSigners();

    const SimpleStorage = await ethers.getContractFactory("SimpleStorage");
    storage = await SimpleStorage.deploy();
    await storage.waitForDeployment();
  });

  describe("Value Storage", function () {
    it("Should store and retrieve a value", async function () {
      const setValueTx = await storage.setValue(42);
      await setValueTx.wait();

      expect(await storage.getValue()).to.equal(42);
    });

    it("Should emit ValueChanged event", async function () {
      await expect(storage.setValue(100))
        .to.emit(storage, "ValueChanged")
        .withArgs(100, owner.address);
    });

    it("Should allow multiple updates", async function () {
      await storage.setValue(1);
      expect(await storage.getValue()).to.equal(1);

      await storage.setValue(2);
      expect(await storage.getValue()).to.equal(2);

      await storage.setValue(999);
      expect(await storage.getValue()).to.equal(999);
    });
  });

  describe("Message Storage", function () {
    it("Should store and retrieve a message", async function () {
      const message = "Hello, Norn!";
      await storage.setMessage(message);

      expect(await storage.getMessage()).to.equal(message);
    });

    it("Should emit MessageChanged event", async function () {
      const message = "Test message";
      await expect(storage.setMessage(message))
        .to.emit(storage, "MessageChanged")
        .withArgs(message, owner.address);
    });

    it("Should handle empty messages", async function () {
      await storage.setMessage("");
      expect(await storage.getMessage()).to.equal("");
    });

    it("Should handle long messages", async function () {
      const longMessage = "A".repeat(1000);
      await storage.setMessage(longMessage);
      expect(await storage.getMessage()).to.equal(longMessage);
    });
  });

  describe("Item Management", function () {
    it("Should add an item and return correct ID", async function () {
      const tx = await storage.addItem("First item");
      const receipt = await tx.wait();

      // Check ItemAdded event
      const event = receipt.logs.find(log => {
        try {
          return storage.interface.parseLog(log).name === "ItemAdded";
        } catch {
          return false;
        }
      });

      expect(event).to.not.be.undefined;
      expect(await storage.itemCount()).to.equal(1);
    });

    it("Should retrieve stored item correctly", async function () {
      const data = "Test data";
      await storage.addItem(data);

      const item = await storage.getItem(1);
      expect(item[0]).to.equal(1); // id
      expect(item[1]).to.equal(owner.address); // owner
      expect(item[3]).to.equal(data); // data
      expect(item[2]).to.be.greaterThan(0); // timestamp
    });

    it("Should add multiple items with unique IDs", async function () {
      await storage.addItem("Item 1");
      await storage.addItem("Item 2");
      await storage.addItem("Item 3");

      expect(await storage.itemCount()).to.equal(3);

      const item1 = await storage.getItem(1);
      const item2 = await storage.getItem(2);
      const item3 = await storage.getItem(3);

      expect(item1[3]).to.equal("Item 1");
      expect(item2[3]).to.equal("Item 2");
      expect(item3[3]).to.equal("Item 3");
    });

    it("Should correctly track item owner", async function () {
      await storage.addItem("Owner's item");

      // Add item from different address
      await storage.connect(addr1).addItem("Addr1's item");

      const item1 = await storage.getItem(1);
      const item2 = await storage.getItem(2);

      expect(item1[1]).to.equal(owner.address);
      expect(item2[1]).to.equal(addr1.address);
    });
  });

  describe("Gas Usage", function () {
    it("Should report gas for setValue", async function () {
      const tx = await storage.setValue(123);
      const receipt = await tx.wait();

      console.log(`setValue gas used: ${receipt.gasUsed.toString()}`);
    });

    it("Should report gas for setMessage", async function () {
      const tx = await storage.setMessage("Test");
      const receipt = await tx.wait();

      console.log(`setMessage gas used: ${receipt.gasUsed.toString()}`);
    });

    it("Should report gas for addItem", async function () {
      const tx = await storage.addItem("New item");
      const receipt = await tx.wait();

      console.log(`addItem gas used: ${receipt.gasUsed.toString()}`);
    });
  });
});
