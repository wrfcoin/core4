// Regression fixture: this must be rejected by check-cross-chain-release-surface.mjs.
export class CrossChainBridgeApi {
  async transfer() {
    return { transactionHash: `0x${Math.random().toString(16).slice(2)}` };
  }
}
