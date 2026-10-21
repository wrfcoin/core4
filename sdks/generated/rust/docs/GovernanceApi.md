# \GovernanceApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                                                | HTTP request                                         | Description               |
| --------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- | ------------------------- |
| [**api_governance_proposals_get**](GovernanceApi.md#api_governance_proposals_get)                                     | **GET** /api/governance/proposals                    | List governance proposals |
| [**api_governance_proposals_post**](GovernanceApi.md#api_governance_proposals_post)                                   | **POST** /api/governance/proposals                   | Create new proposal       |
| [**api_governance_proposals_proposal_id_vote_post**](GovernanceApi.md#api_governance_proposals_proposal_id_vote_post) | **POST** /api/governance/proposals/{proposalId}/vote | Vote on proposal          |

## api_governance_proposals_get

> Vec<models::Proposal> api_governance_proposals_get(status, limit) List governance proposals

### Parameters

| Name       | Type               | Description | Required | Notes           |
| ---------- | ------------------ | ----------- | -------- | --------------- |
| **status** | Option<**String**> |             |          |
| **limit**  | Option<**i32**>    |             |          | [default to 20] |

### Return type

[**Vec<models::Proposal>**](Proposal.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_governance_proposals_post

> models::Proposal api_governance_proposals_post(create_proposal_request) Create new proposal

### Parameters

| Name                        | Type                                                  | Description | Required   | Notes |
| --------------------------- | ----------------------------------------------------- | ----------- | ---------- | ----- |
| **create_proposal_request** | [**CreateProposalRequest**](CreateProposalRequest.md) |             | [required] |

### Return type

[**models::Proposal**](Proposal.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_governance_proposals_proposal_id_vote_post

> models::VoteResponse api_governance_proposals_proposal_id_vote_post(proposal_id, vote_request)
> Vote on proposal

### Parameters

| Name             | Type                              | Description | Required   | Notes |
| ---------------- | --------------------------------- | ----------- | ---------- | ----- |
| **proposal_id**  | **String**                        |             | [required] |
| **vote_request** | [**VoteRequest**](VoteRequest.md) |             | [required] |

### Return type

[**models::VoteResponse**](VoteResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
