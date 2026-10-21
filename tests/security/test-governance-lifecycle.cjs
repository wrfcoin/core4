const axios = require('axios');

/**
 * TDD-6.1: DAO Proposal & Stake-Weighted Voting
 * Tests:
 * 1. Seed proposer & voter accounts with governance stake.
 * 2. Submit proposal to change emission_rate.
 * 3. Mine one block so the proposal is on-chain.
 * 4. Cast a yes-vote (in a SEPARATE block to avoid ordering ambiguity).
 * 5. Mine blocks past the voting deadline.
 * 6. Verify proposal appears in listing with Executed status.
 */

const API_BASE = 'http://localhost:8556';
const OPERATOR_TOKEN = process.env.WRFCOIN_OPERATOR_TOKEN;

async function runTest() {
  console.log('🚀 Starting TDD-6.1 Governance Lifecycle Test');

  try {
    const config = { headers: { Authorization: `Bearer ${OPERATOR_TOKEN}` } };

    // 0. Seed accounts (direct state, NOT via mempool)
    const proposerAddress = '0x' + 'a'.repeat(40);
    const voterAddress    = '0x' + 'b'.repeat(40);
    const minerAddress    = '0x' + 'f'.repeat(40);
    console.log(`Step 0: Seeding proposer (${proposerAddress}), voter, miner`);
    await Promise.all([
      axios.post(`${API_BASE}/debug/seed-account`, { address: proposerAddress, amount: 1000000, stake: 500000 }, config),
      axios.post(`${API_BASE}/debug/seed-account`, { address: voterAddress,    amount: 500000,  stake: 400000 }, config),
      axios.post(`${API_BASE}/debug/seed-account`, { address: minerAddress,    amount: 1000 }, config),
    ]);

    // 1. Submit Proposal
    const proposalId = `prop-${Date.now()}`;
    // Use a high deadline so we can control finalization timing.
    const deadline = 10;
    console.log(`Step 1: Submitting proposal ${proposalId} (deadline: block ${deadline})`);

    const propResp = await axios.post(`${API_BASE}/governance/proposals/submit`, {
      proposal_id: proposalId,
      parameter: 'emission_rate',
      value: '150',
      voting_deadline: deadline,
      deposit: 1000,
    }, {
      headers: {
        ...config.headers,
        'X-Proposer-Address': proposerAddress,
      },
    });
    const returnedTxId = propResp.data.id;
    console.log(`Proposal tx accepted (tx ${returnedTxId})`);

    // 2. Mine one block to commit the proposal BEFORE submitting the vote.
    //    This avoids same-block ordering ambiguity.
    let mineResp = await axios.post(`${API_BASE}/blocks/mine`, { miner_address: minerAddress }, config);
    let data = mineResp.data.data || mineResp.data;
    console.log(`Step 2: Mined block ${data.height} (${data.transaction_count} txs) — proposal should be on-chain`);

    // 3. Cast Vote — proposal is now on-chain, vote goes into next block.
    console.log('Step 3: Casting yes-vote');
    await axios.post(`${API_BASE}/governance/vote`, {
      proposal_id: proposalId,
      approve: true,
      // Omit voting_power — the chain derives it from the voter's stake snapshot.
    }, {
      headers: {
        ...config.headers,
        'X-Voter-Address': voterAddress,
      },
    });
    console.log('Vote submitted to mempool');

    // 4. Mine blocks past deadline
    console.log(`Step 4: Mining blocks to pass deadline (${deadline})`);
    for (let i = 0; i < deadline; i++) {
      mineResp = await axios.post(`${API_BASE}/blocks/mine`, { miner_address: minerAddress }, config);
      data = mineResp.data.data || mineResp.data;
      console.log(`  Block ${data.height} (${data.transaction_count} txs)`);
    }

    // 5. Verify proposal appears in listing
    console.log('Step 5: Verifying proposal in governance list');
    const listResp = await axios.get(`${API_BASE}/governance/proposals`, config);
    const proposals = listResp.data.data || listResp.data;
    const myProp = proposals.find(p =>
      p.id === proposalId || p.proposal_id === proposalId ||
      p.id === returnedTxId || p.proposal_id === returnedTxId
    );

    if (!myProp) {
      console.error(`❌ Proposal not found (searched id=${proposalId}, tx=${returnedTxId})`);
      console.error(`   Available: ${JSON.stringify(proposals.map(p => p.id))}`);
      process.exit(1);
    }

    const yesVotes = myProp.yes_votes || 0;
    console.log(`Proposal found: status=${myProp.status}, yes_votes=${yesVotes}, finalized_at=${myProp.finalized_at_block}`);
    if (!(myProp.status === 'Executed' || myProp.status === 'Passed' || myProp.status === 'Finalized' || myProp.executed)) {
      console.error(`❌ Proposal status is "${myProp.status}", expected Executed/Passed/Finalized`);
      console.error(`   Eligible: ${myProp.eligible_voting_power}, quorum: ${myProp.quorum_percent}%`);
      console.error(`   Votes: yes=${myProp.yes_votes} no=${myProp.no_votes}`);
      process.exit(1);
    }

    console.log('✅ TDD-6.1 PASSED: proposal submitted, vote cast, blocks mined, proposal executed');

  } catch (error) {
    console.error('❌ Test Failed:', error.response ? error.response.data : error.message);
    process.exit(1);
  }
}

runTest();
