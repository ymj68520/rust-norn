// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/**
 * @title Ballot
 * @dev A simple voting contract for testing EVM functionality
 */
contract Ballot {
    struct Voter {
        uint256 weight;
        bool voted;
        address delegate;
        uint256 vote;
    }

    struct Proposal {
        string name;
        uint256 voteCount;
    }

    address public chairperson;
    mapping(address => Voter) public voters;
    Proposal[] public proposals;

    event Voted(address indexed voter, uint256 proposalId);
    event Delegated(address indexed from, address indexed to);

    /**
     * @dev Creates a new ballot with proposals.
     * @param proposalNames Names of the proposals.
     */
    constructor(string[] memory proposalNames) {
        chairperson = msg.sender;
        voters[chairperson].weight = 1;

        for (uint256 i = 0; i < proposalNames.length; i++) {
            proposals.push(Proposal({
                name: proposalNames[i],
                voteCount: 0
            }));
        }
    }

    /**
     * @dev Gives voting rights to an address.
     * @param to The address to give voting rights to.
     */
    function giveRightToVote(address to) public {
        require(
            msg.sender == chairperson,
            "Only chairperson can give right to vote"
        );
        require(
            !voters[to].voted,
            "Voter already voted"
        );
        require(voters[to].weight == 0);
        voters[to].weight = 1;
    }

    /**
     * @dev Delegates your vote to another voter.
     * @param to The address to delegate to.
     */
    function delegate(address to) public {
        Voter storage sender = voters[msg.sender];

        require(!sender.voted, "You already voted");
        require(to != msg.sender, "Self-delegation is disallowed");

        while (voters[to].delegate != address(0)) {
            to = voters[to].delegate;

            require(to != msg.sender, "Found loop in delegation");
        }

        sender.voted = true;
        sender.delegate = to;

        Voter storage delegate_ = voters[to];
        if (delegate_.voted) {
            proposals[delegate_.vote].voteCount += sender.weight;
        } else {
            delegate_.weight += sender.weight;
        }

        emit Delegated(msg.sender, to);
    }

    /**
     * @dev Casts a vote for a proposal.
     * @param proposalId The ID of the proposal to vote for.
     */
    function vote(uint256 proposalId) public {
        Voter storage sender = voters[msg.sender];

        require(sender.weight != 0, "Has no right to vote");
        require(!sender.voted, "Already voted");
        sender.voted = true;
        sender.vote = proposalId;

        proposals[proposalId].voteCount += sender.weight;
        emit Voted(msg.sender, proposalId);
    }

    /**
     * @dev Computes the winning proposal.
     * @return winningProposal The ID of the winning proposal.
     */
    function winningProposal() public view returns (uint256 winningProposal) {
        uint256 winningVoteCount = 0;
        for (uint256 p = 0; p < proposals.length; p++) {
            if (proposals[p].voteCount > winningVoteCount) {
                winningVoteCount = proposals[p].voteCount;
                winningProposal = p;
            }
        }
    }

    /**
     * @dev Gets the name of the winning proposal.
     * @return winnerName The name of the winning proposal.
     */
    function winnerName() public view returns (string memory winnerName) {
        winnerName = proposals[winningProposal()].name;
    }
}
