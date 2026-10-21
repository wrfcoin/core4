# Exact node build stanza from wrfcoin/infra at 750298cdf20c,
# infra/docker/Dockerfile.core:43-45. Documentation fixture only.
# This is not a complete Dockerfile and is never executed by the checker.
RUN cargo build --release -p wrfcoin-native-chain \
      --bin launch_api \
      --features "full-mining,p2p,hsm,upgrade-notice-fetch"
