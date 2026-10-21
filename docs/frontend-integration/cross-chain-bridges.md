# Cross-chain frontend integration: NOT_AVAILABLE

The public WRFCoin release does not provide bridge discovery, transfer, status,
fee, or history APIs. Frontends must render this capability as
`NOT_AVAILABLE` and must not offer a transfer action.

The release node's `/cross_chain` status route fails closed. Authenticated
`/bridge/safety/*` operator routes are safety controls for dormant state, not a
public bridge API.
