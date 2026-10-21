# wrfcoin_sdk.GovernanceApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                                                | HTTP request                                         | Description               |
| --------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- | ------------------------- |
| [**api_governance_proposals_get**](GovernanceApi.md#api_governance_proposals_get)                                     | **GET** /api/governance/proposals                    | List governance proposals |
| [**api_governance_proposals_post**](GovernanceApi.md#api_governance_proposals_post)                                   | **POST** /api/governance/proposals                   | Create new proposal       |
| [**api_governance_proposals_proposal_id_vote_post**](GovernanceApi.md#api_governance_proposals_proposal_id_vote_post) | **POST** /api/governance/proposals/{proposalId}/vote | Vote on proposal          |

# **api_governance_proposals_get**

> List[Proposal] api_governance_proposals_get(status=status, limit=limit)

List governance proposals

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.proposal import Proposal
from wrfcoin_sdk.rest import ApiException
from pprint import pprint

# Defining the host is optional and defaults to https://api.wrfcoin.com/v1
# See configuration.py for a list of all supported configuration parameters.
configuration = wrfcoin_sdk.Configuration(
    host = "https://api.wrfcoin.com/v1"
)

# The client must configure the authentication and authorization parameters
# in accordance with the API server security policy.
# Examples for each auth method are provided below, use the example that
# satisfies your auth use case.

# Configure API key authorization: ApiKeyAuth
configuration.api_key['ApiKeyAuth'] = os.environ["API_KEY"]

# Uncomment below to setup prefix (e.g. Bearer) for API key, if needed
# configuration.api_key_prefix['ApiKeyAuth'] = 'Bearer'

# Configure Bearer authorization (JWT): BearerAuth
configuration = wrfcoin_sdk.Configuration(
    access_token = os.environ["BEARER_TOKEN"]
)

# Enter a context with an instance of the API client
with wrfcoin_sdk.ApiClient(configuration) as api_client:
    # Create an instance of the API class
    api_instance = wrfcoin_sdk.GovernanceApi(api_client)
    status = 'status_example' # str |  (optional)
    limit = 20 # int |  (optional) (default to 20)

    try:
        # List governance proposals
        api_response = api_instance.api_governance_proposals_get(status=status, limit=limit)
        print("The response of GovernanceApi->api_governance_proposals_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling GovernanceApi->api_governance_proposals_get: %s\n" % e)
```

### Parameters

| Name       | Type    | Description | Notes                      |
| ---------- | ------- | ----------- | -------------------------- |
| **status** | **str** |             | [optional]                 |
| **limit**  | **int** |             | [optional] [default to 20] |

### Return type

[**List[Proposal]**](Proposal.md)

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

# **api_governance_proposals_post**

> Proposal api_governance_proposals_post(create_proposal_request)

Create new proposal

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.create_proposal_request import CreateProposalRequest
from wrfcoin_sdk.models.proposal import Proposal
from wrfcoin_sdk.rest import ApiException
from pprint import pprint

# Defining the host is optional and defaults to https://api.wrfcoin.com/v1
# See configuration.py for a list of all supported configuration parameters.
configuration = wrfcoin_sdk.Configuration(
    host = "https://api.wrfcoin.com/v1"
)

# The client must configure the authentication and authorization parameters
# in accordance with the API server security policy.
# Examples for each auth method are provided below, use the example that
# satisfies your auth use case.

# Configure API key authorization: ApiKeyAuth
configuration.api_key['ApiKeyAuth'] = os.environ["API_KEY"]

# Uncomment below to setup prefix (e.g. Bearer) for API key, if needed
# configuration.api_key_prefix['ApiKeyAuth'] = 'Bearer'

# Configure Bearer authorization (JWT): BearerAuth
configuration = wrfcoin_sdk.Configuration(
    access_token = os.environ["BEARER_TOKEN"]
)

# Enter a context with an instance of the API client
with wrfcoin_sdk.ApiClient(configuration) as api_client:
    # Create an instance of the API class
    api_instance = wrfcoin_sdk.GovernanceApi(api_client)
    create_proposal_request = wrfcoin_sdk.CreateProposalRequest() # CreateProposalRequest |

    try:
        # Create new proposal
        api_response = api_instance.api_governance_proposals_post(create_proposal_request)
        print("The response of GovernanceApi->api_governance_proposals_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling GovernanceApi->api_governance_proposals_post: %s\n" % e)
```

### Parameters

| Name                        | Type                                                  | Description | Notes |
| --------------------------- | ----------------------------------------------------- | ----------- | ----- |
| **create_proposal_request** | [**CreateProposalRequest**](CreateProposalRequest.md) |             |

### Return type

[**Proposal**](Proposal.md)

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

# **api_governance_proposals_proposal_id_vote_post**

> VoteResponse api_governance_proposals_proposal_id_vote_post(proposal_id, vote_request)

Vote on proposal

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.vote_request import VoteRequest
from wrfcoin_sdk.models.vote_response import VoteResponse
from wrfcoin_sdk.rest import ApiException
from pprint import pprint

# Defining the host is optional and defaults to https://api.wrfcoin.com/v1
# See configuration.py for a list of all supported configuration parameters.
configuration = wrfcoin_sdk.Configuration(
    host = "https://api.wrfcoin.com/v1"
)

# The client must configure the authentication and authorization parameters
# in accordance with the API server security policy.
# Examples for each auth method are provided below, use the example that
# satisfies your auth use case.

# Configure API key authorization: ApiKeyAuth
configuration.api_key['ApiKeyAuth'] = os.environ["API_KEY"]

# Uncomment below to setup prefix (e.g. Bearer) for API key, if needed
# configuration.api_key_prefix['ApiKeyAuth'] = 'Bearer'

# Configure Bearer authorization (JWT): BearerAuth
configuration = wrfcoin_sdk.Configuration(
    access_token = os.environ["BEARER_TOKEN"]
)

# Enter a context with an instance of the API client
with wrfcoin_sdk.ApiClient(configuration) as api_client:
    # Create an instance of the API class
    api_instance = wrfcoin_sdk.GovernanceApi(api_client)
    proposal_id = 'proposal_id_example' # str |
    vote_request = wrfcoin_sdk.VoteRequest() # VoteRequest |

    try:
        # Vote on proposal
        api_response = api_instance.api_governance_proposals_proposal_id_vote_post(proposal_id, vote_request)
        print("The response of GovernanceApi->api_governance_proposals_proposal_id_vote_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling GovernanceApi->api_governance_proposals_proposal_id_vote_post: %s\n" % e)
```

### Parameters

| Name             | Type                              | Description | Notes |
| ---------------- | --------------------------------- | ----------- | ----- |
| **proposal_id**  | **str**                           |             |
| **vote_request** | [**VoteRequest**](VoteRequest.md) |             |

### Return type

[**VoteResponse**](VoteResponse.md)

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
