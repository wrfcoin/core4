// Simulate a sandwiching attack transaction
const transaction = {
  id: '0x7abc1234def5678901234567890123456789012345678901234567890124abcd',
  from: '0x742d35Cc6634C0532925a3b844Bc454e4438f44e',
  to: '0x5E32E7D86C598139D2Fb11841860C3840008C9DC',
  amount: 0.5,
  fee: 0.01,
  timestamp: Date.now(),
};

// Port used by our High Throughput Node
const API_PORT = 5000;

console.log(
  `Submitting test transaction to High Throughput Node on port ${API_PORT}...`
);

// Send the transaction to the High Throughput Node
fetch(`http://localhost:${API_PORT}/transactions`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify(transaction),
})
  .then(response => {
    if (!response.ok) {
      throw new Error(`HTTP error! Status: ${response.status}`);
    }
    return response.json();
  })
  .then(data => {
    console.log('Transaction submitted:', data);

    // Now simulate a suspected MEV attack detection
    const mevSuspect = {
      transaction_id: transaction.id,
      mev_type: 'SANDWICH',
      confidence: 85,
      related_transactions: [
        '0x6def1234abc5678901234567890123456789012345678901234567890124efab',
        '0x5cde1234bcd5678901234567890123456789012345678901234567890124fedc',
      ],
      detected_at: new Date().toISOString(),
      evidence:
        'High gas price transaction detected between similar transactions targeting the same liquidity pool',
    };

    console.log('Submitting MEV suspect report...');

    // Add this as a test entry in MEV protection dashboard
    return fetch(`http://localhost:${API_PORT}/api/mev/report`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(mevSuspect),
    });
  })
  .then(response => {
    if (!response.ok) {
      throw new Error(`HTTP error! Status: ${response.status}`);
    }
    return response.json();
  })
  .then(data => {
    console.log('MEV suspect report submitted:', data);
  })
  .catch(error => console.error('Error:', error));
