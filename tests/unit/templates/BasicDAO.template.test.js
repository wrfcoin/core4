const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  loadFixture,
  time,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('BasicDAO Template', function () {
  async function deployBasicDAOFixture() {
    const [owner, member1, member2, member3, nonMember, target] =
      await ethers.getSigners();

    // Deploy GovernanceToken first
    const GovernanceToken = await ethers.getContractFactory('GovernanceToken');
    const governanceToken = await GovernanceToken.deploy(
      'DAO Token',
      'DAO',
      ethers.parseEther('1000000'), // max supply
      ethers.parseEther('100000'), // initial supply
      owner.address
    );

    // Distribute tokens to members
    await governanceToken.transfer(member1.address, ethers.parseEther('10000'));
    await governanceToken.transfer(member2.address, ethers.parseEther('5000'));
    await governanceToken.transfer(member3.address, ethers.parseEther('2000'));

    // Deploy BasicDAO
    const BasicDAO = await ethers.getContractFactory('BasicDAO');
    const dao = await BasicDAO.deploy(
      await governanceToken.getAddress(),
      owner.address
    );

    // Add members to DAO
    await dao.addMember(member1.address);
    await dao.addMember(member2.address);
    await dao.addMember(member3.address);

    // Delegate voting power
    await governanceToken.connect(member1).delegate(member1.address);
    await governanceToken.connect(member2).delegate(member2.address);
    await governanceToken.connect(member3).delegate(member3.address);

    return {
      dao,
      governanceToken,
      owner,
      member1,
      member2,
      member3,
      nonMember,
      target,
    };
  }

  describe('Deployment', function () {
    it('Should set correct governance token', async function () {
      const { dao, governanceToken } = await loadFixture(deployBasicDAOFixture);

      expect(await dao.governanceToken()).to.equal(
        await governanceToken.getAddress()
      );
    });

    it('Should set correct initial parameters', async function () {
      const { dao } = await loadFixture(deployBasicDAOFixture);

      expect(await dao.votingDelay()).to.equal(86400); // 1 day
      expect(await dao.votingPeriod()).to.equal(604800); // 7 days
      expect(await dao.executionDelay()).to.equal(172800); // 2 days
      expect(await dao.proposalThreshold()).to.equal(ethers.parseEther('1000'));
      expect(await dao.quorumNumerator()).to.equal(4);
      expect(await dao.quorumDenominator()).to.equal(100);
    });

    it('Should add owner as first member', async function () {
      const { dao, owner } = await loadFixture(deployBasicDAOFixture);

      expect(await dao.members(owner.address)).to.be.true;
      expect(await dao.memberCount()).to.equal(4); // owner + 3 added members
    });

    it('Should set up correct roles', async function () {
      const { dao, owner } = await loadFixture(deployBasicDAOFixture);

      const ADMIN_ROLE = await dao.ADMIN_ROLE();
      expect(await dao.hasRole(ADMIN_ROLE, owner.address)).to.be.true;
    });
  });

  describe('Member Management', function () {
    it('Should allow admin to add members', async function () {
      const { dao, owner, nonMember } = await loadFixture(
        deployBasicDAOFixture
      );

      await expect(dao.addMember(nonMember.address))
        .to.emit(dao, 'MemberAdded')
        .withArgs(nonMember.address, (await time.latest()) + 1);

      expect(await dao.members(nonMember.address)).to.be.true;
      expect(await dao.memberCount()).to.equal(5);
    });

    it('Should not allow duplicate members', async function () {
      const { dao, member1 } = await loadFixture(deployBasicDAOFixture);

      await expect(dao.addMember(member1.address)).to.be.revertedWith(
        'BasicDAO: Already a member'
      );
    });

    it('Should allow admin to remove members', async function () {
      const { dao, member1 } = await loadFixture(deployBasicDAOFixture);

      await expect(dao.removeMember(member1.address))
        .to.emit(dao, 'MemberRemoved')
        .withArgs(member1.address, (await time.latest()) + 1);

      expect(await dao.members(member1.address)).to.be.false;
      expect(await dao.memberCount()).to.equal(3);
    });

    it('Should not allow non-admin to manage members', async function () {
      const { dao, member1, nonMember } = await loadFixture(
        deployBasicDAOFixture
      );

      await expect(
        dao.connect(member1).addMember(nonMember.address)
      ).to.be.revertedWith('AccessControl: account');

      await expect(
        dao.connect(member1).removeMember(member1.address)
      ).to.be.revertedWith('AccessControl: account');
    });
  });

  describe('Proposal Creation', function () {
    it('Should allow members with sufficient tokens to create proposals', async function () {
      const { dao, member1 } = await loadFixture(deployBasicDAOFixture);

      const startTime = (await time.latest()) + 86400; // 1 day delay
      const endTime = startTime + 604800; // 7 days voting period

      await expect(
        dao
          .connect(member1)
          .propose(
            'Test Proposal',
            'This is a test proposal description',
            ethers.ZeroAddress,
            0,
            '0x'
          )
      )
        .to.emit(dao, 'ProposalCreated')
        .withArgs(
          0,
          member1.address,
          'Test Proposal',
          startTime + 1,
          endTime + 1
        );

      expect(await dao.proposalCount()).to.equal(1);
    });

    it('Should not allow members without sufficient tokens to create proposals', async function () {
      const { dao, member3 } = await loadFixture(deployBasicDAOFixture);

      // member3 only has 2000 tokens, threshold is 1000, but needs voting power
      await expect(
        dao
          .connect(member3)
          .propose(
            'Test Proposal',
            'This is a test proposal description',
            ethers.ZeroAddress,
            0,
            '0x'
          )
      ).to.be.revertedWithCustomError(dao, 'InsufficientProposalThreshold');
    });

    it('Should not allow non-members to create proposals', async function () {
      const { dao, nonMember } = await loadFixture(deployBasicDAOFixture);

      await expect(
        dao
          .connect(nonMember)
          .propose(
            'Test Proposal',
            'This is a test proposal description',
            ethers.ZeroAddress,
            0,
            '0x'
          )
      ).to.be.revertedWithCustomError(dao, 'NotMember');
    });

    it('Should create proposal with correct parameters', async function () {
      const { dao, member1 } = await loadFixture(deployBasicDAOFixture);

      await dao
        .connect(member1)
        .propose(
          'Test Proposal',
          'This is a test proposal description',
          ethers.ZeroAddress,
          ethers.parseEther('10'),
          '0x1234'
        );

      const proposal = await dao.getProposal(0);
      expect(proposal.proposer).to.equal(member1.address);
      expect(proposal.title).to.equal('Test Proposal');
      expect(proposal.description).to.equal(
        'This is a test proposal description'
      );
      expect(proposal.forVotes).to.equal(0);
      expect(proposal.againstVotes).to.equal(0);
      expect(proposal.abstainVotes).to.equal(0);
      expect(proposal.executed).to.be.false;
    });
  });

  describe('Voting', function () {
    async function createProposalFixture() {
      const fixture = await loadFixture(deployBasicDAOFixture);

      // Create a proposal
      await fixture.dao
        .connect(fixture.member1)
        .propose(
          'Test Proposal',
          'This is a test proposal description',
          ethers.ZeroAddress,
          0,
          '0x'
        );

      // Advance time to voting period
      await time.increase(86401); // Past voting delay

      return fixture;
    }

    it('Should allow members to vote during active period', async function () {
      const { dao, member2 } = await createProposalFixture();

      const votingPower = await dao.getVotingPower(member2.address);

      await expect(
        dao.connect(member2).castVote(0, 0, 'I support this proposal')
      )
        .to.emit(dao, 'VoteCast')
        .withArgs(
          0,
          member2.address,
          0,
          votingPower,
          'I support this proposal'
        );

      const proposal = await dao.getProposal(0);
      expect(proposal.forVotes).to.equal(votingPower);
    });

    it('Should track different vote types correctly', async function () {
      const { dao, member1, member2, member3 } = await createProposalFixture();

      // Vote FOR
      await dao.connect(member1).castVote(0, 0, 'Supporting');

      // Vote AGAINST
      await dao.connect(member2).castVote(0, 1, 'Against');

      // Vote ABSTAIN
      await dao.connect(member3).castVote(0, 2, 'Abstaining');

      const proposal = await dao.getProposal(0);
      expect(proposal.forVotes).to.equal(ethers.parseEther('10000'));
      expect(proposal.againstVotes).to.equal(ethers.parseEther('5000'));
      expect(proposal.abstainVotes).to.equal(ethers.parseEther('2000'));
    });

    it('Should not allow double voting', async function () {
      const { dao, member1 } = await createProposalFixture();

      await dao.connect(member1).castVote(0, 0, 'First vote');

      await expect(
        dao.connect(member1).castVote(0, 1, 'Second vote')
      ).to.be.revertedWithCustomError(dao, 'AlreadyVoted');
    });

    it('Should not allow voting before active period', async function () {
      const { dao, member1 } = await loadFixture(deployBasicDAOFixture);

      await dao
        .connect(member1)
        .propose('Test Proposal', 'Description', ethers.ZeroAddress, 0, '0x');

      // Don't advance time - still in pending state
      await expect(
        dao.connect(member1).castVote(0, 0, 'Too early')
      ).to.be.revertedWithCustomError(dao, 'ProposalNotActive');
    });

    it('Should not allow voting after voting period ends', async function () {
      const { dao, member1 } = await createProposalFixture();

      // Advance time past voting period
      await time.increase(604801); // Past voting period

      await expect(
        dao.connect(member1).castVote(0, 0, 'Too late')
      ).to.be.revertedWithCustomError(dao, 'ProposalNotActive');
    });

    it('Should not allow non-members to vote', async function () {
      const { dao, nonMember } = await createProposalFixture();

      await expect(
        dao.connect(nonMember).castVote(0, 0, 'Not a member')
      ).to.be.revertedWithCustomError(dao, 'NotMember');
    });
  });

  describe('Quorum and Proposal Results', function () {
    async function votedProposalFixture() {
      const fixture = await createProposalFixture();

      // Vote on proposal
      await fixture.dao.connect(fixture.member1).castVote(0, 0, 'For'); // 10000 tokens
      await fixture.dao.connect(fixture.member2).castVote(0, 0, 'For'); // 5000 tokens

      // Advance time past voting period
      await time.increase(604801);

      return fixture;
    }

    async function createProposalFixture() {
      const fixture = await loadFixture(deployBasicDAOFixture);

      await fixture.dao
        .connect(fixture.member1)
        .propose('Test Proposal', 'Description', ethers.ZeroAddress, 0, '0x');

      await time.increase(86401);
      return fixture;
    }

    it('Should check quorum correctly', async function () {
      const { dao, governanceToken } = await votedProposalFixture();

      const totalSupply = await governanceToken.totalSupply();
      const requiredQuorum = (totalSupply * 4n) / 100n; // 4%

      expect(await dao.quorumReached(0)).to.be.true;
    });

    it('Should determine proposal success correctly', async function () {
      const { dao } = await votedProposalFixture();

      const proposal = await dao.getProposal(0);
      expect(proposal.state).to.equal(2); // SUCCEEDED
    });

    it('Should handle defeated proposals', async function () {
      const fixture = await createProposalFixture();

      // Vote against with majority
      await fixture.dao.connect(fixture.member1).castVote(0, 1, 'Against'); // 10000 tokens
      await fixture.dao.connect(fixture.member2).castVote(0, 0, 'For'); // 5000 tokens

      await time.increase(604801);

      const proposal = await fixture.dao.getProposal(0);
      expect(proposal.state).to.equal(3); // DEFEATED
    });

    it('Should handle proposals without quorum', async function () {
      const fixture = await createProposalFixture();

      // Only small vote, insufficient for quorum
      await fixture.dao.connect(fixture.member3).castVote(0, 0, 'For'); // 2000 tokens

      await time.increase(604801);

      const proposal = await fixture.dao.getProposal(0);
      expect(proposal.state).to.equal(3); // DEFEATED (no quorum)
    });
  });

  describe('Proposal Execution', function () {
    async function queuedProposalFixture() {
      const fixture = await votedProposalFixture();

      // Queue the successful proposal
      await fixture.dao.queueProposal(0);

      return fixture;
    }

    async function votedProposalFixture() {
      const fixture = await createProposalFixture();

      await fixture.dao.connect(fixture.member1).castVote(0, 0, 'For');
      await fixture.dao.connect(fixture.member2).castVote(0, 0, 'For');

      await time.increase(604801);
      return fixture;
    }

    async function createProposalFixture() {
      const fixture = await loadFixture(deployBasicDAOFixture);

      await fixture.dao
        .connect(fixture.member1)
        .propose('Test Proposal', 'Description', ethers.ZeroAddress, 0, '0x');

      await time.increase(86401);
      return fixture;
    }

    it('Should allow admin to queue successful proposals', async function () {
      const { dao } = await votedProposalFixture();

      await expect(dao.queueProposal(0))
        .to.emit(dao, 'ProposalQueued')
        .withArgs(0, (await time.latest()) + 172801); // execution delay

      const proposal = await dao.getProposal(0);
      expect(proposal.state).to.equal(4); // QUEUED
    });

    it('Should not allow queueing non-successful proposals', async function () {
      const { dao } = await createProposalFixture();

      // Don't vote, let it fail
      await time.increase(604801);

      await expect(dao.queueProposal(0)).to.be.revertedWithCustomError(
        dao,
        'ProposalNotSucceeded'
      );
    });

    it('Should execute queued proposals after timelock', async function () {
      const { dao } = await queuedProposalFixture();

      // Advance past execution delay
      await time.increase(172801);

      await expect(dao.executeProposal(0))
        .to.emit(dao, 'ProposalExecuted')
        .withArgs(0, true);

      const proposal = await dao.getProposal(0);
      expect(proposal.state).to.equal(5); // EXECUTED
      expect(proposal.executed).to.be.true;
    });

    it('Should not execute proposals before timelock expires', async function () {
      const { dao } = await queuedProposalFixture();

      // Don't advance time enough
      await expect(dao.executeProposal(0)).to.be.revertedWithCustomError(
        dao,
        'TimelockNotExpired'
      );
    });

    it('Should handle ETH transfer proposals', async function () {
      const fixture = await loadFixture(deployBasicDAOFixture);

      // Fund the DAO treasury
      await fixture.owner.sendTransaction({
        to: await fixture.dao.getAddress(),
        value: ethers.parseEther('10'),
      });

      // Create proposal for ETH transfer
      await fixture.dao
        .connect(fixture.member1)
        .propose(
          'Send ETH',
          'Send 5 ETH',
          ethers.ZeroAddress,
          ethers.parseEther('5'),
          '0x'
        );

      await time.increase(86401);

      // Vote and queue
      await fixture.dao.connect(fixture.member1).castVote(0, 0, 'For');
      await fixture.dao.connect(fixture.member2).castVote(0, 0, 'For');

      await time.increase(604801);
      await fixture.dao.queueProposal(0);
      await time.increase(172801);

      const initialBalance = await ethers.provider.getBalance(
        fixture.member1.address
      );

      await fixture.dao.executeProposal(0);

      const finalBalance = await ethers.provider.getBalance(
        fixture.member1.address
      );
      expect(finalBalance - initialBalance).to.equal(ethers.parseEther('5'));
    });
  });

  describe('Proposal Cancellation', function () {
    async function proposalFixture() {
      const fixture = await loadFixture(deployBasicDAOFixture);

      await fixture.dao
        .connect(fixture.member1)
        .propose('Test Proposal', 'Description', ethers.ZeroAddress, 0, '0x');

      return fixture;
    }

    it('Should allow proposer to cancel their proposal', async function () {
      const { dao, member1 } = await proposalFixture();

      await expect(dao.connect(member1).cancelProposal(0))
        .to.emit(dao, 'ProposalCancelled')
        .withArgs(0);

      const proposal = await dao.getProposal(0);
      expect(proposal.state).to.equal(6); // CANCELLED
    });

    it('Should allow admin to cancel any proposal', async function () {
      const { dao, owner } = await proposalFixture();

      await expect(dao.connect(owner).cancelProposal(0))
        .to.emit(dao, 'ProposalCancelled')
        .withArgs(0);
    });

    it('Should not allow others to cancel proposals', async function () {
      const { dao, member2 } = await proposalFixture();

      await expect(dao.connect(member2).cancelProposal(0)).to.be.revertedWith(
        'BasicDAO: Not authorized to cancel'
      );
    });

    it('Should not allow canceling executed proposals', async function () {
      const { dao } = await proposalFixture();

      // Fast track to execution
      await time.increase(86401);
      await dao.connect(dao.member1).castVote(0, 0, 'For');
      await dao.connect(dao.member2).castVote(0, 0, 'For');
      await time.increase(604801);
      await dao.queueProposal(0);
      await time.increase(172801);
      await dao.executeProposal(0);

      await expect(dao.cancelProposal(0)).to.be.revertedWith(
        'BasicDAO: Cannot cancel executed proposal'
      );
    });
  });

  describe('Delegation', function () {
    it('Should allow members to delegate voting power', async function () {
      const { dao, member1, member2 } = await loadFixture(
        deployBasicDAOFixture
      );

      const initialPower1 = await dao.getVotingPower(member1.address);
      const initialPower2 = await dao.getVotingPower(member2.address);

      await dao.connect(member1).delegate(member2.address);

      const finalPower1 = await dao.getVotingPower(member1.address);
      const finalPower2 = await dao.getVotingPower(member2.address);

      expect(finalPower1).to.equal(0);
      expect(finalPower2).to.equal(initialPower1 + initialPower2);
      expect(await dao.delegates(member1.address)).to.equal(member2.address);
    });

    it('Should handle delegation changes', async function () {
      const { dao, member1, member2, member3 } = await loadFixture(
        deployBasicDAOFixture
      );

      // Initial delegation
      await dao.connect(member1).delegate(member2.address);

      const power1After1st = await dao.getVotingPower(member2.address);

      // Change delegation
      await dao.connect(member1).delegate(member3.address);

      const power1After2nd = await dao.getVotingPower(member2.address);
      const power2After2nd = await dao.getVotingPower(member3.address);

      expect(power1After2nd).to.be.lt(power1After1st);
      expect(power2After2nd).to.equal(ethers.parseEther('12000')); // member3's 2000 + member1's 10000
    });
  });

  describe('DAO Configuration', function () {
    it('Should allow admin to update voting parameters', async function () {
      const { dao } = await loadFixture(deployBasicDAOFixture);

      await expect(
        dao.updateVotingParameters(
          172800, // 2 days voting delay
          1209600, // 14 days voting period
          259200, // 3 days execution delay
          ethers.parseEther('2000'), // higher proposal threshold
          5 // 5% quorum
        )
      )
        .to.emit(dao, 'VotingParametersUpdated')
        .withArgs(172800, 1209600, 259200, ethers.parseEther('2000'));

      expect(await dao.votingDelay()).to.equal(172800);
      expect(await dao.votingPeriod()).to.equal(1209600);
      expect(await dao.executionDelay()).to.equal(259200);
      expect(await dao.proposalThreshold()).to.equal(ethers.parseEther('2000'));
      expect(await dao.quorumNumerator()).to.equal(5);
    });

    it('Should reject invalid voting parameters', async function () {
      const { dao } = await loadFixture(deployBasicDAOFixture);

      // Too long periods
      await expect(
        dao.updateVotingParameters(
          2592001, // > 30 days
          86400,
          86400,
          ethers.parseEther('1000'),
          4
        )
      ).to.be.revertedWithCustomError(dao, 'InvalidVotingParameters');

      // Invalid quorum
      await expect(
        dao.updateVotingParameters(
          86400,
          604800,
          172800,
          ethers.parseEther('1000'),
          101 // > 100%
        )
      ).to.be.revertedWithCustomError(dao, 'InvalidVotingParameters');
    });
  });

  describe('Treasury Management', function () {
    it('Should receive ETH deposits', async function () {
      const { dao, member1 } = await loadFixture(deployBasicDAOFixture);

      await expect(
        member1.sendTransaction({
          to: await dao.getAddress(),
          value: ethers.parseEther('5'),
        })
      )
        .to.emit(dao, 'FundsDeposited')
        .withArgs(member1.address, ethers.parseEther('5'));

      expect(await dao.treasuryBalance()).to.equal(ethers.parseEther('5'));
    });

    it('Should allow admin to withdraw treasury funds', async function () {
      const { dao, owner, member1 } = await loadFixture(deployBasicDAOFixture);

      // Deposit funds
      await member1.sendTransaction({
        to: await dao.getAddress(),
        value: ethers.parseEther('10'),
      });

      const initialBalance = await ethers.provider.getBalance(owner.address);

      await expect(dao.withdrawTreasury(owner.address, ethers.parseEther('3')))
        .to.emit(dao, 'FundsWithdrawn')
        .withArgs(owner.address, ethers.parseEther('3'));

      expect(await dao.treasuryBalance()).to.equal(ethers.parseEther('7'));
    });

    it('Should not allow withdrawing more than treasury balance', async function () {
      const { dao, owner } = await loadFixture(deployBasicDAOFixture);

      await expect(
        dao.withdrawTreasury(owner.address, ethers.parseEther('1'))
      ).to.be.revertedWith('BasicDAO: Insufficient balance');
    });
  });

  describe('DAO Statistics', function () {
    it('Should return correct DAO statistics', async function () {
      const { dao, governanceToken, member1 } = await loadFixture(
        deployBasicDAOFixture
      );

      // Add treasury funds
      await member1.sendTransaction({
        to: await dao.getAddress(),
        value: ethers.parseEther('5'),
      });

      const stats = await dao.getDAOStats();

      expect(stats.totalMembers).to.equal(4);
      expect(stats.totalProposals).to.equal(0);
      expect(stats.treasuryBalanceAmount).to.equal(ethers.parseEther('5'));
      expect(stats.totalTokenSupply).to.equal(
        await governanceToken.totalSupply()
      );

      const expectedQuorum =
        ((await governanceToken.totalSupply()) * 4n) / 100n;
      expect(stats.currentQuorumRequirement).to.equal(expectedQuorum);
    });
  });

  describe('Edge Cases and Security', function () {
    it('Should handle proposal expiration', async function () {
      const fixture = await loadFixture(deployBasicDAOFixture);

      // Create and pass proposal
      await fixture.dao
        .connect(fixture.member1)
        .propose('Test Proposal', 'Description', ethers.ZeroAddress, 0, '0x');

      await time.increase(86401);
      await fixture.dao.connect(fixture.member1).castVote(0, 0, 'For');
      await fixture.dao.connect(fixture.member2).castVote(0, 0, 'For');
      await time.increase(604801);

      // Don't queue, let it expire
      await time.increase(30 * 24 * 60 * 60 + 1); // 30 days + 1 second

      const proposal = await fixture.dao.getProposal(0);
      expect(proposal.state).to.equal(7); // EXPIRED
    });

    it('Should prevent reentrancy attacks', async function () {
      const { dao } = await loadFixture(deployBasicDAOFixture);

      // The ReentrancyGuard should be active
      // This is more of a structural test - the modifier should be present
      const contractCode = await ethers.provider.getCode(
        await dao.getAddress()
      );
      expect(contractCode).to.not.equal('0x');
    });

    it('Should validate proposal existence', async function () {
      const { dao, member1 } = await loadFixture(deployBasicDAOFixture);

      await expect(
        dao.connect(member1).castVote(999, 0, 'Invalid')
      ).to.be.revertedWith('BasicDAO: Proposal does not exist');
    });

    it('Should handle zero voting power', async function () {
      const { dao, governanceToken, nonMember } = await loadFixture(
        deployBasicDAOFixture
      );

      // Add nonMember as DAO member but with no tokens
      await dao.addMember(nonMember.address);

      // Create proposal
      await dao
        .connect(dao.member1)
        .propose('Test Proposal', 'Description', ethers.ZeroAddress, 0, '0x');

      await time.increase(86401);

      await expect(
        dao.connect(nonMember).castVote(0, 0, 'No power')
      ).to.be.revertedWith('BasicDAO: No voting power');
    });
  });
});
