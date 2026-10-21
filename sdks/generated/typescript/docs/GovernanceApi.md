# GovernanceApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                    | HTTP request                                         | Description               |
| ----------------------------------------------------------------------------------------- | ---------------------------------------------------- | ------------------------- |
| [**apiGovernanceProposalsGet**](#apigovernanceproposalsget)                               | **GET** /api/governance/proposals                    | List governance proposals |
| [**apiGovernanceProposalsPost**](#apigovernanceproposalspost)                             | **POST** /api/governance/proposals                   | Create new proposal       |
| [**apiGovernanceProposalsProposalIdVotePost**](#apigovernanceproposalsproposalidvotepost) | **POST** /api/governance/proposals/{proposalId}/vote | Vote on proposal          |

# **apiGovernanceProposalsGet**

> Array<Proposal> apiGovernanceProposalsGet()

### Example

```typescript
import { GovernanceApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new GovernanceApi(configuration);

let status: 'active' | 'pending' | 'executed' | 'defeated'; // (optional) (default to undefined)
let limit: number; // (optional) (default to 20)

const { status, data } = await apiInstance.apiGovernanceProposalsGet(status, limit);
```

### Parameters

| Name       | Type                  | Description       | Notes                     |
| ---------- | --------------------- | ----------------- | ------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | --- | -------------------------------- |
| **status** | [\*\*&#39;active&#39; | &#39;pending&#39; | &#39;executed&#39;        | &#39;defeated&#39;**]**Array<&#39;active&#39; &#124; &#39;pending&#39; &#124; &#39;executed&#39; &#124; &#39;defeated&#39;>\*\* |     | (optional) defaults to undefined |
| **limit**  | [**number**]          |                   | (optional) defaults to 20 |

### Return type

**Array<Proposal>**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                   | Response headers |
| ----------- | ----------------------------- | ---------------- |
| **200**     | Proposals listed successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiGovernanceProposalsPost**

> Proposal apiGovernanceProposalsPost(createProposalRequest)

### Example

```typescript
import { GovernanceApi, Configuration, CreateProposalRequest } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new GovernanceApi(configuration);

let createProposalRequest: CreateProposalRequest; //

const { status, data } = await apiInstance.apiGovernanceProposalsPost(createProposalRequest);
```

### Parameters

| Name                      | Type                      | Description | Notes |
| ------------------------- | ------------------------- | ----------- | ----- |
| **createProposalRequest** | **CreateProposalRequest** |             |       |

### Return type

**Proposal**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                   | Response headers |
| ----------- | ----------------------------- | ---------------- |
| **201**     | Proposal created successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiGovernanceProposalsProposalIdVotePost**

> VoteResponse apiGovernanceProposalsProposalIdVotePost(voteRequest)

### Example

```typescript
import { GovernanceApi, Configuration, VoteRequest } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new GovernanceApi(configuration);

let proposalId: string; // (default to undefined)
let voteRequest: VoteRequest; //

const { status, data } = await apiInstance.apiGovernanceProposalsProposalIdVotePost(
  proposalId,
  voteRequest
);
```

### Parameters

| Name            | Type            | Description | Notes                 |
| --------------- | --------------- | ----------- | --------------------- |
| **voteRequest** | **VoteRequest** |             |                       |
| **proposalId**  | [**string**]    |             | defaults to undefined |

### Return type

**VoteResponse**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description            | Response headers |
| ----------- | ---------------------- | ---------------- |
| **200**     | Vote cast successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
