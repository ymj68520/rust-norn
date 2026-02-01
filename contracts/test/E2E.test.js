const { expect } = require("chai");
const { ethers } = require("hardhat");

/**
 * End-to-End Integration Tests
 *
 * These tests simulate real-world scenarios and verify the complete
 * integration between the norn blockchain EVM and smart contracts.
 */
describe("E2E Integration Tests", function () {
  let token, storage, ballot;
  let owner, addr1, addr2, addr3;

  before(async function () {
    [owner, addr1, addr2, addr3] = await ethers.getSigners();

    // Deploy all contracts
    const NornToken = await ethers.getContractFactory("NornToken");
    token = await NornToken.deploy();
    await token.waitForDeployment();

    const SimpleStorage = await ethers.getContractFactory("SimpleStorage");
    storage = await SimpleStorage.deploy();
    await storage.waitForDeployment();

    const Ballot = await ethers.getContractFactory("Ballot");
    ballot = await Ballot.deploy(["Proposal A", "Proposal B", "Proposal C"]);
    await ballot.waitForDeployment();
  });

  describe("Scenario 1: Token Distribution & Voting", function () {
    it("Should complete full token distribution and voting cycle", async function () {
      // Step 1: Mint tokens to multiple users
      const mintAmount = ethers.parseEther("10000");

      await token.mint(addr1.address, mintAmount);
      await token.mint(addr2.address, mintAmount);
      await token.mint(addr3.address, mintAmount);

      // Verify balances
      expect(await token.balanceOf(addr1.address)).to.equal(mintAmount);
      expect(await token.balanceOf(addr2.address)).to.equal(mintAmount);
      expect(await token.balanceOf(addr3.address)).to.equal(mintAmount);

      // Step 2: Transfer tokens between users
      await token.connect(addr1).transfer(addr2.address, ethers.parseEther("1000"));
      expect(await token.balanceOf(addr2.address)).to.equal(ethers.parseEther("11000"));

      // Step 3: Setup voting
      await ballot.giveRightToVote(addr1.address);
      await ballot.giveRightToVote(addr2.address);
      await ballot.giveRightToVote(addr3.address);

      // Step 4: Cast votes
      await ballot.connect(addr1).vote(0); // addr1 votes for Proposal A
      await ballot.connect(addr2).vote(0); // addr2 votes for Proposal A
      await ballot.connect(addr3).vote(1); // addr3 votes for Proposal B

      // Step 5: Verify winner
      const winnerName = await ballot.winnerName();
      expect(winnerName).to.equal("Proposal A");
    });
  });

  describe("Scenario 2: Storage with Token Payments", function () {
    it("Should store data and track payments", async function () {
      // Store some data
      await storage.setValue(100);
      await storage.setMessage("Payment for service");

      // Add multiple items
      await storage.addItem("Service 1");
      await storage.addItem("Service 2");
      await storage.addItem("Service 3");

      // Verify all data
      expect(await storage.getValue()).to.equal(100);
      expect(await storage.getMessage()).to.equal("Payment for service");
      expect(await storage.itemCount()).to.equal(3);

      // Simulate payment for services
      const payment = ethers.parseEther("100");
      await token.transfer(addr1.address, payment);

      // Verify payment
      expect(await token.balanceOf(addr1.address)).to.equal(payment);
    });
  });

  describe("Scenario 3: Vote Delegation", function () {
    it("Should handle complex delegation scenarios", async function () {
      // Setup voting rights
      await ballot.giveRightToVote(addr1.address);
      await ballot.giveRightToVote(addr2.address);
      await ballot.giveRightToVote(addr3.address);

      // addr1 delegates to addr2
      await ballot.connect(addr1).delegate(addr2.address);

      // addr2 votes
      await ballot.connect(addr2).vote(2); // Proposal C

      // addr3 votes for different proposal
      await ballot.connect(addr3).vote(1); // Proposal B

      // addr1's delegated vote should count towards addr2's vote
      const winnerName = await ballot.winnerName();
      expect(winnerName).to.equal("Proposal C");
    });
  });

  describe("Scenario 4: Multi-Contract Interaction", function () {
    it("Should handle interactions between multiple contracts", async function () {
      // Store contract address in storage (as item data)
      const tokenAddress = await token.getAddress();
      await storage.addItem(tokenAddress);

      // Retrieve and verify
      const item = await storage.getItem(1);
      expect(item[3]).to.equal(tokenAddress);

      // Transfer tokens to storage contract (if it had payable functions)
      const storageBalance = await token.balanceOf(await storage.getAddress());
      expect(storageBalance).to.equal(0);
    });
  });

  describe("Scenario 5: Stress Testing", function () {
    it("Should handle multiple rapid transactions", async function () {
      const txs = [];

      // Execute multiple setValue transactions
      for (let i = 0; i < 50; i++) {
        txs.push(storage.setValue(i));
      }

      // Wait for all transactions
      await Promise.all(txs);

      // Verify final value
      const finalValue = await storage.getValue();
      expect(finalValue).to.equal(49); // Last value set
    });

    it("Should handle large message storage", async function () {
      const largeMessage = "x".repeat(2000); // 2000 character message
      await storage.setMessage(largeMessage);

      const retrieved = await storage.getMessage();
      expect(retrieved).to.equal(largeMessage);
      expect(retrieved.length).to.equal(2000);
    });
  });

  describe("Scenario 6: Access Control", function () {
    it("Should enforce access control correctly", async function () {
      // Try to mint from non-owner (should fail)
      await expect(
        token.connect(addr1).mint(addr2.address, 100)
      ).to.be.revertedWithCustomError(token, "OwnableUnauthorizedAccount");

      // Try to burn from non-owner (should fail)
      await expect(
        token.connect(addr1).burn(owner.address, 100)
      ).to.be.revertedWithCustomError(token, "OwnableUnauthorizedAccount");

      // Owner should be able to mint
      await token.mint(addr1.address, ethers.parseEther("500"));
      expect(await token.balanceOf(addr1.address)).to.equal(ethers.parseEther("500"));
    });
  });

  describe("Scenario 7: Event Verification", function () {
    it("Should emit all expected events", async function () {
      // Test ValueChanged event
      await expect(storage.setValue(42))
        .to.emit(storage, "ValueChanged")
        .withArgs(42, owner.address);

      // Test MessageChanged event
      await expect(storage.setMessage("Test"))
        .to.emit(storage, "MessageChanged")
        .withArgs("Test", owner.address);

      // Test ItemAdded event
      await expect(storage.addItem("New Item"))
        .to.emit(storage, "ItemAdded")
        .withArgs(1, owner.address);

      // Test Transfer event
      await expect(token.transfer(addr1.address, 100))
        .to.emit(token, "Transfer")
        .withArgs(owner.address, addr1.address, 100);
    });
  });

  describe("Scenario 8: Gas Optimization", function () {
    it("Should report gas usage for common operations", async function () {
      // Measure gas for token operations
      const transferTx = await token.transfer(addr1.address, 100);
      const transferReceipt = await transferTx.wait();
      console.log("Token transfer gas:", transferReceipt.gasUsed.toString());

      // Measure gas for storage operations
      const setValueTx = await storage.setValue(123);
      const setValueReceipt = await setValueTx.wait();
      console.log("Set value gas:", setValueReceipt.gasUsed.toString());

      // Measure gas for voting
      await ballot.giveRightToVote(addr1.address);
      const voteTx = await ballot.connect(addr1).vote(0);
      const voteReceipt = await voteTx.wait();
      console.log("Vote gas:", voteReceipt.gasUsed.toString());
    });
  });

  describe("Scenario 9: Error Handling", function () {
    it("Should handle errors gracefully", async function () {
      // Try to vote without rights
      await expect(
        ballot.connect(addr1).vote(0)
      ).to.be.reverted; // Will fail because addr1 has no voting rights

      // Give voting rights and try again
      await ballot.giveRightToVote(addr1.address);
      await ballot.connect(addr1).vote(0); // Should succeed

      // Try to vote twice
      await expect(
        ballot.connect(addr1).vote(1)
      ).to.be.revertedWith("Already voted");
    });
  });

  describe("Scenario 10: Time-Based Operations", function () {
    it("Should handle block timestamps correctly", async function () {
      // Add an item and check its timestamp
      const blockNumBefore = await ethers.provider.getBlockNumber();
      const blockBefore = await ethers.provider.getBlock(blockNumBefore);
      const timestampBefore = blockBefore.timestamp;

      await storage.addItem("Timed item");

      const item = await storage.getItem(1);
      const itemTimestamp = item[2];

      // Verify timestamp is reasonable
      expect(itemTimestamp).to.be.at.least(timestampBefore);
      expect(itemTimestamp).to.be.at.most(timestampBefore + 60); // Within 1 minute
    });
  });
});
