> **NOT the testnet API.** This client is generated from the backend developer-portal OpenAPI spec, not from the routes the testnet node (`launch_api`) serves, and is not published or kept in sync with the node. Do not assume its operations exist on the testnet. The hand-derived contract for the node is `docs/api/launch-api.openapi.yaml` (core4#3245); inventory and status: `docs/api/generated-code-inventory.md` (core4#2995).

## @wrfcoin/sdk-typescript@1.0.0

This generator creates TypeScript/JavaScript client that utilizes
[axios](https://github.com/axios/axios). The generated Node module can be used in the following
environments:

Environment

- Node.js
- Webpack
- Browserify

Language level

- ES5 - you must have a Promises/A+ library installed
- ES6

Module system

- CommonJS
- ES6 module system

It can be used in both TypeScript and JavaScript. In TypeScript, the definition will be
automatically resolved via `package.json`.
([Reference](https://www.typescriptlang.org/docs/handbook/declaration-files/consumption.html))

### Building

To build and compile the typescript sources to javascript use:

```
npm install
npm run build
```

### Publishing

First build the package then run `npm publish`

### Consuming

navigate to the folder of your consuming project and run one of the following commands.

_published:_

```
npm install @wrfcoin/sdk-typescript@1.0.0 --save
```

_unPublished (not recommended):_

```
npm install PATH_TO_GENERATED_PACKAGE --save
```

### Documentation for API Endpoints

All URIs are relative to *https://api.wrfcoin.com/v1*

| Class                 | Method                                                                                                         | HTTP request                                         | Description                           |
| --------------------- | -------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- | ------------------------------------- |
| _AIMLApi_             | [**apiAimlInferencePost**](docs/AIMLApi.md#apiaimlinferencepost)                                               | **POST** /api/aiml/inference                         | Run AI/ML inference                   |
| _AIMLApi_             | [**apiAimlModelsGet**](docs/AIMLApi.md#apiaimlmodelsget)                                                       | **GET** /api/aiml/models                             | List available AI/ML models           |
| _AnalyticsApi_        | [**apiAnalyticsGet**](docs/AnalyticsApi.md#apianalyticsget)                                                    | **GET** /api/analytics                               | Get network analytics                 |
| _BlockchainApi_       | [**apiBlockchainBlocksBlockHashGet**](docs/BlockchainApi.md#apiblockchainblocksblockhashget)                   | **GET** /api/blockchain/blocks/{blockHash}           | Get block by hash                     |
| _BlockchainApi_       | [**apiBlockchainBlocksGet**](docs/BlockchainApi.md#apiblockchainblocksget)                                     | **GET** /api/blockchain/blocks                       | Get blockchain blocks                 |
| _BlockchainApi_       | [**apiBlockchainStatusGet**](docs/BlockchainApi.md#apiblockchainstatusget)                                     | **GET** /api/blockchain/status                       | Get blockchain status                 |
| _BlockchainApi_       | [**apiBlockchainTransactionsTxHashGet**](docs/BlockchainApi.md#apiblockchaintransactionstxhashget)             | **GET** /api/blockchain/transactions/{txHash}        | Get transaction by hash               |
| _ComplianceApi_       | [**apiComplianceStatusGet**](docs/ComplianceApi.md#apicompliancestatusget)                                     | **GET** /api/compliance/status                       | Get compliance status                 |
| _ComputeApi_          | [**apiComputeProvidersGet**](docs/ComputeApi.md#apicomputeprovidersget)                                        | **GET** /api/compute/providers                       | List compute providers                |
| _ComputeApi_          | [**apiComputeTasksPost**](docs/ComputeApi.md#apicomputetaskspost)                                              | **POST** /api/compute/tasks                          | Submit compute task                   |
| _ComputeApi_          | [**apiComputeTasksTaskIdStatusGet**](docs/ComputeApi.md#apicomputetaskstaskidstatusget)                        | **GET** /api/compute/tasks/{taskId}/status           | Get compute task status               |
| _GovernanceApi_       | [**apiGovernanceProposalsGet**](docs/GovernanceApi.md#apigovernanceproposalsget)                               | **GET** /api/governance/proposals                    | List governance proposals             |
| _GovernanceApi_       | [**apiGovernanceProposalsPost**](docs/GovernanceApi.md#apigovernanceproposalspost)                             | **POST** /api/governance/proposals                   | Create new proposal                   |
| _GovernanceApi_       | [**apiGovernanceProposalsProposalIdVotePost**](docs/GovernanceApi.md#apigovernanceproposalsproposalidvotepost) | **POST** /api/governance/proposals/{proposalId}/vote | Vote on proposal                      |
| _IntegrationsApi_     | [**apiIntegrationsLangchainEnablePost**](docs/IntegrationsApi.md#apiintegrationslangchainenablepost)           | **POST** /api/integrations/langchain/enable          | Enable LangChain integration          |
| _IntegrationsApi_     | [**apiIntegrationsLangchainInsightsGet**](docs/IntegrationsApi.md#apiintegrationslangchaininsightsget)         | **GET** /api/integrations/langchain/insights         | Get LangChain insights                |
| _IntegrationsApi_     | [**apiIntegrationsLangchainStatusGet**](docs/IntegrationsApi.md#apiintegrationslangchainstatusget)             | **GET** /api/integrations/langchain/status           | Get LangChain integration status      |
| _MiningApi_           | [**apiMiningRewardsGet**](docs/MiningApi.md#apiminingrewardsget)                                               | **GET** /api/mining/rewards                          | Get mining rewards                    |
| _PerformanceApi_      | [**apiPerformanceMetricsGet**](docs/PerformanceApi.md#apiperformancemetricsget)                                | **GET** /api/performance/metrics                     | Get performance metrics               |
| _PerformanceApi_      | [**apiPerformanceOptimizationPost**](docs/PerformanceApi.md#apiperformanceoptimizationpost)                    | **POST** /api/performance/optimization               | Trigger performance optimization      |
| _SecurityApi_         | [**apiSecurityStatsGet**](docs/SecurityApi.md#apisecuritystatsget)                                             | **GET** /api/security/stats                          | Get security statistics               |
| _SmartContractsApi_   | [**apiContractsCallPost**](docs/SmartContractsApi.md#apicontractscallpost)                                     | **POST** /api/contracts/call                         | Call contract function                |
| _SmartContractsApi_   | [**apiContractsDeployPost**](docs/SmartContractsApi.md#apicontractsdeploypost)                                 | **POST** /api/contracts/deploy                       | Deploy smart contract                 |
| _SmartContractsApi_   | [**apiContractsEstimateGasPost**](docs/SmartContractsApi.md#apicontractsestimategaspost)                       | **POST** /api/contracts/estimate-gas                 | Estimate gas for contract call        |
| _SmartContractsApi_   | [**apiContractsQueryPost**](docs/SmartContractsApi.md#apicontractsquerypost)                                   | **POST** /api/contracts/query                        | Query contract state                  |
| _SystemApi_           | [**apiHealthGet**](docs/SystemApi.md#apihealthget)                                                             | **GET** /api/health                                  | API health check                      |
| _SystemApi_           | [**healthGet**](docs/SystemApi.md#healthget)                                                                   | **GET** /health                                      | System health check                   |
| _WalletApi_           | [**apiWalletGenerateMnemonicPost**](docs/WalletApi.md#apiwalletgeneratemnemonicpost)                           | **POST** /api/wallet/generate-mnemonic               | Generate mnemonic phrase              |
| _WalletApi_           | [**apiWalletHardwareDevicesGet**](docs/WalletApi.md#apiwallethardwaredevicesget)                               | **GET** /api/wallet/hardware/devices                 | List connected hardware wallets       |
| _WalletApi_           | [**apiWalletHardwareSignPost**](docs/WalletApi.md#apiwallethardwaresignpost)                                   | **POST** /api/wallet/hardware/sign                   | Sign transaction with hardware wallet |
| _WalletApi_           | [**apiWalletValidateMnemonicPost**](docs/WalletApi.md#apiwalletvalidatemnemonicpost)                           | **POST** /api/wallet/validate-mnemonic               | Validate mnemonic phrase              |
| _WeatherDataApi_      | [**apiWeatherDataGet**](docs/WeatherDataApi.md#apiweatherdataget)                                              | **GET** /api/weather/data                            | Get weather data                      |
| _WeatherDataApi_      | [**apiWeatherSecurityStatusGet**](docs/WeatherDataApi.md#apiweathersecuritystatusget)                          | **GET** /api/weather/security-status                 | Get weather security status           |
| _WeatherDataApi_      | [**apiWeatherSubmitPost**](docs/WeatherDataApi.md#apiweathersubmitpost)                                        | **POST** /api/weather/submit                         | Submit weather data                   |

### Documentation For Models

- [ApiAimlInferencePost200Response](docs/ApiAimlInferencePost200Response.md)
- [ApiAimlInferencePostRequest](docs/ApiAimlInferencePostRequest.md)
- [ApiAimlModelsGet200Response](docs/ApiAimlModelsGet200Response.md)
- [ApiAimlModelsGet200ResponseModelsInner](docs/ApiAimlModelsGet200ResponseModelsInner.md)
- [ApiAimlModelsGet200ResponseModelsInnerPricing](docs/ApiAimlModelsGet200ResponseModelsInnerPricing.md)
- [ApiAimlModelsGet200ResponseModelsInnerRequirements](docs/ApiAimlModelsGet200ResponseModelsInnerRequirements.md)
- [ApiAnalyticsGet200Response](docs/ApiAnalyticsGet200Response.md)
- [ApiBlockchainBlocksGet200Response](docs/ApiBlockchainBlocksGet200Response.md)
- [ApiComplianceStatusGet200Response](docs/ApiComplianceStatusGet200Response.md)
- [ApiComputeProvidersGet200Response](docs/ApiComputeProvidersGet200Response.md)
- [ApiComputeProvidersGet200ResponseProvidersInner](docs/ApiComputeProvidersGet200ResponseProvidersInner.md)
- [ApiComputeProvidersGet200ResponseProvidersInnerResources](docs/ApiComputeProvidersGet200ResponseProvidersInnerResources.md)
- [ApiComputeTasksPost200Response](docs/ApiComputeTasksPost200Response.md)
- [ApiComputeTasksPostRequest](docs/ApiComputeTasksPostRequest.md)
- [ApiComputeTasksPostRequestResourceRequirements](docs/ApiComputeTasksPostRequestResourceRequirements.md)
- [ApiComputeTasksTaskIdStatusGet200Response](docs/ApiComputeTasksTaskIdStatusGet200Response.md)
- [ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage](docs/ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage.md)
- [ApiContractsEstimateGasPost200Response](docs/ApiContractsEstimateGasPost200Response.md)
- [ApiIntegrationsLangchainEnablePost200Response](docs/ApiIntegrationsLangchainEnablePost200Response.md)
- [ApiIntegrationsLangchainEnablePost200ResponseData](docs/ApiIntegrationsLangchainEnablePost200ResponseData.md)
- [ApiIntegrationsLangchainInsightsGet200Response](docs/ApiIntegrationsLangchainInsightsGet200Response.md)
- [ApiIntegrationsLangchainInsightsGet200ResponseData](docs/ApiIntegrationsLangchainInsightsGet200ResponseData.md)
- [ApiIntegrationsLangchainStatusGet200Response](docs/ApiIntegrationsLangchainStatusGet200Response.md)
- [ApiMiningRewardsGet200Response](docs/ApiMiningRewardsGet200Response.md)
- [ApiPerformanceMetricsGet200Response](docs/ApiPerformanceMetricsGet200Response.md)
- [ApiPerformanceMetricsGet200ResponseMetrics](docs/ApiPerformanceMetricsGet200ResponseMetrics.md)
- [ApiPerformanceMetricsGet200ResponseMetricsLatency](docs/ApiPerformanceMetricsGet200ResponseMetricsLatency.md)
- [ApiPerformanceMetricsGet200ResponseMetricsThroughput](docs/ApiPerformanceMetricsGet200ResponseMetricsThroughput.md)
- [ApiPerformanceOptimizationPost200Response](docs/ApiPerformanceOptimizationPost200Response.md)
- [ApiPerformanceOptimizationPost200ResponseExpectedImprovements](docs/ApiPerformanceOptimizationPost200ResponseExpectedImprovements.md)
- [ApiPerformanceOptimizationPostRequest](docs/ApiPerformanceOptimizationPostRequest.md)
- [ApiSecurityStatsGet200Response](docs/ApiSecurityStatsGet200Response.md)
- [ApiWalletGenerateMnemonicPost200Response](docs/ApiWalletGenerateMnemonicPost200Response.md)
- [ApiWalletValidateMnemonicPost200Response](docs/ApiWalletValidateMnemonicPost200Response.md)
- [ApiWalletValidateMnemonicPostRequest](docs/ApiWalletValidateMnemonicPostRequest.md)
- [Block](docs/Block.md)
- [BlockchainStatus](docs/BlockchainStatus.md)
- [ComplianceStatus](docs/ComplianceStatus.md)
- [ComplianceStatusCertificationsInner](docs/ComplianceStatusCertificationsInner.md)
- [ComplianceStatusDataProtection](docs/ComplianceStatusDataProtection.md)
- [ContractCallRequest](docs/ContractCallRequest.md)
- [ContractCallResponse](docs/ContractCallResponse.md)
- [ContractDeployRequest](docs/ContractDeployRequest.md)
- [ContractDeployResponse](docs/ContractDeployResponse.md)
- [ContractQueryRequest](docs/ContractQueryRequest.md)
- [ContractQueryResponse](docs/ContractQueryResponse.md)
- [CreateProposalRequest](docs/CreateProposalRequest.md)
- [HardwareDevice](docs/HardwareDevice.md)
- [HardwareSignRequest](docs/HardwareSignRequest.md)
- [HardwareSignResponse](docs/HardwareSignResponse.md)
- [HealthGet200Response](docs/HealthGet200Response.md)
- [HealthStatus](docs/HealthStatus.md)
- [HealthStatusBlockchain](docs/HealthStatusBlockchain.md)
- [HealthStatusDatabase](docs/HealthStatusDatabase.md)
- [HealthStatusDatabaseConnectionPool](docs/HealthStatusDatabaseConnectionPool.md)
- [HealthStatusDatabaseQueryPerformance](docs/HealthStatusDatabaseQueryPerformance.md)
- [HealthStatusDatabaseStorage](docs/HealthStatusDatabaseStorage.md)
- [HealthStatusPerformance](docs/HealthStatusPerformance.md)
- [HealthStatusPerformanceCpu](docs/HealthStatusPerformanceCpu.md)
- [HealthStatusPerformanceNetwork](docs/HealthStatusPerformanceNetwork.md)
- [HealthStatusSecurity](docs/HealthStatusSecurity.md)
- [HealthStatusSecurityCertificates](docs/HealthStatusSecurityCertificates.md)
- [HealthStatusSecurityFirewall](docs/HealthStatusSecurityFirewall.md)
- [HealthStatusSecurityVulnerabilities](docs/HealthStatusSecurityVulnerabilities.md)
- [HealthStatusServicesInner](docs/HealthStatusServicesInner.md)
- [LangChainInsight](docs/LangChainInsight.md)
- [LangChainStatus](docs/LangChainStatus.md)
- [LangChainStatusAnalytics](docs/LangChainStatusAnalytics.md)
- [Location](docs/Location.md)
- [MiningReward](docs/MiningReward.md)
- [MiningRewardBlockchain](docs/MiningRewardBlockchain.md)
- [MiningRewardMultipliers](docs/MiningRewardMultipliers.md)
- [MiningRewardPerformance](docs/MiningRewardPerformance.md)
- [MiningRewardSource](docs/MiningRewardSource.md)
- [ModelError](docs/ModelError.md)
- [NetworkAnalytics](docs/NetworkAnalytics.md)
- [NetworkAnalyticsCrossChain](docs/NetworkAnalyticsCrossChain.md)
- [NetworkAnalyticsOverview](docs/NetworkAnalyticsOverview.md)
- [NetworkAnalyticsPerformance](docs/NetworkAnalyticsPerformance.md)
- [NetworkAnalyticsSecurity](docs/NetworkAnalyticsSecurity.md)
- [NetworkAnalyticsWeather](docs/NetworkAnalyticsWeather.md)
- [Pagination](docs/Pagination.md)
- [Proposal](docs/Proposal.md)
- [SecurityStats](docs/SecurityStats.md)
- [Transaction](docs/Transaction.md)
- [VoteRequest](docs/VoteRequest.md)
- [VoteResponse](docs/VoteResponse.md)
- [WeatherData](docs/WeatherData.md)
- [WeatherDataSubmission](docs/WeatherDataSubmission.md)
- [WeatherDataSubmissionQuality](docs/WeatherDataSubmissionQuality.md)
- [WeatherDataSubmissionSensorDataInner](docs/WeatherDataSubmissionSensorDataInner.md)
- [WeatherMeasurement](docs/WeatherMeasurement.md)
- [WeatherSecurityStatus](docs/WeatherSecurityStatus.md)
- [WeatherSecurityStatusApiSecurity](docs/WeatherSecurityStatusApiSecurity.md)
- [WeatherSecurityStatusDataValidation](docs/WeatherSecurityStatusDataValidation.md)
- [WeatherSubmissionResponse](docs/WeatherSubmissionResponse.md)
- [WeatherSubmissionResponseData](docs/WeatherSubmissionResponseData.md)
- [WeatherSubmissionResponseDataBlockchain](docs/WeatherSubmissionResponseDataBlockchain.md)
- [WeatherSubmissionResponseDataQuality](docs/WeatherSubmissionResponseDataQuality.md)
- [WeatherSubmissionResponseDataRewards](docs/WeatherSubmissionResponseDataRewards.md)
- [WeatherSubmissionResponseDataSensorsInner](docs/WeatherSubmissionResponseDataSensorsInner.md)

<a id="documentation-for-authorization"></a>

## Documentation For Authorization

Authentication schemes defined for the API: <a id="ApiKeyAuth"></a>

### ApiKeyAuth

- **Type**: API key
- **API key parameter name**: X-API-Key
- **Location**: HTTP header

<a id="BearerAuth"></a>

### BearerAuth

- **Type**: Bearer authentication (JWT)
