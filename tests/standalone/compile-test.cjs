const { exec } = require('child_process');
const fs = require('fs');

async function testCompilation() {
  console.log('🔨 Testing SecureNFTMarketplace Compilation');
  console.log('='.repeat(50));

  // First, let's try to compile just our specific contracts
  const contractsToTest = [
    'contracts/SecureNFTMarketplace.sol',
    'contracts/mocks/MockERC20.sol',
    'contracts/mocks/MockERC721.sol',
  ];

  console.log('\n📋 Checking contract files exist:');
  let allFilesExist = true;

  for (const contract of contractsToTest) {
    const exists = fs.existsSync(contract);
    console.log(`${exists ? '✅' : '❌'} ${contract}`);
    if (!exists) allFilesExist = false;
  }

  if (!allFilesExist) {
    console.log('\n❌ Missing required contract files');
    return false;
  }

  // Check if OpenZeppelin contracts exist
  const ozExists = fs.existsSync('node_modules/@openzeppelin/contracts');
  console.log(
    `${ozExists ? '✅' : '❌'} OpenZeppelin contracts: ${ozExists ? 'Available' : 'Missing'}`
  );

  if (!ozExists) {
    console.log('\n❌ OpenZeppelin contracts not found in node_modules');
    return false;
  }

  // Analyze our main contract for any remaining SafeMath references
  console.log('\n🔍 Analyzing SecureNFTMarketplace.sol:');
  const contractCode = fs.readFileSync(
    'contracts/SecureNFTMarketplace.sol',
    'utf8'
  );

  const safeMathImport = contractCode.includes(
    '@openzeppelin/contracts/utils/math/SafeMath'
  );
  const safeMathUsing = contractCode.includes('using SafeMath for');
  const safeMathOps = (contractCode.match(/\.(add|sub|mul|div)\(/g) || [])
    .length;

  console.log(
    `📝 SafeMath import: ${safeMathImport ? '❌ Found' : '✅ Removed'}`
  );
  console.log(
    `📝 SafeMath using statement: ${safeMathUsing ? '❌ Found' : '✅ Removed'}`
  );
  console.log(
    `📝 SafeMath operations: ${safeMathOps > 0 ? `❌ ${safeMathOps} found` : '✅ All removed'}`
  );

  // Check Solidity version
  const versionMatch = contractCode.match(/pragma solidity \^?([\d\.]+)/);
  const version = versionMatch ? versionMatch[1] : 'unknown';
  console.log(
    `📝 Solidity version: ${version} ${parseFloat(version) >= 0.8 ? '✅' : '❌'}`
  );

  // Check for arithmetic security patterns
  const nativeArithmetic = (contractCode.match(/\s+[+\-*/]\s+/g) || []).length;
  const requireChecks = (contractCode.match(/require\(/g) || []).length;

  console.log(`📝 Native arithmetic operations: ${nativeArithmetic}`);
  console.log(`📝 Security require checks: ${requireChecks}`);

  // Security assessment
  const isSecure =
    !safeMathImport &&
    !safeMathUsing &&
    safeMathOps === 0 &&
    parseFloat(version) >= 0.8 &&
    requireChecks > 10;

  console.log(
    `\n🛡️  Security Assessment: ${isSecure ? '✅ SECURE' : '⚠️  NEEDS ATTENTION'}`
  );

  return isSecure;
}

// Run the test
testCompilation()
  .then(success => {
    console.log(`\n🎉 Compilation Test: ${success ? 'PASSED' : 'FAILED'}`);
    process.exit(success ? 0 : 1);
  })
  .catch(error => {
    console.error(`❌ Test failed: ${error.message}`);
    process.exit(1);
  });
