# GovernanceApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                                    | HTTP request                                         | Description               |
| --------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- | ------------------------- |
| [**apiGovernanceProposalsGet**](GovernanceApi.md#apiGovernanceProposalsGet)                               | **GET** /api/governance/proposals                    | List governance proposals |
| [**apiGovernanceProposalsPost**](GovernanceApi.md#apiGovernanceProposalsPost)                             | **POST** /api/governance/proposals                   | Create new proposal       |
| [**apiGovernanceProposalsProposalIdVotePost**](GovernanceApi.md#apiGovernanceProposalsProposalIdVotePost) | **POST** /api/governance/proposals/{proposalId}/vote | Vote on proposal          |

<a id="apiGovernanceProposalsGet"></a>

# **apiGovernanceProposalsGet**

> List&lt;Proposal&gt; apiGovernanceProposalsGet(status, limit)

List governance proposals

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.GovernanceApi;

public class Example {
  public static void main(String[] args) {
    ApiClient defaultClient = Configuration.getDefaultApiClient();
    defaultClient.setBasePath("https://api.wrfcoin.com/v1");

    // Configure API key authorization: ApiKeyAuth
    ApiKeyAuth ApiKeyAuth = (ApiKeyAuth) defaultClient.getAuthentication("ApiKeyAuth");
    ApiKeyAuth.setApiKey("YOUR API KEY");
    // Uncomment the following line to set a prefix for the API key, e.g. "Token" (defaults to null)
    //ApiKeyAuth.setApiKeyPrefix("Token");

    // Configure HTTP bearer authorization: BearerAuth
    HttpBearerAuth BearerAuth = (HttpBearerAuth) defaultClient.getAuthentication("BearerAuth");
    BearerAuth.setBearerToken("BEARER TOKEN");

    GovernanceApi apiInstance = new GovernanceApi(defaultClient);
    String status = "active"; // String |
    Integer limit = 20; // Integer |
    try {
      List<Proposal> result = apiInstance.apiGovernanceProposalsGet(status, limit);
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling GovernanceApi#apiGovernanceProposalsGet");
      System.err.println("Status code: " + e.getCode());
      System.err.println("Reason: " + e.getResponseBody());
      System.err.println("Response headers: " + e.getResponseHeaders());
      e.printStackTrace();
    }
  }
}
```

### Parameters

| Name       | Type        | Description | Notes                                                  |
| ---------- | ----------- | ----------- | ------------------------------------------------------ |
| **status** | **String**  |             | [optional] [enum: active, pending, executed, defeated] |
| **limit**  | **Integer** |             | [optional] [default to 20]                             |

### Return type

[**List&lt;Proposal&gt;**](Proposal.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                   | Response headers |
| ----------- | ----------------------------- | ---------------- |
| **200**     | Proposals listed successfully | -                |

<a id="apiGovernanceProposalsPost"></a>

# **apiGovernanceProposalsPost**

> Proposal apiGovernanceProposalsPost(createProposalRequest)

Create new proposal

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.GovernanceApi;

public class Example {
  public static void main(String[] args) {
    ApiClient defaultClient = Configuration.getDefaultApiClient();
    defaultClient.setBasePath("https://api.wrfcoin.com/v1");

    // Configure API key authorization: ApiKeyAuth
    ApiKeyAuth ApiKeyAuth = (ApiKeyAuth) defaultClient.getAuthentication("ApiKeyAuth");
    ApiKeyAuth.setApiKey("YOUR API KEY");
    // Uncomment the following line to set a prefix for the API key, e.g. "Token" (defaults to null)
    //ApiKeyAuth.setApiKeyPrefix("Token");

    // Configure HTTP bearer authorization: BearerAuth
    HttpBearerAuth BearerAuth = (HttpBearerAuth) defaultClient.getAuthentication("BearerAuth");
    BearerAuth.setBearerToken("BEARER TOKEN");

    GovernanceApi apiInstance = new GovernanceApi(defaultClient);
    CreateProposalRequest createProposalRequest = new CreateProposalRequest(); // CreateProposalRequest |
    try {
      Proposal result = apiInstance.apiGovernanceProposalsPost(createProposalRequest);
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling GovernanceApi#apiGovernanceProposalsPost");
      System.err.println("Status code: " + e.getCode());
      System.err.println("Reason: " + e.getResponseBody());
      System.err.println("Response headers: " + e.getResponseHeaders());
      e.printStackTrace();
    }
  }
}
```

### Parameters

| Name                      | Type                                                  | Description | Notes |
| ------------------------- | ----------------------------------------------------- | ----------- | ----- |
| **createProposalRequest** | [**CreateProposalRequest**](CreateProposalRequest.md) |             |       |

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

<a id="apiGovernanceProposalsProposalIdVotePost"></a>

# **apiGovernanceProposalsProposalIdVotePost**

> VoteResponse apiGovernanceProposalsProposalIdVotePost(proposalId, voteRequest)

Vote on proposal

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.GovernanceApi;

public class Example {
  public static void main(String[] args) {
    ApiClient defaultClient = Configuration.getDefaultApiClient();
    defaultClient.setBasePath("https://api.wrfcoin.com/v1");

    // Configure API key authorization: ApiKeyAuth
    ApiKeyAuth ApiKeyAuth = (ApiKeyAuth) defaultClient.getAuthentication("ApiKeyAuth");
    ApiKeyAuth.setApiKey("YOUR API KEY");
    // Uncomment the following line to set a prefix for the API key, e.g. "Token" (defaults to null)
    //ApiKeyAuth.setApiKeyPrefix("Token");

    // Configure HTTP bearer authorization: BearerAuth
    HttpBearerAuth BearerAuth = (HttpBearerAuth) defaultClient.getAuthentication("BearerAuth");
    BearerAuth.setBearerToken("BEARER TOKEN");

    GovernanceApi apiInstance = new GovernanceApi(defaultClient);
    String proposalId = "proposalId_example"; // String |
    VoteRequest voteRequest = new VoteRequest(); // VoteRequest |
    try {
      VoteResponse result = apiInstance.apiGovernanceProposalsProposalIdVotePost(proposalId, voteRequest);
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling GovernanceApi#apiGovernanceProposalsProposalIdVotePost");
      System.err.println("Status code: " + e.getCode());
      System.err.println("Reason: " + e.getResponseBody());
      System.err.println("Response headers: " + e.getResponseHeaders());
      e.printStackTrace();
    }
  }
}
```

### Parameters

| Name            | Type                              | Description | Notes |
| --------------- | --------------------------------- | ----------- | ----- |
| **proposalId**  | **String**                        |             |       |
| **voteRequest** | [**VoteRequest**](VoteRequest.md) |             |       |

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
