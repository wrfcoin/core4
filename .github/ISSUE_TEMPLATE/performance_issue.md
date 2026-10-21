---
name: ⚡ Performance Issue
about: Report performance problems or optimization opportunities
title: '[PERFORMANCE] Brief description of the performance issue'
labels: ['performance', 'needs-investigation']
assignees: ''
---

## ⚡ Performance Issue Summary

A clear and concise description of the performance problem.

## 📊 Performance Metrics

**Current Performance:**

- Response time: [e.g., 2.5 seconds]
- Throughput: [e.g., 500 TPS]
- Memory usage: [e.g., 2GB]
- CPU usage: [e.g., 80%]
- Database query time: [e.g., 500ms]

**Expected Performance:**

- Response time: [e.g., < 200ms]
- Throughput: [e.g., > 1000 TPS]
- Memory usage: [e.g., < 1GB]
- CPU usage: [e.g., < 50%]
- Database query time: [e.g., < 100ms]

## 🎯 Affected Components

- [ ] Frontend (React)
- [ ] Main API (port 4001)
- [ ] Cross-Chain Bridge (port 3000)
- [ ] Wallet Service (port 4000)
- [ ] IoT Networks (port 3002)
- [ ] Compute Networks (port 6000)
- [ ] Storage Networks (port 7000)
- [ ] Governance (port 8000)
- [ ] Blockchain Core (Rust)
- [ ] Smart Contracts
- [ ] Database (PostgreSQL)
- [ ] Cache (Redis)

## 🔄 Steps to Reproduce

1. Set up environment with [specific configuration]
2. Execute [specific operation]
3. Measure performance using [specific tools]
4. Observe degraded performance

## 🌍 Environment Information

**System Configuration:**

- OS: [e.g., Ubuntu 22.04]
- CPU: [e.g., 8 cores, 3.2GHz]
- RAM: [e.g., 16GB]
- Storage: [e.g., SSD, 500GB]
- Network: [e.g., 1Gbps]

**Software Versions:**

- Node.js: [e.g., 20.1.0]
- Rust: [e.g., 1.70.0]
- PostgreSQL: [e.g., 15.2]
- Redis: [e.g., 7.0]
- Docker: [e.g., 24.0.0]

## 📈 Load Conditions

**Traffic/Load:**

- Concurrent users: [e.g., 100]
- Requests per second: [e.g., 500]
- Data volume: [e.g., 1TB]
- Transaction volume: [e.g., 1000 TPS]

**Test Duration:**

- [ ] Issue appears immediately
- [ ] Issue appears after [X] minutes/hours
- [ ] Issue appears under sustained load
- [ ] Issue appears randomly

## 🔍 Monitoring Data

**Performance Logs:**

```
Paste relevant performance logs, metrics, or profiling data here
```

**Error Messages (if any):**

```
Paste any error messages related to performance here
```

## 🧪 Testing Results

**Performance Testing Tools Used:**

- [ ] Artillery (load testing)
- [ ] Lighthouse (frontend performance)
- [ ] Node.js profiler
- [ ] Rust profiler (cargo flamegraph)
- [ ] Database performance tools
- [ ] Custom benchmarks

**Benchmark Results:**

```
Paste benchmark results here
```

## 💡 Analysis & Observations

**Suspected Bottlenecks:**

- [ ] CPU-bound operations
- [ ] Memory leaks or excessive usage
- [ ] Database query optimization
- [ ] Network latency/bandwidth
- [ ] Inefficient algorithms
- [ ] Blocking I/O operations
- [ ] Excessive API calls
- [ ] Large data processing
- [ ] Frontend rendering issues

**Additional Observations:** [Describe any patterns, trends, or additional observations]

## 🎯 Business Impact

- **User Experience**: [How does this affect users?]
- **System Capacity**: [How does this limit system capacity?]
- **Cost Impact**: [Does this increase operational costs?]
- **Climate Mission**: [Does this affect climate data processing?]

## 🔧 Proposed Solutions

If you have ideas for optimization:

- [ ] Algorithm optimization
- [ ] Database indexing/query optimization
- [ ] Caching implementation
- [ ] Code refactoring
- [ ] Infrastructure scaling
- [ ] Microservice decomposition
- [ ] Frontend optimization
- [ ] Memory management improvements

**Detailed Proposal:** [Describe your proposed solution]

## 📊 Success Criteria

How will we know the performance issue is resolved?

- Response time improved to: [target]
- Throughput increased to: [target]
- Memory usage reduced to: [target]
- CPU usage reduced to: [target]

## 🚧 Workarounds

Are there any temporary workarounds?

- [ ] Scaling resources
- [ ] Rate limiting
- [ ] Caching
- [ ] Load balancing
- [ ] Feature flags to disable functionality

## 🔄 Regression Information

- [ ] This is a new performance regression
- [ ] This has always been an issue
- [ ] Performance degraded after [specific change/version]
- [ ] Unknown when performance degraded

**Last Known Good Performance:**

- Version: [e.g., v1.2.3]
- Date: [e.g., 2025-01-15]
- Metrics: [previous performance numbers]

## ✅ Checklist

- [ ] I have measured actual performance metrics
- [ ] I have provided environment information
- [ ] I have identified the affected components
- [ ] I have considered the business impact
- [ ] I have searched for existing performance issues
