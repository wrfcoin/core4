# ADR-[Number]: [Brief Decision Title]

**Status**: [Proposed | Accepted | Superseded by ADR-XXX | Deprecated] **Date**: [YYYY-MM-DD]
**Authors**: [List of decision makers] **Reviewers**: [List of reviewers]

<!--
STYLE GUIDE CHECKLIST:
- [ ] Status is clearly indicated
- [ ] Date format is YYYY-MM-DD
- [ ] Context explains the problem clearly
- [ ] Decision is stated unambiguously
- [ ] Rationale explains reasoning
- [ ] Consequences cover positive, negative, and neutral impacts
- [ ] Alternatives considered section is complete
- [ ] Implementation details are specific
-->

## Summary

[One paragraph summary of the decision and its impact]

## Context

### Problem Statement

[Describe the problem that needs to be solved. What forces are at play? What are the constraints?]

### Background

[Provide context about the current state, stakeholders involved, and any relevant history]

### Requirements

[List the key requirements that the solution must satisfy]

- Functional requirement 1
- Functional requirement 2
- Non-functional requirement 1 (performance, security, etc.)
- Non-functional requirement 2

### Constraints

[List any limitations or constraints that affect the decision]

- Technical constraints
- Business constraints
- Timeline constraints
- Resource constraints

## Decision

[State the decision clearly and concisely. What exactly are we doing?]

### Key Components

- Component/aspect 1
- Component/aspect 2
- Component/aspect 3

### Implementation Approach

[High-level description of how this will be implemented]

## Rationale

### Why This Decision

[Explain the reasoning behind this specific choice. What makes this the best option?]

### Alignment with Goals

[How does this decision support broader architectural and business goals?]

### Risk Assessment

[What risks does this decision mitigate? What new risks does it introduce?]

## Consequences

### Positive Consequences

- ✅ Benefit 1: [Specific improvement]
- ✅ Benefit 2: [Specific improvement]
- ✅ Benefit 3: [Specific improvement]

### Negative Consequences

- ❌ Drawback 1: [Specific cost or limitation]
- ❌ Drawback 2: [Specific cost or limitation]
- ❌ Drawback 3: [Specific cost or limitation]

### Neutral Consequences

- ℹ️ Change 1: [Neither positive nor negative impact]
- ℹ️ Change 2: [Neither positive nor negative impact]

### Impact Assessment

- **Performance**: [Impact on system performance]
- **Security**: [Security implications]
- **Maintainability**: [Effect on code maintenance]
- **Scalability**: [Scaling considerations]
- **Developer Experience**: [Impact on development workflow]
- **User Experience**: [Impact on end users]

## Alternatives Considered

### Alternative 1: [Name]

**Description**: [Brief description of the alternative] **Pros**:

- Pro 1
- Pro 2 **Cons**:
- Con 1
- Con 2 **Why Rejected**: [Specific reason this wasn't chosen]

### Alternative 2: [Name]

**Description**: [Brief description of the alternative] **Pros**:

- Pro 1
- Pro 2 **Cons**:
- Con 1
- Con 2 **Why Rejected**: [Specific reason this wasn't chosen]

### Alternative 3: Status Quo

**Description**: [What would happen if we made no change] **Pros**:

- No implementation cost
- No risk of new issues **Cons**:
- Problems remain unsolved
- Technical debt accumulates **Why Rejected**: [Why status quo is insufficient]

## Implementation Plan

### Phase 1: [Phase Name]

- [ ] Task 1
- [ ] Task 2
- [ ] Task 3 **Timeline**: [Estimated duration] **Dependencies**: [Any dependencies]

### Phase 2: [Phase Name]

- [ ] Task 1
- [ ] Task 2
- [ ] Task 3 **Timeline**: [Estimated duration] **Dependencies**: [Any dependencies]

### Success Criteria

[How will we know if this decision was successful?]

- Measurable outcome 1
- Measurable outcome 2
- Measurable outcome 3

### Rollback Plan

[If this decision proves problematic, how can we reverse it?]

## Monitoring and Review

### Metrics to Track

- Metric 1: [What to measure and why]
- Metric 2: [What to measure and why]
- Metric 3: [What to measure and why]

### Review Schedule

- **30 days**: Initial assessment of implementation
- **90 days**: Full impact evaluation
- **6 months**: Long-term effects review

### Review Criteria

[What would trigger a reconsideration of this decision?]

## Related Decisions

### Prerequisites

- ADR-XXX: [Related decision that this builds on]
- RFC-XXX: [Related proposal]

### Follow-up Decisions

- ADR-XXX: [Decision that may supersede this one]
- [Future decisions that may be needed]

### External References

- [Link to relevant documentation]
- [Link to external standards or best practices]
- [Link to vendor documentation]

## Communication Plan

### Stakeholders to Notify

- Development team
- Operations team
- Product management
- External partners (if applicable)

### Communication Timeline

- **Before Implementation**: [When and how to announce]
- **During Implementation**: [Progress updates]
- **After Implementation**: [Results communication]

---

**Approval Signatures:**

- [ ] Technical Lead: [Name] - [Date]
- [ ] Architecture Review Board: [Name] - [Date]
- [ ] Product Owner: [Name] - [Date]

**Implementation Tracking:**

- Implementation Issue: [Link to tracking issue]
- Documentation Updates: [Link to related docs]
- Code Changes: [Link to relevant PRs]
