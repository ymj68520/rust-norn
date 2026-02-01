const { expect } = require("chai");
const { ethers } = require("hardhat");

describe("NornToken", function () {
  let nornToken;
  let owner;
  let addr1;
  let addr2;

  beforeEach(async function () {
    [owner, addr1, addr2] = await ethers.getSigners();

    const NornToken = await ethers.getContractFactory("NornToken");
    nornToken = await NornToken.deploy();
    await nornToken.waitForDeployment();
  });

  describe("Deployment", function () {
    it("Should set the right owner", async function () {
      expect(await nornToken.owner()).to.equal(owner.address);
    });

    it("Should mint initial supply to owner", async function () {
      const ownerBalance = await nornToken.balanceOf(owner.address);
      expect(await nornToken.totalSupply()).to.equal(ownerBalance);
      expect(ownerBalance).to.equal(ethers.parseEther("10000000"));
    });

    it("Should have correct name and symbol", async function () {
      expect(await nornToken.name()).to.equal("Norn Token");
      expect(await nornToken.symbol()).to.equal("NORN");
    });
  });

  describe("Transactions", function () {
    it("Should transfer tokens between accounts", async function () {
      // Transfer 50 tokens from owner to addr1
      await nornToken.transfer(addr1.address, 50);
      const addr1Balance = await nornToken.balanceOf(addr1.address);
      expect(addr1Balance).to.equal(50);

      // Transfer 50 tokens from addr1 to addr2
      await nornToken.connect(addr1).transfer(addr2.address, 50);
      const addr2Balance = await nornToken.balanceOf(addr2.address);
      expect(addr2Balance).to.equal(50);
      expect(await nornToken.balanceOf(addr1.address)).to.equal(0);
    });

    it("Should fail if sender doesn't have enough tokens", async function () {
      const initialOwnerBalance = await nornToken.balanceOf(owner.address);

      // Try to send 1 token from addr1 (0 tokens) to owner
      await expect(
        nornToken.connect(addr1).transfer(owner.address, 1)
      ).to.be.revertedWithCustomError(nornToken, "ERC20InsufficientBalance");

      // Owner balance shouldn't have changed
      expect(await nornToken.balanceOf(owner.address)).to.equal(
        initialOwnerBalance
      );
    });

    it("Should update balances after transfers", async function () {
      const initialOwnerBalance = await nornToken.balanceOf(owner.address);

      // Transfer 100 tokens from owner to addr1
      await nornToken.transfer(addr1.address, 100);

      // Check balances
      const finalOwnerBalance = await nornToken.balanceOf(owner.address);
      const finalAddr1Balance = await nornToken.balanceOf(addr1.address);

      expect(finalOwnerBalance).to.equal(initialOwnerBalance - 100n);
      expect(finalAddr1Balance).to.equal(100);
    });
  });

  describe("Minting", function () {
    it("Should allow owner to mint tokens", async function () {
      const mintAmount = ethers.parseEther("1000");
      await nornToken.mint(addr1.address, mintAmount);

      expect(await nornToken.balanceOf(addr1.address)).to.equal(mintAmount);
    });

    it("Should fail if non-owner tries to mint", async function () {
      await expect(
        nornToken.connect(addr1).mint(addr1.address, 100)
      ).to.be.revertedWithCustomError(nornToken, "OwnableUnauthorizedAccount");
    });

    it("Should enforce cap on total supply", async function () {
      const CAP = ethers.parseEther("1000000000");
      const initialSupply = ethers.parseEther("10000000");

      // Try to mint more than cap allows
      await expect(
        nornToken.mint(addr1.address, CAP - initialSupply + 1n)
      ).to.be.revertedWith("NornToken: cap exceeded");

      // Minting up to cap should work
      await nornToken.mint(addr1.address, CAP - initialSupply);
      expect(await nornToken.totalSupply()).to.equal(CAP);
    });
  });

  describe("Burning", function () {
    it("Should allow owner to burn tokens", async function () {
      const initialBalance = await nornToken.balanceOf(owner.address);
      const burnAmount = ethers.parseEther("1000");

      await nornToken.burn(owner.address, burnAmount);

      expect(await nornToken.balanceOf(owner.address)).to.equal(
        initialBalance - burnAmount
      );
    });

    it("Should fail if non-owner tries to burn", async function () {
      await expect(
        nornToken.connect(addr1).burn(owner.address, 100)
      ).to.be.revertedWithCustomError(nornToken, "OwnableUnauthorizedAccount");
    });
  });

  describe("Cap", function () {
    it("Should return correct cap", async function () {
      expect(await nornToken.cap()).to.equal(ethers.parseEther("1000000000"));
    });
  });
});
