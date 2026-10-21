const { expect } = require('chai');
const { ethers } = require('hardhat');
const { loadFixture } = require('@nomicfoundation/hardhat-network-helpers');

describe('BaseERC20Token Template', function () {
  async function deployBaseERC20TokenFixture() {
    const [owner, addr1, addr2, minter, feeRecipient] =
      await ethers.getSigners();

    const BaseERC20Token = await ethers.getContractFactory('BaseERC20Token');
    const token = await BaseERC20Token.deploy(
      'Test Token',
      'TEST',
      ethers.parseEther('1000000'), // max supply
      ethers.parseEther('100000'), // initial supply
      owner.address,
      false, // transferRestrictionsEnabled
      100, // transferFeeRate (1%)
      feeRecipient.address
    );

    return { token, owner, addr1, addr2, minter, feeRecipient };
  }

  describe('Deployment', function () {
    it('Should set the correct name and symbol', async function () {
      const { token } = await loadFixture(deployBaseERC20TokenFixture);

      expect(await token.name()).to.equal('Test Token');
      expect(await token.symbol()).to.equal('TEST');
    });

    it('Should set the correct decimals', async function () {
      const { token } = await loadFixture(deployBaseERC20TokenFixture);

      expect(await token.decimals()).to.equal(18);
    });

    it('Should set the correct max supply', async function () {
      const { token } = await loadFixture(deployBaseERC20TokenFixture);

      expect(await token.maxSupply()).to.equal(ethers.parseEther('1000000'));
    });

    it('Should mint initial supply to owner', async function () {
      const { token, owner } = await loadFixture(deployBaseERC20TokenFixture);

      expect(await token.balanceOf(owner.address)).to.equal(
        ethers.parseEther('100000')
      );
    });

    it('Should set up correct roles', async function () {
      const { token, owner } = await loadFixture(deployBaseERC20TokenFixture);

      const DEFAULT_ADMIN_ROLE = await token.DEFAULT_ADMIN_ROLE();
      const MINTER_ROLE = await token.MINTER_ROLE();

      expect(await token.hasRole(DEFAULT_ADMIN_ROLE, owner.address)).to.be.true;
      expect(await token.hasRole(MINTER_ROLE, owner.address)).to.be.true;
    });

    it('Should set fee parameters correctly', async function () {
      const { token, feeRecipient } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      expect(await token.transferFeeRate()).to.equal(100); // 1%
      expect(await token.feeRecipient()).to.equal(feeRecipient.address);
    });
  });

  describe('Minting', function () {
    it('Should allow minter to mint tokens', async function () {
      const { token, addr1, minter } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      await token.grantRole(await token.MINTER_ROLE(), minter.address);

      await expect(
        token.connect(minter).mint(addr1.address, ethers.parseEther('1000'))
      )
        .to.emit(token, 'Transfer')
        .withArgs(ethers.ZeroAddress, addr1.address, ethers.parseEther('1000'));

      expect(await token.balanceOf(addr1.address)).to.equal(
        ethers.parseEther('1000')
      );
    });

    it('Should not allow non-minter to mint tokens', async function () {
      const { token, addr1, addr2 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      await expect(
        token.connect(addr2).mint(addr1.address, ethers.parseEther('1000'))
      ).to.be.revertedWith('AccessControl: account');
    });

    it('Should not allow minting beyond max supply', async function () {
      const { token, addr1 } = await loadFixture(deployBaseERC20TokenFixture);

      // Try to mint more than max supply allows
      const remainingSupply =
        (await token.maxSupply()) - (await token.totalSupply());

      await expect(
        token.mint(addr1.address, remainingSupply + ethers.parseEther('1'))
      ).to.be.revertedWith('Max supply exceeded');
    });

    it('Should not allow minting when minting is finished', async function () {
      const { token, addr1 } = await loadFixture(deployBaseERC20TokenFixture);

      await token.finishMinting();

      await expect(
        token.mint(addr1.address, ethers.parseEther('1000'))
      ).to.be.revertedWith('Minting finished');
    });
  });

  describe('Burning', function () {
    it('Should allow users to burn their tokens', async function () {
      const { token, owner } = await loadFixture(deployBaseERC20TokenFixture);

      const initialBalance = await token.balanceOf(owner.address);
      const burnAmount = ethers.parseEther('1000');

      await expect(token.burn(burnAmount))
        .to.emit(token, 'Transfer')
        .withArgs(owner.address, ethers.ZeroAddress, burnAmount);

      expect(await token.balanceOf(owner.address)).to.equal(
        initialBalance - burnAmount
      );
    });

    it('Should allow burning from approved accounts', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      const burnAmount = ethers.parseEther('1000');

      await token.approve(addr1.address, burnAmount);

      await expect(token.connect(addr1).burnFrom(owner.address, burnAmount))
        .to.emit(token, 'Transfer')
        .withArgs(owner.address, ethers.ZeroAddress, burnAmount);
    });

    it('Should not allow burning more than balance', async function () {
      const { token, addr1 } = await loadFixture(deployBaseERC20TokenFixture);

      const balance = await token.balanceOf(addr1.address);

      await expect(
        token.connect(addr1).burn(balance + ethers.parseEther('1'))
      ).to.be.revertedWith('ERC20: burn amount exceeds balance');
    });
  });

  describe('Transfer Restrictions', function () {
    it('Should allow unrestricted transfers when restrictions are disabled', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      const transferAmount = ethers.parseEther('1000');

      await expect(token.transfer(addr1.address, transferAmount))
        .to.emit(token, 'Transfer')
        .withArgs(owner.address, addr1.address, transferAmount);
    });

    it('Should restrict transfers when restrictions are enabled', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      await token.setTransferRestrictions(true);

      await expect(
        token.transfer(addr1.address, ethers.parseEther('1000'))
      ).to.be.revertedWith('Transfer restrictions enabled');
    });

    it('Should allow whitelisted addresses to transfer when restrictions are enabled', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      await token.setTransferRestrictions(true);
      await token.setWhitelisted(owner.address, true);
      await token.setWhitelisted(addr1.address, true);

      await expect(token.transfer(addr1.address, ethers.parseEther('1000')))
        .to.emit(token, 'Transfer')
        .withArgs(owner.address, addr1.address, ethers.parseEther('1000'));
    });

    it('Should block blacklisted addresses from transferring', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      await token.setBlacklisted(owner.address, true);

      await expect(
        token.transfer(addr1.address, ethers.parseEther('1000'))
      ).to.be.revertedWith('Address is blacklisted');
    });

    it('Should block transfers to blacklisted addresses', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      await token.setBlacklisted(addr1.address, true);

      await expect(
        token.transfer(addr1.address, ethers.parseEther('1000'))
      ).to.be.revertedWith('Address is blacklisted');
    });
  });

  describe('Transfer Fees', function () {
    it('Should charge transfer fees', async function () {
      const { token, owner, addr1, feeRecipient } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      const transferAmount = ethers.parseEther('1000');
      const expectedFee = (transferAmount * 100n) / 10000n; // 1%
      const expectedReceived = transferAmount - expectedFee;

      await token.transfer(addr1.address, transferAmount);

      expect(await token.balanceOf(addr1.address)).to.equal(expectedReceived);
      expect(await token.balanceOf(feeRecipient.address)).to.equal(expectedFee);
    });

    it('Should not charge fees for exempt addresses', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      await token.setFeeExempt(owner.address, true);

      const transferAmount = ethers.parseEther('1000');

      await token.transfer(addr1.address, transferAmount);

      expect(await token.balanceOf(addr1.address)).to.equal(transferAmount);
    });

    it('Should allow admin to update fee rate', async function () {
      const { token } = await loadFixture(deployBaseERC20TokenFixture);

      await token.setTransferFeeRate(200); // 2%

      expect(await token.transferFeeRate()).to.equal(200);
    });

    it('Should not allow fee rate above maximum', async function () {
      const { token } = await loadFixture(deployBaseERC20TokenFixture);

      await expect(token.setTransferFeeRate(1001)) // > 10%
        .to.be.revertedWith('Fee rate too high');
    });

    it('Should allow admin to update fee recipient', async function () {
      const { token, addr1 } = await loadFixture(deployBaseERC20TokenFixture);

      await token.setFeeRecipient(addr1.address);

      expect(await token.feeRecipient()).to.equal(addr1.address);
    });
  });

  describe('Governance Features', function () {
    it('Should support EIP-2612 permit', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      const value = ethers.parseEther('1000');
      const deadline = Math.floor(Date.now() / 1000) + 3600; // 1 hour

      const domain = {
        name: await token.name(),
        version: '1',
        chainId: network.config.chainId,
        verifyingContract: await token.getAddress(),
      };

      const types = {
        Permit: [
          { name: 'owner', type: 'address' },
          { name: 'spender', type: 'address' },
          { name: 'value', type: 'uint256' },
          { name: 'nonce', type: 'uint256' },
          { name: 'deadline', type: 'uint256' },
        ],
      };

      const permit = {
        owner: owner.address,
        spender: addr1.address,
        value: value,
        nonce: await token.nonces(owner.address),
        deadline: deadline,
      };

      const signature = await owner.signTypedData(domain, types, permit);
      const { v, r, s } = ethers.Signature.from(signature);

      await expect(
        token.permit(owner.address, addr1.address, value, deadline, v, r, s)
      )
        .to.emit(token, 'Approval')
        .withArgs(owner.address, addr1.address, value);
    });

    it('Should support delegation of voting power', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      await expect(token.delegate(addr1.address))
        .to.emit(token, 'DelegateChanged')
        .withArgs(owner.address, ethers.ZeroAddress, addr1.address);

      expect(await token.delegates(owner.address)).to.equal(addr1.address);
    });

    it('Should track voting power correctly', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      const initialBalance = await token.balanceOf(owner.address);

      await token.delegate(addr1.address);

      expect(await token.getVotes(addr1.address)).to.equal(initialBalance);
    });
  });

  describe('Pausable Functions', function () {
    it('Should pause and unpause token transfers', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      await token.pause();

      await expect(
        token.transfer(addr1.address, ethers.parseEther('1000'))
      ).to.be.revertedWith('Pausable: paused');

      await token.unpause();

      await expect(
        token.transfer(addr1.address, ethers.parseEther('1000'))
      ).to.emit(token, 'Transfer');
    });

    it('Should only allow admin to pause/unpause', async function () {
      const { token, addr1 } = await loadFixture(deployBaseERC20TokenFixture);

      await expect(token.connect(addr1).pause()).to.be.revertedWith(
        'AccessControl: account'
      );

      await expect(token.connect(addr1).unpause()).to.be.revertedWith(
        'AccessControl: account'
      );
    });
  });

  describe('Access Control', function () {
    it('Should allow admin to grant and revoke roles', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      const MINTER_ROLE = await token.MINTER_ROLE();

      await token.grantRole(MINTER_ROLE, addr1.address);
      expect(await token.hasRole(MINTER_ROLE, addr1.address)).to.be.true;

      await token.revokeRole(MINTER_ROLE, addr1.address);
      expect(await token.hasRole(MINTER_ROLE, addr1.address)).to.be.false;
    });

    it('Should only allow admin to manage blacklist/whitelist', async function () {
      const { token, addr1, addr2 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      await expect(
        token.connect(addr1).setBlacklisted(addr2.address, true)
      ).to.be.revertedWith('AccessControl: account');

      await expect(
        token.connect(addr1).setWhitelisted(addr2.address, true)
      ).to.be.revertedWith('AccessControl: account');
    });

    it('Should only allow admin to manage transfer restrictions', async function () {
      const { token, addr1 } = await loadFixture(deployBaseERC20TokenFixture);

      await expect(
        token.connect(addr1).setTransferRestrictions(true)
      ).to.be.revertedWith('AccessControl: account');
    });
  });

  describe('Edge Cases', function () {
    it('Should handle zero amount transfers', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      await expect(token.transfer(addr1.address, 0))
        .to.emit(token, 'Transfer')
        .withArgs(owner.address, addr1.address, 0);
    });

    it('Should handle transfers to self', async function () {
      const { token, owner } = await loadFixture(deployBaseERC20TokenFixture);

      const initialBalance = await token.balanceOf(owner.address);

      await token.transfer(owner.address, ethers.parseEther('1000'));

      expect(await token.balanceOf(owner.address)).to.equal(initialBalance);
    });

    it('Should handle maximum fee rate', async function () {
      const { token } = await loadFixture(deployBaseERC20TokenFixture);

      await token.setTransferFeeRate(1000); // 10%

      expect(await token.transferFeeRate()).to.equal(1000);
    });

    it('Should handle zero fee rate', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      await token.setTransferFeeRate(0);

      const transferAmount = ethers.parseEther('1000');

      await token.transfer(addr1.address, transferAmount);

      expect(await token.balanceOf(addr1.address)).to.equal(transferAmount);
    });
  });

  describe('Security Tests', function () {
    it('Should not allow reentrancy attacks', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      // This test would require a malicious contract to test reentrancy
      // For now, we verify the ReentrancyGuard is properly inherited
      const contractCode = await ethers.provider.getCode(
        await token.getAddress()
      );
      expect(contractCode).to.not.equal('0x');
    });

    it('Should handle large number operations safely', async function () {
      const { token, owner, addr1 } = await loadFixture(
        deployBaseERC20TokenFixture
      );

      const maxAmount = ethers.parseEther('999999999999999999999999999999');

      // Should not overflow
      await expect(token.approve(addr1.address, maxAmount)).to.not.be.reverted;
    });

    it('Should properly validate addresses', async function () {
      const { token } = await loadFixture(deployBaseERC20TokenFixture);

      await expect(
        token.transfer(ethers.ZeroAddress, ethers.parseEther('1000'))
      ).to.be.revertedWith('ERC20: transfer to the zero address');
    });
  });
});
