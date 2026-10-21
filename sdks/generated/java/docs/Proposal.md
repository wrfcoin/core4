# Proposal

## Properties

| Name              | Type                          | Description                         | Notes      |
| ----------------- | ----------------------------- | ----------------------------------- | ---------- |
| **id**            | **String**                    | Proposal ID                         | [optional] |
| **title**         | **String**                    | Proposal title                      | [optional] |
| **description**   | **String**                    | Proposal description                | [optional] |
| **proposer**      | **String**                    | Proposer address                    | [optional] |
| **status**        | [**StatusEnum**](#StatusEnum) |                                     | [optional] |
| **votesFor**      | **String**                    | Total votes in favor                | [optional] |
| **votesAgainst**  | **String**                    | Total votes against                 | [optional] |
| **votesAbstain**  | **String**                    | Total abstain votes                 | [optional] |
| **startTime**     | **Integer**                   | Voting start timestamp              | [optional] |
| **endTime**       | **Integer**                   | Voting end timestamp                | [optional] |
| **executionTime** | **Integer**                   | Execution timestamp (if applicable) | [optional] |
| **quorum**        | **String**                    | Required quorum                     | [optional] |
| **threshold**     | **String**                    | Approval threshold                  | [optional] |

## Enum: StatusEnum

| Name     | Value                |
| -------- | -------------------- |
| ACTIVE   | &quot;active&quot;   |
| PENDING  | &quot;pending&quot;  |
| EXECUTED | &quot;executed&quot; |
| DEFEATED | &quot;defeated&quot; |
| EXPIRED  | &quot;expired&quot;  |
