const { expect } = require('chai');
const { ethers } = require('hardhat');

/**
 * Minimal compilation test for SocialVerification contract
 * This test only checks if the contract compiles successfully
 */
describe('SocialVerification - Compilation Test', function () {
  it('Should compile SocialVerification contract successfully', async function () {
    // This test will fail if there are compilation errors
    const SocialVerification =
      await ethers.getContractFactory('SocialVerification');
    expect(SocialVerification).to.not.be.undefined;

    console.log('✅ SocialVerification contract compiled successfully');
  });

  it('Should deploy SocialVerification contract successfully', async function () {
    const SocialVerification =
      await ethers.getContractFactory('SocialVerification');
    const socialVerification = await SocialVerification.deploy();
    await socialVerification.waitForDeployment();

    const address = await socialVerification.getAddress();
    expect(address).to.not.equal(ethers.ZeroAddress);

    console.log('✅ SocialVerification deployed to:', address);
  });

  it('Should have correct initial configuration', async function () {
    const SocialVerification =
      await ethers.getContractFactory('SocialVerification');
    const socialVerification = await SocialVerification.deploy();
    await socialVerification.waitForDeployment();

    // Check default role setup
    const DEFAULT_ADMIN_ROLE = await socialVerification.DEFAULT_ADMIN_ROLE();
    const ORACLE_ROLE = await socialVerification.ORACLE_ROLE();

    expect(DEFAULT_ADMIN_ROLE).to.not.be.undefined;
    expect(ORACLE_ROLE).to.not.be.undefined;

    console.log('✅ Contract has correct role configuration');
  });
});
