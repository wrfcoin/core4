# GovernanceAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                                    | HTTP request                                         | Description               |
| --------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- | ------------------------- |
| [**apiGovernanceProposalsGet**](GovernanceAPI.md#apigovernanceproposalsget)                               | **GET** /api/governance/proposals                    | List governance proposals |
| [**apiGovernanceProposalsPost**](GovernanceAPI.md#apigovernanceproposalspost)                             | **POST** /api/governance/proposals                   | Create new proposal       |
| [**apiGovernanceProposalsProposalIdVotePost**](GovernanceAPI.md#apigovernanceproposalsproposalidvotepost) | **POST** /api/governance/proposals/{proposalId}/vote | Vote on proposal          |

# **apiGovernanceProposalsGet**

```swift
    open class func apiGovernanceProposalsGet(status: Status_apiGovernanceProposalsGet? = nil, limit: Int? = nil, completion: @escaping (_ data: [Proposal]?, _ error: Error?) -> Void)
```

List governance proposals

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let status = "status_example" // String |  (optional)
let limit = 987 // Int |  (optional) (default to 20)

// List governance proposals
GovernanceAPI.apiGovernanceProposalsGet(status: status, limit: limit) { (response, error) in
    guard error == nil else {
        print(error)
        return
    }

    if (response) {
        dump(response)
    }
}
```

### Parameters

| Name       | Type       | Description | Notes                      |
| ---------- | ---------- | ----------- | -------------------------- |
| **status** | **String** |             | [optional]                 |
| **limit**  | **Int**    |             | [optional] [default to 20] |

### Return type

[**[Proposal]**](Proposal.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiGovernanceProposalsPost**

```swift
    open class func apiGovernanceProposalsPost(createProposalRequest: CreateProposalRequest, completion: @escaping (_ data: Proposal?, _ error: Error?) -> Void)
```

Create new proposal

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let createProposalRequest = CreateProposalRequest(title: "title_example", description: "description_example", actions: [123], votingPeriod: 123) // CreateProposalRequest |

// Create new proposal
GovernanceAPI.apiGovernanceProposalsPost(createProposalRequest: createProposalRequest) { (response, error) in
    guard error == nil else {
        print(error)
        return
    }

    if (response) {
        dump(response)
    }
}
```

### Parameters

| Name                      | Type                                                  | Description | Notes |
| ------------------------- | ----------------------------------------------------- | ----------- | ----- |
| **createProposalRequest** | [**CreateProposalRequest**](CreateProposalRequest.md) |             |

### Return type

[**Proposal**](Proposal.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiGovernanceProposalsProposalIdVotePost**

```swift
    open class func apiGovernanceProposalsProposalIdVotePost(proposalId: String, voteRequest: VoteRequest, completion: @escaping (_ data: VoteResponse?, _ error: Error?) -> Void)
```

Vote on proposal

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let proposalId = "proposalId_example" // String |
let voteRequest = VoteRequest(vote: "vote_example", votingPower: "votingPower_example", reason: "reason_example") // VoteRequest |

// Vote on proposal
GovernanceAPI.apiGovernanceProposalsProposalIdVotePost(proposalId: proposalId, voteRequest: voteRequest) { (response, error) in
    guard error == nil else {
        print(error)
        return
    }

    if (response) {
        dump(response)
    }
}
```

### Parameters

| Name            | Type                              | Description | Notes |
| --------------- | --------------------------------- | ----------- | ----- |
| **proposalId**  | **String**                        |             |
| **voteRequest** | [**VoteRequest**](VoteRequest.md) |             |

### Return type

[**VoteResponse**](VoteResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
