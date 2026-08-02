// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/**
 * @title DeFiVault
 * @dev A multi-feature DeFi Vault demonstrating:
 *  - ERC-20 style token accounting & allowance
 *  - Staking & Reward Distribution (Synthetix-style reward rate per token)
 *  - Constant-Product AMM Liquidity Swap (x * y = k) with 0.3% fee
 *  - Complex state updates, multi-mapping reads/writes, and event emissions
 */
contract DeFiVault {
    // Contract Owner
    address public owner;
    
    // ERC-20 State
    string public name = "Norn DeFi Vault Token";
    string public symbol = "NDVT";
    uint8 public decimals = 18;
    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    // Staking State
    uint256 public totalStaked;
    mapping(address => uint256) public stakedBalance;
    mapping(address => uint256) public userRewardPerTokenPaid;
    mapping(address => uint256) public rewards;
    uint256 public rewardPerTokenStored;
    uint256 public lastUpdateTime;
    uint256 public rewardRate = 100; // 100 reward units per second

    // AMM Pool State (Token A and Token B reserve accounting inside the vault)
    uint256 public reserveA;
    uint256 public reserveB;

    // Events
    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Staked(address indexed user, uint256 amount);
    event Withdrawn(address indexed user, uint256 amount);
    event RewardPaid(address indexed user, uint256 reward);
    event SwapExecuted(address indexed sender, uint256 amountIn, uint256 amountOut, bool aToB);
    event LiquidityAdded(address indexed provider, uint256 amountA, uint256 amountB);

    modifier onlyOwner() {
        require(msg.sender == owner, "Only owner can call");
        _;
    }

    modifier updateReward(address account) {
        rewardPerTokenStored = rewardPerToken();
        lastUpdateTime = block.timestamp;
        if (account != address(0)) {
            rewards[account] = earned(account);
            userRewardPerTokenPaid[account] = rewardPerTokenStored;
        }
        _;
    }

    constructor(uint256 initialSupply) {
        owner = msg.sender;
        totalSupply = initialSupply;
        balanceOf[msg.sender] = initialSupply;
        lastUpdateTime = block.timestamp;
        emit Transfer(address(0), msg.sender, initialSupply);
    }

    // --- ERC-20 Functions ---

    function transfer(address recipient, uint256 amount) external returns (bool) {
        require(balanceOf[msg.sender] >= amount, "Insufficient balance");
        balanceOf[msg.sender] -= amount;
        balanceOf[recipient] += amount;
        emit Transfer(msg.sender, recipient, amount);
        return true;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
        return true;
    }

    function mint(address to, uint256 amount) external onlyOwner {
        totalSupply += amount;
        balanceOf[to] += amount;
        emit Transfer(address(0), to, amount);
    }

    // --- Staking & Rewards Functions ---

    function rewardPerToken() public view returns (uint256) {
        if (totalStaked == 0) {
            return rewardPerTokenStored;
        }
        return rewardPerTokenStored + ((block.timestamp - lastUpdateTime) * rewardRate * 1e18 / totalStaked);
    }

    function earned(address account) public view returns (uint256) {
        return (stakedBalance[account] * (rewardPerToken() - userRewardPerTokenPaid[account]) / 1e18) + rewards[account];
    }

    function stake(uint256 amount) external updateReward(msg.sender) {
        require(amount > 0, "Cannot stake 0");
        require(balanceOf[msg.sender] >= amount, "Insufficient balance to stake");

        balanceOf[msg.sender] -= amount;
        stakedBalance[msg.sender] += amount;
        totalStaked += amount;

        emit Staked(msg.sender, amount);
    }

    function withdrawStaked(uint256 amount) external updateReward(msg.sender) {
        require(amount > 0, "Cannot withdraw 0");
        require(stakedBalance[msg.sender] >= amount, "Insufficient staked balance");

        stakedBalance[msg.sender] -= amount;
        totalStaked -= amount;
        balanceOf[msg.sender] += amount;

        emit Withdrawn(msg.sender, amount);
    }

    function claimReward() external updateReward(msg.sender) returns (uint256) {
        uint256 reward = rewards[msg.sender];
        if (reward > 0) {
            rewards[msg.sender] = 0;
            totalSupply += reward;
            balanceOf[msg.sender] += reward;
            emit RewardPaid(msg.sender, reward);
        }
        return reward;
    }

    // --- Constant-Product AMM Liquidity & Swap ---

    function addLiquidity(uint256 amountA, uint256 amountB) external onlyOwner {
        require(amountA > 0 && amountB > 0, "Invalid liquidity amounts");
        reserveA += amountA;
        reserveB += amountB;
        emit LiquidityAdded(msg.sender, amountA, amountB);
    }

    function getAmountOut(uint256 amountIn, uint256 reserveIn, uint256 reserveOut) public pure returns (uint256) {
        require(amountIn > 0, "Insufficient input amount");
        require(reserveIn > 0 && reserveOut > 0, "Insufficient liquidity");
        
        // 0.3% fee calculation (997/1000)
        uint256 amountInWithFee = amountIn * 997;
        uint256 numerator = amountInWithFee * reserveOut;
        uint256 denominator = (reserveIn * 1000) + amountInWithFee;
        return numerator / denominator;
    }

    function swap(uint256 amountIn, bool aToB) external returns (uint256 amountOut) {
        require(amountIn > 0, "Must swap more than 0");
        
        if (aToB) {
            amountOut = getAmountOut(amountIn, reserveA, reserveB);
            require(balanceOf[msg.sender] >= amountIn, "Insufficient Token A balance");
            require(reserveB >= amountOut, "Insufficient pool liquidity B");

            balanceOf[msg.sender] -= amountIn;
            reserveA += amountIn;
            reserveB -= amountOut;
            balanceOf[msg.sender] += amountOut;
        } else {
            amountOut = getAmountOut(amountIn, reserveB, reserveA);
            require(balanceOf[msg.sender] >= amountIn, "Insufficient Token B balance");
            require(reserveA >= amountOut, "Insufficient pool liquidity A");

            balanceOf[msg.sender] -= amountIn;
            reserveB += amountIn;
            reserveA -= amountOut;
            balanceOf[msg.sender] += amountOut;
        }

        emit SwapExecuted(msg.sender, amountIn, amountOut, aToB);
    }
}
